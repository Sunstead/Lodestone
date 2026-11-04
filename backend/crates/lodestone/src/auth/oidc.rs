//! Access tokens from an OpenID Connect provider (Authentik), checked
//! offline against its published keys.
//!
//! Ported from Solstice Sync (`apps/sync/src/auth/oidc.rs`), which took it
//! from Cosmos/Helion, so every Sunstead service checks tokens the same
//! way. Lodestone never
//! sees a password or a refresh token. The app signs in with the provider,
//! then presents the short-lived access token (a JWT) here. We verify its
//! signature against the provider's JWKS, and its issuer, audience and
//! expiry, and read who it is and their groups.
//!
//! Keys are fetched at startup and cached by `kid`. A token signed with an
//! unknown `kid` (the provider rotated keys) triggers one refetch, at most
//! once a minute, so a flood of junk tokens can't make us hammer the
//! provider. If the provider has never been reachable the result is 503,
//! not 401: the token might be fine, and the app should retry rather than
//! send the user back through sign-in.

use crate::config::OidcConfig;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, Instant},
};

const REFETCH_AFTER: Duration = Duration::from_secs(60);
const REFRESH_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const LEEWAY_SECS: u64 = 60;

/// Asymmetric only. HS256 would mean the provider signs with the client
/// secret, which a public client doesn't have and we must never accept.
const ALLOWED: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

#[derive(Debug)]
pub enum VerifyError {
    /// Bad, expired or foreign token. 401: sign in again.
    Invalid(String),
    /// Can't check right now. 503: retry.
    Unavailable(String),
}

/// Who a valid token says the caller is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub subject: String,
    pub username: String,
    /// The provider's display name, or the username.
    pub display_name: String,
    pub groups: Vec<String>,
    /// When the token expires (seconds since the Unix epoch). A WebSocket
    /// opened with it is closed then, so it can't outlive the sign-in.
    pub expires: u64,
}

pub struct OidcVerifier {
    pub(crate) issuer: String,
    audience: String,
    discovery_url: String,
    http: reqwest::Client,
    keys: RwLock<Keys>,
    /// Single-flight: concurrent requests with a new `kid` share one fetch.
    fetching: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct Keys {
    by_kid: HashMap<String, DecodingKey>,
    /// Keys published without a `kid`, used when a token has none either.
    anonymous: Vec<DecodingKey>,
    fetched_at: Option<Instant>,
    last_attempt: Option<Instant>,
    endpoints: Endpoints,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
    #[serde(default)]
    authorization_endpoint: Option<String>,
    #[serde(default)]
    token_endpoint: Option<String>,
    #[serde(default)]
    end_session_endpoint: Option<String>,
}

/// Where the web app's sign-in goes, from discovery.
#[derive(Clone, Debug, Default)]
struct Endpoints {
    authorization: Option<String>,
    token: Option<String>,
    end_session: Option<String>,
}

/// An ID token's claims, for web sign-in: an access token's, plus the nonce
/// that ties it to this sign-in.
#[derive(Deserialize)]
struct IdClaims {
    #[serde(flatten)]
    claims: Claims,
    #[serde(default)]
    nonce: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
}

fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The PKCE S256 challenge for a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
}

impl OidcVerifier {
    pub fn new(cfg: &OidcConfig) -> Self {
        let discovery_url = cfg
            .discovery_url
            .clone()
            .unwrap_or_else(|| format!("{}/.well-known/openid-configuration", cfg.issuer.trim_end_matches('/')));
        Self {
            issuer: cfg.issuer.clone(),
            audience: cfg.client_id.clone(),
            discovery_url,
            http: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .user_agent(concat!("lodestone/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
            keys: RwLock::new(Keys::default()),
            fetching: tokio::sync::Mutex::new(()),
        }
    }

    /// Fetches keys until it works, then refreshes them every few hours so
    /// a routine rotation never costs a request a refetch.
    pub fn spawn_refresh(self: &std::sync::Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match this.fetch_keys().await {
                    Ok(n) => {
                        tracing::info!(issuer = %this.issuer, keys = n, "identity provider keys loaded");
                        backoff = Duration::from_secs(1);
                        tokio::time::sleep(REFRESH_EVERY).await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            url = %this.discovery_url,
                            error = %e,
                            retry_in = backoff.as_secs(),
                            "cannot load identity provider keys"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                    }
                }
            }
        });
    }

    async fn fetch_keys(&self) -> Result<usize, String> {
        let _guard = self.fetching.lock().await;
        self.fetch_locked().await
    }

    /// For an unknown `kid`: fetches unless a fetch was tried within the
    /// last minute. Requests that queued behind one find it done and use its
    /// keys, so a burst of them costs the provider one fetch, not one each.
    async fn refetch_if_due(&self) -> Result<(), String> {
        let _guard = self.fetching.lock().await;
        let due = self
            .keys
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .last_attempt
            .is_none_or(|t| t.elapsed() >= REFETCH_AFTER);
        if due {
            self.fetch_locked().await.map(|_| ())
        } else {
            Ok(())
        }
    }

    /// Call with `fetching` held.
    async fn fetch_locked(&self) -> Result<usize, String> {
        self.keys.write().unwrap_or_else(|p| p.into_inner()).last_attempt = Some(Instant::now());

        let discovery: Discovery = self.get_json(&self.discovery_url).await?;
        if !same_issuer(&discovery.issuer, &self.issuer) {
            return Err(format!(
                "discovery says the issuer is {}, but the server is configured for {}",
                discovery.issuer, self.issuer
            ));
        }
        let set: JwkSet = self.get_json(&discovery.jwks_uri).await?;

        let mut keys = Keys::default();
        for jwk in &set.keys {
            let Ok(key) = DecodingKey::from_jwk(jwk) else {
                continue;
            };
            match &jwk.common.key_id {
                Some(kid) => {
                    keys.by_kid.insert(kid.clone(), key);
                }
                None => keys.anonymous.push(key),
            }
        }
        let count = keys.by_kid.len() + keys.anonymous.len();
        if count == 0 {
            return Err("the provider published no usable signing keys".into());
        }
        keys.fetched_at = Some(Instant::now());
        keys.last_attempt = keys.fetched_at;
        keys.endpoints = Endpoints {
            authorization: discovery.authorization_endpoint,
            token: discovery.token_endpoint,
            end_session: discovery.end_session_endpoint,
        };
        *self.keys.write().unwrap_or_else(|p| p.into_inner()) = keys;
        Ok(count)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let res = self.http.get(url).send().await.map_err(|e| format!("{url}: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("{url}: HTTP {}", res.status()));
        }
        res.json().await.map_err(|e| format!("{url}: {e}"))
    }

    /// Keys to try for this `kid`, refetching once if it's new to us.
    async fn keys_for(&self, kid: Option<&str>) -> Result<Vec<DecodingKey>, VerifyError> {
        let lookup = |keys: &Keys| -> Vec<DecodingKey> {
            match kid {
                Some(kid) => keys.by_kid.get(kid).cloned().into_iter().collect(),
                None => keys.anonymous.iter().chain(keys.by_kid.values()).cloned().collect(),
            }
        };

        let (found, loaded, may_refetch) = {
            let keys = self.keys.read().unwrap_or_else(|p| p.into_inner());
            let found = lookup(&keys);
            let may_refetch = keys.last_attempt.is_none_or(|t| t.elapsed() >= REFETCH_AFTER);
            (found, keys.fetched_at.is_some(), may_refetch)
        };
        if !found.is_empty() {
            return Ok(found);
        }
        if may_refetch {
            if let Err(e) = self.refetch_if_due().await {
                if !loaded {
                    return Err(VerifyError::Unavailable(format!("cannot reach the identity provider: {e}")));
                }
                tracing::warn!(error = %e, "key refetch failed; keeping the keys we have");
            }
            let keys = self.keys.read().unwrap_or_else(|p| p.into_inner());
            let found = lookup(&keys);
            if !found.is_empty() {
                return Ok(found);
            }
            if keys.fetched_at.is_none() {
                return Err(VerifyError::Unavailable("the identity provider's keys are not loaded yet".into()));
            }
        } else if !loaded {
            return Err(VerifyError::Unavailable("the identity provider's keys are not loaded yet".into()));
        }
        Err(VerifyError::Invalid("token signed with an unknown key".into()))
    }

    pub async fn verify(&self, token: &str) -> Result<Principal, VerifyError> {
        let claims: Claims = self.decode(token).await?;
        self.principal(claims)
    }

    /// Checks a JWT's signature, issuer, audience and expiry, and reads its
    /// claims.
    async fn decode<C: serde::de::DeserializeOwned>(&self, token: &str) -> Result<C, VerifyError> {
        let header = decode_header(token).map_err(|e| VerifyError::Invalid(format!("not a JWT: {e}")))?;
        if !ALLOWED.contains(&header.alg) {
            return Err(VerifyError::Invalid(format!("{:?} tokens are not accepted", header.alg)));
        }

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str(), self.issuer.trim_end_matches('/')]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.leeway = LEEWAY_SECS;
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);

        let mut last = None;
        for key in self.keys_for(header.kid.as_deref()).await? {
            match decode::<C>(token, &key, &validation) {
                Ok(data) => return Ok(data.claims),
                Err(e) => {
                    last = Some(e);
                }
            }
        }
        Err(VerifyError::Invalid(last.map(|e| e.to_string()).unwrap_or_else(|| "no key matched".into())))
    }

    async fn endpoints(&self) -> Result<Endpoints, VerifyError> {
        let loaded = |v: &Self| {
            let keys = v.keys.read().unwrap_or_else(|p| p.into_inner());
            keys.fetched_at.map(|_| keys.endpoints.clone())
        };
        if let Some(e) = loaded(self) {
            return Ok(e);
        }
        self.fetch_keys()
            .await
            .map_err(|e| VerifyError::Unavailable(format!("cannot reach the identity provider: {e}")))?;
        loaded(self).ok_or_else(|| VerifyError::Unavailable("no provider loaded".into()))
    }

    /// Where to send a browser to sign in (the web app): the authorization
    /// code flow with PKCE, as a public client, plus a nonce.
    pub async fn authorize_url(
        &self,
        redirect_uri: &str,
        scopes: &str,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, VerifyError> {
        let endpoint = self
            .endpoints()
            .await?
            .authorization
            .ok_or_else(|| VerifyError::Unavailable("the provider has no authorization_endpoint".into()))?;
        let mut url = url::Url::parse(&endpoint)
            .map_err(|e| VerifyError::Unavailable(format!("bad authorization_endpoint: {e}")))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.audience)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", scopes)
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", &pkce_challenge(pkce_verifier))
            .append_pair("code_challenge_method", "S256");
        Ok(url.into())
    }

    /// Trades a web sign-in's code for who signed in, from the ID token. The
    /// tokens themselves are dropped: the browser gets a session instead.
    pub async fn exchange(
        &self,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<Principal, VerifyError> {
        let endpoint = self
            .endpoints()
            .await?
            .token
            .ok_or_else(|| VerifyError::Unavailable("the provider has no token_endpoint".into()))?;
        let res = self
            .http
            .post(&endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", self.audience.as_str()),
                ("code_verifier", pkce_verifier),
            ])
            .send()
            .await
            .map_err(|e| VerifyError::Unavailable(format!("token endpoint: {e}")))?;
        let status = res.status();
        if status.is_server_error() {
            return Err(VerifyError::Unavailable(format!("token endpoint: HTTP {status}")));
        }
        if !status.is_success() {
            return Err(VerifyError::Invalid(format!("the token endpoint refused the code: HTTP {status}")));
        }
        let tokens: TokenResponse =
            res.json().await.map_err(|e| VerifyError::Unavailable(format!("token endpoint: {e}")))?;
        let id_token = tokens.id_token.ok_or_else(|| VerifyError::Invalid("no id_token in the response".into()))?;
        let claims: IdClaims = self.decode(&id_token).await?;
        if claims.nonce.as_deref() != Some(nonce) {
            return Err(VerifyError::Invalid("the token's nonce doesn't match this sign-in".into()));
        }
        self.principal(claims.claims)
    }

    /// The provider's sign-out page, if it has one.
    pub async fn end_session_url(&self) -> Option<String> {
        self.endpoints().await.ok()?.end_session
    }

    /// Users are keyed by subject; the username is the provider's own
    /// (never a display name, which users may be able to edit).
    fn principal(&self, c: Claims) -> Result<Principal, VerifyError> {
        let username = c.preferred_username.filter(|n| !n.is_empty()).ok_or_else(|| {
            VerifyError::Invalid("the token has no preferred_username (request the profile scope)".into())
        })?;
        Ok(Principal {
            display_name: c.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| username.clone()),
            username,
            subject: c.sub,
            groups: c.groups,
            expires: c.exp,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::{extract::Form, response::IntoResponse, routing::get, Json, Router};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    /// Throwaway RSA key made for these tests; it protects nothing.
    const KEY_DER: &[u8] = include_bytes!("testdata/rsa-test-key.der");
    const KEY_N: &str = include_str!("testdata/rsa-test-key.n");

    pub struct Provider {
        pub issuer: String,
        pub jwks_hits: Arc<AtomicUsize>,
        /// What the token endpoint answers with next.
        pub next_id_token: Arc<std::sync::Mutex<Option<String>>>,
        /// The last form posted to the token endpoint.
        pub token_form: Arc<std::sync::Mutex<Option<HashMap<String, String>>>>,
    }

    /// A minimal OIDC provider on a random port: discovery and JWKS.
    pub async fn provider(kid: &'static str) -> Provider {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let issuer = format!("{base}/application/o/lodestone/");
        let jwks_hits = Arc::new(AtomicUsize::new(0));

        let discovery = json!({
            "issuer": issuer,
            "jwks_uri": format!("{base}/jwks"),
            "authorization_endpoint": format!("{base}/application/o/authorize/"),
            "token_endpoint": format!("{base}/application/o/token/"),
            "end_session_endpoint": format!("{base}/application/o/lodestone/end-session/"),
        });
        let hits = jwks_hits.clone();
        let next_id_token = Arc::new(std::sync::Mutex::new(None::<String>));
        let token_form = Arc::new(std::sync::Mutex::new(None));
        let (next, form) = (next_id_token.clone(), token_form.clone());
        let app = Router::new()
            .route(
                "/application/o/lodestone/.well-known/openid-configuration",
                get(move || {
                    let d = discovery.clone();
                    async move { Json(d) }
                })
            )
            .route(
                "/jwks",
                get(move || {
                    hits.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Json(
                            json!({
                            "keys": [{ "kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256", "n": KEY_N.trim(), "e": "AQAB" }]
                        })
                        )
                    }
                })
            )
            .route(
                "/application/o/token/",
                axum::routing::post(move |Form(f): Form<HashMap<String, String>>| {
                    *form.lock().unwrap() = Some(f);
                    let id = next.lock().unwrap().take();
                    async move {
                        match id {
                            Some(t) => Json(json!({ "access_token": "at", "id_token": t }))
                                .into_response(),
                            None => axum::http::StatusCode::BAD_REQUEST.into_response(),
                        }
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Provider { issuer, jwks_hits, next_id_token, token_form }
    }

    pub fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
    }

    pub fn token(kid: &str, claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.into());
        encode(&header, &claims, &EncodingKey::from_rsa_der(KEY_DER)).unwrap()
    }

    pub fn claims(issuer: &str, groups: &[&str]) -> serde_json::Value {
        json!({
            "iss": issuer,
            "aud": "lodestone",
            "sub": "abc123",
            "preferred_username": "pwb",
            "groups": groups,
            "exp": now() + 600,
            "iat": now(),
        })
    }

    /// An ID token's claims for a web sign-in with this nonce.
    pub fn id_claims(issuer: &str, nonce: &str, groups: &[&str]) -> serde_json::Value {
        let mut c = claims(issuer, groups);
        c["nonce"] = json!(nonce);
        c
    }

    #[tokio::test]
    async fn a_web_sign_in_exchanges_its_code_as_a_public_client() {
        let p = provider("k1").await;
        let v = OidcVerifier::new(&cfg(&p.issuer));
        let redirect = "https://s.example/auth/callback";
        let url = v.authorize_url(redirect, "openid profile", "st", "n1", "verifier").await.unwrap();
        let url = url::Url::parse(&url).unwrap();
        let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "lodestone");
        assert_eq!(q["code_challenge"], pkce_challenge("verifier"));
        assert_eq!(q["nonce"], "n1");

        *p.next_id_token.lock().unwrap() = Some(token("k1", id_claims(&p.issuer, "n1", &[])));
        let who = v.exchange(redirect, "code", "verifier", "n1").await.unwrap();
        assert_eq!(who.username, "pwb");
        let form = p.token_form.lock().unwrap().clone().unwrap();
        assert_eq!(form["code_verifier"], "verifier");
        assert!(!form.contains_key("client_secret"));

        // Another sign-in's token: refused.
        *p.next_id_token.lock().unwrap() = Some(token("k1", id_claims(&p.issuer, "other", &[])));
        assert!(matches!(v.exchange(redirect, "code", "verifier", "n1").await, Err(VerifyError::Invalid(_))));
        // A refused code: invalid, not unavailable.
        assert!(matches!(v.exchange(redirect, "code", "verifier", "n1").await, Err(VerifyError::Invalid(_))));
    }

    fn cfg(issuer: &str) -> OidcConfig {
        OidcConfig {
            issuer: issuer.into(),
            client_id: "lodestone".into(),
            scopes: "openid".into(),
            allowed_groups: vec![],
            admin_groups: vec![],
            discovery_url: None,
        }
    }

    #[tokio::test]
    async fn accepts_a_valid_token() {
        let p = provider("k1").await;
        let v = OidcVerifier::new(&cfg(&p.issuer));
        let who = v.verify(&token("k1", claims(&p.issuer, &["homelab-users"]))).await.unwrap();
        assert_eq!(who.username, "pwb");
        assert_eq!(who.subject, "abc123");
        assert_eq!(who.groups, ["homelab-users"]);
        assert!(who.expires > now());
    }

    #[tokio::test]
    async fn rejects_expired_foreign_and_misaddressed_tokens() {
        let p = provider("k1").await;
        let v = OidcVerifier::new(&cfg(&p.issuer));
        let invalid = |r: Result<Principal, VerifyError>| matches!(r, Err(VerifyError::Invalid(_)));

        let mut c = claims(&p.issuer, &[]);
        c["exp"] = json!(now() - 3600);
        assert!(invalid(v.verify(&token("k1", c)).await), "expired");

        let mut c = claims(&p.issuer, &[]);
        c["aud"] = json!("gitea");
        assert!(invalid(v.verify(&token("k1", c)).await), "another app's token");

        let c = claims("https://evil.example/application/o/lodestone/", &[]);
        assert!(invalid(v.verify(&token("k1", c)).await), "another issuer");

        assert!(invalid(v.verify("not.a.jwt").await), "garbage");

        // A display name must never stand in for the username.
        let mut c = claims(&p.issuer, &[]);
        c.as_object_mut().unwrap().remove("preferred_username");
        c["name"] = json!("pwb");
        assert!(invalid(v.verify(&token("k1", c)).await), "no username");

        // Same claims, signed with a shared secret: must never pass.
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("k1".into());
        let hs =
            encode(&header, &claims(&p.issuer, &["homelab-admins"]), &EncodingKey::from_secret(b"lodestone")).unwrap();
        assert!(invalid(v.verify(&hs).await), "HS256");
    }

    #[tokio::test]
    async fn an_unknown_kid_refetches_once_then_backs_off() {
        let p = provider("k1").await;
        let v = OidcVerifier::new(&cfg(&p.issuer));
        v.verify(&token("k1", claims(&p.issuer, &[]))).await.unwrap();
        assert_eq!(p.jwks_hits.load(Ordering::SeqCst), 1);

        // The fetch just happened, so a new kid within a minute doesn't
        // trigger another: junk tokens can't make us hammer the provider.
        assert!(matches!(v.verify(&token("k2", claims(&p.issuer, &[]))).await, Err(VerifyError::Invalid(_))));
        assert_eq!(p.jwks_hits.load(Ordering::SeqCst), 1);

        // After the window, an unknown kid does refetch.
        v.keys.write().unwrap().last_attempt = Some(Instant::now() - REFETCH_AFTER);
        let _ = v.verify(&token("k2", claims(&p.issuer, &[]))).await;
        assert_eq!(p.jwks_hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_burst_of_unknown_kids_costs_one_fetch() {
        let p = provider("k1").await;
        let v = Arc::new(OidcVerifier::new(&cfg(&p.issuer)));
        v.verify(&token("k1", claims(&p.issuer, &[]))).await.unwrap();
        v.keys.write().unwrap().last_attempt = Some(Instant::now() - REFETCH_AFTER);

        // Every request sees a refetch is due and queues behind a fetch
        // already under way.
        let busy = v.fetching.lock().await;
        let burst: Vec<_> = (0..8)
            .map(|_| {
                let v = v.clone();
                let t = token("k2", claims(&p.issuer, &[]));
                tokio::spawn(async move { v.verify(&t).await })
            })
            .collect();
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        drop(busy);
        for b in burst {
            assert!(matches!(b.await.unwrap(), Err(VerifyError::Invalid(_))));
        }
        assert_eq!(p.jwks_hits.load(Ordering::SeqCst), 2, "the first load, then one refetch for all eight");
    }

    #[tokio::test]
    async fn an_unreachable_provider_is_unavailable_not_unauthorized() {
        let v = OidcVerifier::new(&cfg("http://127.0.0.1:9/application/o/lodestone/"));
        let r = v.verify(&token("k1", claims("http://127.0.0.1:9/application/o/lodestone/", &[]))).await;
        assert!(matches!(r, Err(VerifyError::Unavailable(_))), "{r:?}");
    }

    #[tokio::test]
    async fn refuses_a_discovery_document_for_a_different_issuer() {
        let p = provider("k1").await;
        let mut c = cfg("https://auth.example/application/o/lodestone/");
        c.discovery_url = Some(format!("{}.well-known/openid-configuration", p.issuer));
        let v = OidcVerifier::new(&c);
        assert!(v.fetch_keys().await.unwrap_err().contains("issuer"));
    }
}
