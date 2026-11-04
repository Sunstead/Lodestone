//! Sign-in and ownership through the real router, with a stand-in OIDC
//! provider issuing tokens for three people.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use super::oidc::tests::{claims, now, provider, token};
use crate::config::{AuthConfig, Config, OidcConfig};

struct Harness {
    router: axum::Router,
    state: crate::AppState,
    issuer: String,
    _dir: tempfile::TempDir,
}

fn bearer(issuer: &str, sub: &str, username: &str, groups: &[&str]) -> String {
    let mut c = claims(issuer, groups);
    c["sub"] = json!(sub);
    c["preferred_username"] = json!(username);
    c["name"] = json!(format!("{username} (display)"));
    c["exp"] = json!(now() + 600);
    format!("Bearer {}", token("k1", c))
}

async fn harness(allowed: &[&str]) -> Harness {
    let p = provider("k1").await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::fake(dir.path());
    config.accept_eula = false;
    config.auth = AuthConfig::Oidc(OidcConfig {
        issuer: p.issuer.clone(),
        client_id: "lodestone".into(),
        scopes: "openid profile".into(),
        allowed_groups: allowed.iter().map(|g| g.to_string()).collect(),
        admin_groups: vec!["lodestone-admins".into()],
        discovery_url: None,
    });
    let state = crate::build_state(config).await.unwrap();
    Harness { router: crate::router(state.clone()), state, issuer: p.issuer, _dir: dir }
}

impl Harness {
    fn alice(&self) -> String {
        bearer(&self.issuer, "a", "alice", &["players"])
    }
    fn bob(&self) -> String {
        bearer(&self.issuer, "b", "bob", &["players"])
    }
    fn admin(&self) -> String {
        bearer(&self.issuer, "c", "carol", &["players", "lodestone-admins"])
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        auth: &[(&str, String)],
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in auth {
            request = request.header(*name, value);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self.router.clone().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn as_user(&self, who: &str, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.call(method, &format!("/api/v1{path}"), &[("authorization", who.to_owned())], body).await
    }
}

fn world(name: &str) -> Value {
    json!({
        "name": name, "levelType": "minecraft:normal", "seed": null, "generateStructures": true,
        "difficulty": "normal", "gamemode": "survival", "hardcore": false, "maxWorldSize": 29999984, "allowNether": true
    })
}

#[tokio::test]
async fn requests_need_a_sign_in_but_health_does_not() {
    let h = harness(&[]).await;
    let (status, body) = h.call(Method::GET, "/api/v1/worlds", &[], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    let (status, body) = h.call(Method::GET, "/api/v1/health", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["auth"]["mode"], "oidc");
    assert_eq!(body["auth"]["clientId"], "lodestone");

    let (status, me) = h.as_user(&h.alice(), Method::GET, "/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["username"], "alice");
    assert_eq!(me["user"]["displayName"], "alice (display)");
    assert_eq!(me["user"]["admin"], false);
}

#[tokio::test]
async fn people_only_see_their_own_assets() {
    let h = harness(&[]).await;
    let (status, created) = h.as_user(&h.alice(), Method::POST, "/worlds", Some(world("Alice's"))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();

    let (_, bobs) = h.as_user(&h.bob(), Method::GET, "/worlds", None).await;
    assert_eq!(bobs, json!([]));
    let (status, _) = h.as_user(&h.bob(), Method::GET, &format!("/worlds/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "someone else's world doesn't exist for you");
    let (status, _) = h.as_user(&h.bob(), Method::DELETE, &format!("/worlds/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Nor can it go into your server.
    let profile = json!({
        "name": "p", "description": "", "softwareId": "paper", "version": "1.21.4",
        "memory": { "minMb": 1024, "maxMb": 2048, "initialMb": 1024 }, "cpuCores": null, "javaArgs": "",
        "autoStart": false, "restartOnCrash": false, "crashThreshold": 3
    });
    let (status, _) = h
        .as_user(
            &h.bob(),
            Method::POST,
            "/instances",
            Some(json!({ "name": "x", "worldId": id, "newProfile": profile })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Admins can open anyone's, but their lists are their own.
    let (status, _) = h.as_user(&h.admin(), Method::GET, &format!("/worlds/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, admins) = h.as_user(&h.admin(), Method::GET, "/worlds", None).await;
    assert_eq!(admins, json!([]));

    let (status, users) = h.as_user(&h.admin(), Method::GET, "/users", None).await;
    assert_eq!(status, StatusCode::OK);
    let alice = users.as_array().unwrap().iter().find(|u| u["user"]["username"] == "alice").unwrap();
    assert_eq!(alice["worlds"], 1);
    let (status, _) = h.as_user(&h.alice(), Method::GET, "/users", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn settings_are_for_admins_and_the_eula_is_personal() {
    let h = harness(&[]).await;
    let patch = Some(json!({ "publicAddress": "play.example.com" }));
    let (status, _) = h.as_user(&h.alice(), Method::PATCH, "/settings", patch.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = h.as_user(&h.admin(), Method::PATCH, "/settings", patch).await;
    assert_eq!(status, StatusCode::OK);

    let (_, me) = h.as_user(&h.alice(), Method::PATCH, "/me", Some(json!({ "eulaAccepted": true }))).await;
    assert_eq!(me["eulaAccepted"], true);
    let (_, me) = h.as_user(&h.bob(), Method::GET, "/me", None).await;
    assert_eq!(me["eulaAccepted"], false, "Alice accepting doesn't accept for Bob");
}

#[tokio::test]
async fn only_allowed_groups_get_in() {
    let h = harness(&["lodestone-users"]).await;
    let (status, body) = h.as_user(&h.alice(), Method::GET, "/me", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let member = bearer(&h.issuer, "d", "dana", &["lodestone-users"]);
    let (status, _) = h.as_user(&member, Method::GET, "/me", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn bad_tokens_are_refused() {
    let h = harness(&[]).await;
    let mut c = claims(&h.issuer, &[]);
    c["exp"] = json!(now() - 3600);
    let expired = format!("Bearer {}", token("k1", c));
    let (status, _) = h.as_user(&expired, Method::GET, "/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cookie_sessions_need_the_csrf_header() {
    let h = harness(&[]).await;
    let user = h.state.store.upsert_user(&h.issuer, "a", "alice", "Alice", false).await.unwrap();
    let session = h.state.store.create_session(&user.id).await.unwrap();
    let cookie = ("cookie", format!("lodestone_session={session}"));

    let (status, me) = h.call(Method::GET, "/api/v1/me", std::slice::from_ref(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["username"], "alice");

    let (status, body) = h.call(Method::POST, "/api/v1/worlds", std::slice::from_ref(&cookie), Some(world("W"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let headers = [cookie.clone(), ("x-lodestone-request", "1".to_owned())];
    let (status, _) = h.call(Method::POST, "/api/v1/worlds", &headers, Some(world("W"))).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = h.call(Method::POST, "/auth/logout", &headers, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = h.call(Method::GET, "/api/v1/me", &[cookie], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "signed out");
}

#[tokio::test]
async fn web_sign_in_needs_a_public_url() {
    let h = harness(&[]).await;
    let response =
        h.router.clone().oneshot(Request::builder().uri("/auth/login").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}
