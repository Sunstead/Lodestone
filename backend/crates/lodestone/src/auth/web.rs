//! Signing in from a browser, for the web app: `/auth/login`,
//! `/auth/callback` and `/auth/logout`, and the session cookie they set.
//! Ported from Solstice Sync (which took it from Atlas).
//!
//! The server runs the authorization code flow with PKCE and a nonce as a
//! public client. The provider's tokens never reach the browser: the server
//! checks the ID token (groups, the provider's username) and starts its own
//! session. The cookie holds a random token; the database only its hash.

use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use url::Url;

use super::oidc::VerifyError;
use super::{admission, Auth};
use crate::app::AppState;
use crate::error::AppError;
use crate::store::{OidcFlow, UserRow as User, SESSION_TTL_SECS};
use crate::util::secret;

fn random_token(len: usize) -> String {
    secret(len + len / 3)
}

/// The header every cookie-authenticated, state-changing request carries.
pub const CSRF_HEADER: &str = "x-lodestone-request";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
}

// ---------------------------------------------------------------- cookie

/// `__Host-` prefixed and `Secure` over HTTPS; plain over HTTP (local
/// development), where browsers won't keep a `Secure` cookie reliably.
#[derive(Debug, Clone)]
pub struct CookieSpec {
    pub name: &'static str,
    secure: bool,
}

impl CookieSpec {
    pub fn for_url(public_url: Option<&Url>) -> Self {
        match public_url {
            Some(u) if u.scheme() == "https" => Self { name: "__Host-lodestone_session", secure: true },
            _ => Self { name: "lodestone_session", secure: false },
        }
    }

    pub fn read(&self, headers: &HeaderMap) -> Option<String> {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(k, _)| *k == self.name)
            .map(|(_, v)| v.trim_matches('"').to_owned())
            .filter(|v| !v.is_empty())
    }

    /// `Lax`, so following a link into the app carries the session;
    /// state-changing requests are guarded separately ([`csrf`]).
    pub fn set(&self, token: &str, max_age: i64) -> HeaderValue {
        let secure = if self.secure { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
            self.name
        ))
        .expect("session tokens are URL-safe base64")
    }

    fn clear(&self) -> HeaderValue {
        self.set("", 0)
    }
}

/// The user behind the request's session cookie, if any.
pub async fn session_user(state: &AppState, headers: &HeaderMap) -> Result<Option<User>, AppError> {
    match state.cookie.read(headers) {
        Some(token) => state.store.session_user(&token).await,
        None => Ok(None),
    }
}

/// Cross-site request forgery guard. A request that relies on the session
/// cookie (it has one, and no `Authorization`) and changes something must
/// send `X-Lodestone-Request: 1`, which a plain form can't and a cross-site
/// `fetch` can't without a CORS preflight the server never answers, and an
/// `Origin`, if any, that is the server's own. Bearer requests (the apps)
/// aren't sent by browsers on their own, so they pass.
pub async fn csrf(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let by_cookie = !req.headers().contains_key(header::AUTHORIZATION) && state.cookie.read(req.headers()).is_some();
    if safe || !by_cookie {
        return next.run(req).await;
    }
    if req.headers().get(CSRF_HEADER).and_then(|v| v.to_str().ok()) != Some("1") {
        return AppError::forbidden("Missing X-Lodestone-Request header").into_response();
    }
    if !same_origin(&state, req.headers()) {
        return AppError::forbidden("Cross-origin request refused").into_response();
    }
    next.run(req).await
}

/// Whether the request's `Origin` (if it sent one) is the server's own.
/// Without a public URL (local development) any origin passes.
pub fn same_origin(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    match &state.config.public_url {
        Some(url) => origin == url.origin().ascii_serialization(),
        None => true,
    }
}

// ---------------------------------------------------------------- return_to

/// Where to go after sign-in. It comes from the URL, so anyone can craft it:
/// only a path on the server's own origin, or sign-in becomes an open
/// redirect.
pub fn safe_return_to(raw: Option<&str>) -> String {
    raw.and_then(check_return_to).unwrap_or_else(|| "/".to_owned())
}

fn check_return_to(raw: &str) -> Option<String> {
    // A path, and only a path: not `//host` (scheme-relative) or `/\host`.
    if !raw.starts_with('/') || raw.starts_with("//") || raw.starts_with("/\\") {
        return None;
    }
    if raw.chars().any(|c| c.is_control() || c == '\\') {
        return None;
    }
    let base = Url::parse("http://lodestone.invalid").expect("a valid URL");
    let joined = base.join(raw).ok()?;
    if joined.origin() != base.origin() {
        return None;
    }
    // Back into sign-in would loop.
    if joined.path() == "/auth" || joined.path().starts_with("/auth/") {
        return None;
    }
    let mut out = joined.path().to_owned();
    if let Some(q) = joined.query() {
        out.push('?');
        out.push_str(q);
    }
    Some(out)
}

// ---------------------------------------------------------------- pages

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A sign-in problem, shown before the app (or a session) exists.
pub fn message(status: StatusCode, title: &str, body: &str, link: Option<(&str, &str)>) -> Response {
    let link = link
        .map(|(href, text)| format!(r#"<p><a href="{}">{}</a></p>"#, escape(href), escape(text)))
        .unwrap_or_default();
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - Lodestone</title>
<style>
  :root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
  body {{ max-width: 32rem; margin: 20vh auto 0; padding: 0 1.5rem; line-height: 1.5; }}
  h1 {{ font-size: 1.25rem; }}
  a {{ color: inherit; }}
</style>
</head>
<body>
<h1>{title}</h1>
<p>{body}</p>
{link}
</body>
</html>
"#,
        title = escape(title),
        body = escape(body),
    );
    (status, [(header::CACHE_CONTROL, "no-store")], Html(html)).into_response()
}

// ---------------------------------------------------------------- routes

/// A redirect that's never cached: it depends on the session.
fn redirect(to: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, to), (header::CACHE_CONTROL, "no-store")]).into_response()
}

fn redirect_uri(state: &AppState) -> Option<String> {
    state.config.public_url.as_ref().and_then(|u| u.join("/auth/callback").ok()).map(String::from)
}

async fn start_session(state: &AppState, user: User, return_to: &str) -> Response {
    match state.store.create_session(&user.id).await {
        Ok(token) => {
            tracing::info!(user = %user.username, "signed in on the web");
            let mut res = redirect(return_to);
            res.headers_mut().insert(header::SET_COOKIE, state.cookie.set(&token, SESSION_TTL_SECS));
            res
        }
        Err(e) => {
            tracing::error!(error = %e, "can't start a session");
            message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Couldn't sign you in",
                "Something went wrong on the server. Try again.",
                Some(("/", "Try again")),
            )
        }
    }
}

fn unavailable() -> Response {
    message(
        StatusCode::SERVICE_UNAVAILABLE,
        "Can't reach sign-in",
        "The identity provider isn't answering right now. Try again in a moment.",
        Some(("/", "Try again")),
    )
}

#[derive(Deserialize)]
struct LoginQuery {
    return_to: Option<String>,
}

async fn login(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<LoginQuery>) -> Response {
    let return_to = safe_return_to(q.return_to.as_deref());
    match &*state.auth {
        // Everyone is the dev user already.
        Auth::Dev { .. } => redirect(&return_to),
        Auth::Oidc { verifier, config } => {
            if let Ok(Some(_)) = session_user(&state, &headers).await {
                return redirect(&return_to);
            }
            let Some(redirect_uri) = redirect_uri(&state) else {
                return message(
                    StatusCode::NOT_IMPLEMENTED,
                    "Web sign-in isn't set up",
                    "This Lodestone has no public address. Set LODESTONE_PUBLIC_URL to sign in from a browser.",
                    None,
                );
            };
            let flow = OidcFlow {
                state: random_token(32),
                pkce_verifier: random_token(32),
                nonce: random_token(32),
                return_to,
            };
            let url = verifier
                .authorize_url(&redirect_uri, &config.scopes, &flow.state, &flow.nonce, &flow.pkce_verifier)
                .await;
            match url {
                Ok(url) => match state.store.put_flow(&flow).await {
                    Ok(()) => redirect(&url),
                    Err(e) => {
                        tracing::error!(error = %e, "can't store the sign-in flow");
                        message(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Couldn't start sign-in",
                            "Something went wrong on the server. Try again.",
                            Some(("/", "Try again")),
                        )
                    }
                },
                Err(e) => {
                    tracing::warn!(error = ?e, "can't reach the identity provider");
                    unavailable()
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    let (Auth::Oidc { verifier, config }, Some(redirect_uri)) = (&*state.auth, redirect_uri(&state)) else {
        return redirect("/");
    };
    // Taken first, so a refused or broken sign-in can't be replayed either.
    let flow = match q.state.as_deref() {
        Some(s) => state.store.take_flow(s).await.unwrap_or_else(|e| {
            tracing::error!(error = %e, "can't read the sign-in flow");
            None
        }),
        None => None,
    };
    let Some(flow) = flow else {
        return message(
            StatusCode::BAD_REQUEST,
            "This sign-in link has expired",
            "Sign-in links work once, for ten minutes. Start again.",
            Some(("/auth/login", "Sign in")),
        );
    };
    let retry = format!(
        "/auth/login?return_to={}",
        url::form_urlencoded::byte_serialize(flow.return_to.as_bytes()).collect::<String>()
    );
    if let Some(error) = q.error {
        tracing::info!(%error, "the provider refused sign-in");
        let body = q.error_description.unwrap_or_else(|| "The identity provider refused the sign-in.".into());
        return message(StatusCode::FORBIDDEN, "Sign-in didn't complete", &body, Some((&retry, "Try again")));
    }
    let Some(code) = q.code else {
        return message(
            StatusCode::BAD_REQUEST,
            "Sign-in didn't complete",
            "The provider sent no code.",
            Some((&retry, "Try again")),
        );
    };

    let who = match verifier.exchange(&redirect_uri, &code, &flow.pkce_verifier, &flow.nonce).await {
        Ok(who) => who,
        Err(VerifyError::Unavailable(e)) => {
            tracing::warn!(error = %e, "can't finish sign-in");
            return unavailable();
        }
        Err(VerifyError::Invalid(e)) => {
            tracing::warn!(error = %e, "sign-in refused");
            return message(
                StatusCode::BAD_REQUEST,
                "Sign-in didn't complete",
                "The sign-in couldn't be verified. Start again.",
                Some((&retry, "Try again")),
            );
        }
    };
    let admin = match admission(config, &who) {
        Ok(admin) => admin,
        Err(_) => {
            tracing::info!(user = %who.username, "signed in but not in an allowed group");
            return message(
                StatusCode::FORBIDDEN,
                "No access to Lodestone",
                "Your account isn't in a group that can use Lodestone. Ask the server's admin.",
                None,
            );
        }
    };
    let user =
        match state.store.upsert_user(&verifier.issuer, &who.subject, &who.username, &who.display_name, admin).await {
            Ok(user) => user,
            Err(e) => return e.into_response(),
        };
    if user.admin {
        // Assets from before sign-in existed go to the first admin.
        if let Ok(claimed) = state.store.claim_orphans(&user.id).await {
            if claimed > 0 {
                tracing::info!(user = %user.username, claimed, "claimed assets made before sign-in");
            }
        }
    }
    start_session(&state, user, &flow.return_to).await
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(token) = state.cookie.read(&headers) {
        state.store.delete_session(&token).await?;
    }
    let redirect = match &*state.auth {
        Auth::Oidc { verifier, .. } => verifier.end_session_url().await.unwrap_or_else(|| "/".into()),
        Auth::Dev { .. } => "/".into(),
    };
    let mut res = Json(serde_json::json!({ "redirect": redirect })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, state.cookie.clear());
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_stays_on_the_server() {
        assert_eq!(safe_return_to(Some("/vault/Notes?path=a.md")), "/vault/Notes?path=a.md");
        assert_eq!(safe_return_to(None), "/");
        for bad in [
            "//evil.com",
            "/\\evil.com",
            "https://evil.com",
            "evil.com",
            "javascript:alert(1)",
            "/\r\nSet-Cookie:x",
            "/auth/login?return_to=/x",
            "",
        ] {
            assert_eq!(safe_return_to(Some(bad)), "/", "{bad:?}");
        }
    }

    #[test]
    fn cookies_are_host_only_and_secure_over_https() {
        let https = CookieSpec::for_url(Some(&Url::parse("https://s.example").unwrap()));
        assert_eq!(https.name, "__Host-lodestone_session");
        let v = https.set("tok", 60);
        let v = v.to_str().unwrap();
        assert!(v.starts_with("__Host-lodestone_session=tok; Path=/; HttpOnly; SameSite=Lax"));
        assert!(v.ends_with("; Secure"));
        assert!(https.clear().to_str().unwrap().contains("Max-Age=0"));

        let http = CookieSpec::for_url(None);
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, "theme=dark; lodestone_session=abc".parse().unwrap());
        assert_eq!(http.read(&h).as_deref(), Some("abc"));
        assert!(!http.set("x", 1).to_str().unwrap().contains("Secure"));
    }

    #[test]
    fn pages_escape_what_they_show() {
        assert_eq!(escape(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }
}
