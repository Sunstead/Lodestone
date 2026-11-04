//! Who's calling, and what they own.
//!
//! Sign-in is OpenID Connect through Authentik, the same as the other
//! Sunstead services (Helion, Solstice): browsers get a session cookie from
//! the server-side code flow in `web.rs`; scripts may send a provider access
//! token as `Authorization: Bearer`. `--dev-user` signs everyone in as one
//! admin, for local development.
//!
//! Every world, profile and instance has an owner. Lists show the caller's
//! own; anything else answers 404 unless the caller is an admin (a member
//! of an admin group), who can open anyone's and change Lodestone's settings.

pub mod oidc;
#[cfg(test)]
mod tests;
pub mod web;

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::app::AppState;
use crate::config::{AuthConfig, OidcConfig};
use crate::error::{AppError, AppResult};
use crate::model::{AuthInfo, AuthMode, UserInfo};
use crate::store::{Asset, Store, UserRow};
use oidc::{OidcVerifier, Principal, VerifyError};

pub const DEV_ISSUER: &str = "dev";

pub enum Auth {
    Oidc { verifier: Arc<OidcVerifier>, config: OidcConfig },
    Dev { username: String },
}

impl Auth {
    pub fn new(config: &AuthConfig) -> Self {
        match config {
            AuthConfig::Oidc(config) => {
                Auth::Oidc { verifier: Arc::new(OidcVerifier::new(config)), config: config.clone() }
            }
            AuthConfig::Dev { username } => Auth::Dev { username: username.clone() },
        }
    }

    /// Loads the provider's keys in the background.
    pub fn start(&self) {
        if let Auth::Oidc { verifier, .. } = self {
            verifier.spawn_refresh();
        }
    }

    pub fn info(&self) -> AuthInfo {
        match self {
            Auth::Oidc { verifier, config } => AuthInfo {
                mode: AuthMode::Oidc,
                issuer: Some(verifier.issuer.clone()),
                client_id: Some(config.client_id.clone()),
            },
            Auth::Dev { .. } => AuthInfo { mode: AuthMode::Dev, issuer: None, client_id: None },
        }
    }
}

/// The signed-in caller.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub admin: bool,
    pub eula_accepted: bool,
}

impl From<UserRow> for CurrentUser {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.username,
            display_name: row.display_name,
            admin: row.admin,
            eula_accepted: row.eula_accepted_at.is_some(),
        }
    }
}

impl CurrentUser {
    pub fn info(&self) -> UserInfo {
        UserInfo {
            id: self.id.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            admin: self.admin,
        }
    }

    pub fn require_admin(&self) -> AppResult<()> {
        if self.admin {
            Ok(())
        } else {
            Err(AppError::forbidden("Only Lodestone admins can do that."))
        }
    }

    /// 404 unless the caller owns the asset (or is an admin), so other
    /// people's ids can't even be confirmed to exist.
    pub async fn check(&self, store: &Store, asset: Asset, id: &str) -> AppResult<()> {
        let owner = store.owner_of(asset, id).await?;
        if self.admin || owner.as_deref() == Some(self.id.as_str()) {
            return Ok(());
        }
        let what = match asset {
            Asset::World => "world",
            Asset::Profile => "configuration",
            Asset::Instance => "server",
        };
        Err(AppError::not_found(what, id))
    }
}

/// Whether a principal may sign in, and whether they're an admin.
pub fn admission(config: &OidcConfig, who: &Principal) -> AppResult<bool> {
    if !config.allowed_groups.is_empty() && !who.groups.iter().any(|g| config.allowed_groups.contains(g)) {
        return Err(AppError::forbidden("Your account isn't in a group allowed to use Lodestone."));
    }
    Ok(who.groups.iter().any(|g| config.admin_groups.contains(g)))
}

fn bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(|token| token.trim().to_owned())
}

/// The dev user, made (or refreshed) on demand. Dev users are admins.
pub async fn dev_user(store: &Store, username: &str) -> AppResult<UserRow> {
    store.upsert_user(DEV_ISSUER, username, username, username, true).await
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        if let Some(user) = parts.extensions.get::<CurrentUser>() {
            return Ok(user.clone());
        }
        let user = authenticate(app, parts).await?;
        parts.extensions.insert(user.clone());
        Ok(user)
    }
}

async fn authenticate(app: &AppState, parts: &Parts) -> AppResult<CurrentUser> {
    if let Some(token) = bearer(parts) {
        let Auth::Oidc { verifier, config } = &*app.auth else {
            return Ok(dev_user(&app.store, dev_name(app)).await?.into());
        };
        let who = verifier.verify(&token).await.map_err(|err| match err {
            VerifyError::Invalid(message) => AppError::Unauthorized(format!("Sign in again ({message}).")),
            VerifyError::Unavailable(message) => AppError::Unavailable(message),
        })?;
        let admin = admission(config, &who)?;
        let user =
            app.store.upsert_user(&verifier.issuer, &who.subject, &who.username, &who.display_name, admin).await?;
        return Ok(user.into());
    }
    if let Some(user) = web::session_user(app, &parts.headers).await? {
        return Ok(user.into());
    }
    match &*app.auth {
        Auth::Dev { username } => Ok(dev_user(&app.store, username).await?.into()),
        Auth::Oidc { .. } => Err(AppError::Unauthorized("Sign in to Lodestone.".into())),
    }
}

fn dev_name(app: &AppState) -> &str {
    match &*app.auth {
        Auth::Dev { username } => username,
        Auth::Oidc { .. } => "dev",
    }
}
