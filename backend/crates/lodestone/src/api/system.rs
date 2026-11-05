use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::model::{
    AppSettings, HealthResponse, Job, LodestoneEvent, Me, ResourceKind, SystemInfo, UpdateMeRequest,
    UpdateSettingsRequest, UserSummary,
};

pub async fn health(State(app): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok".into(), version: crate::VERSION.into(), auth: app.auth.info() })
}

/// Whether `user` may see something about `id`: theirs, an admin, or
/// (for ids nothing owns any more) harmless.
async fn visible(app: &AppState, user: &CurrentUser, id: &str) -> bool {
    if user.admin {
        return true;
    }
    match app.store.owner_of_any(id).await {
        Ok(Some(owner)) => owner.as_deref() == Some(user.id.as_str()),
        Ok(None) => false,
        Err(_) => false,
    }
}

/// One SSE stream for every change: `lodestone` events carry a
/// `LodestoneEvent`, filtered to what the caller owns. Collection-wide
/// hints and deletions (just an id) go to everyone.
pub async fn events(
    State(app): State<AppState>,
    user: CurrentUser,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(app.events.subscribe()).filter_map(move |event| {
        let (app, user) = (app.clone(), user.clone());
        async move {
            // A lagging client skips events; the UI refetches on the next one.
            let event = event.ok()?;
            let subject = match &event {
                LodestoneEvent::Changed { id, .. } => id.clone(),
                LodestoneEvent::Deleted { .. } => None,
                LodestoneEvent::InstanceState { id, .. } => Some(id.clone()),
                LodestoneEvent::Job { job } => Some(job.target_id.clone()),
            };
            if let Some(id) = subject {
                if !visible(&app, &user, &id).await {
                    return None;
                }
            }
            Some(Ok(Event::default().event("lodestone").json_data(event).ok()?))
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn list_jobs(State(app): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Job>>> {
    let mut out = vec![];
    for job in app.store.recent_jobs(100).await? {
        if visible(&app, &user, &job.target_id).await {
            out.push(job);
        }
        if out.len() == 50 {
            break;
        }
    }
    Ok(Json(out))
}

pub async fn get_job(State(app): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<Json<Job>> {
    let job = app.store.get_job(&id).await?;
    if !visible(&app, &user, &job.target_id).await {
        return Err(AppError::not_found("job", &id));
    }
    Ok(Json(job))
}

pub async fn me(State(app): State<AppState>, user: CurrentUser) -> Json<Me> {
    Json(Me { eula_accepted: user.eula_accepted || app.config.accept_eula, user: user.info(), auth: app.auth.info() })
}

pub async fn patch_me(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(patch): Json<UpdateMeRequest>,
) -> AppResult<Json<Me>> {
    let mut user = user;
    if let Some(accepted) = patch.eula_accepted {
        app.store.set_eula(&user.id, accepted).await?;
        user.eula_accepted = accepted;
    }
    Ok(me(State(app), user).await)
}

pub async fn list_users(State(app): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<UserSummary>>> {
    user.require_admin()?;
    let users = app.store.list_users().await?;
    Ok(Json(
        users
            .into_iter()
            .map(|(row, [worlds, profiles, instances])| UserSummary {
                eula_accepted: row.eula_accepted_at.is_some(),
                last_seen_at: row.last_seen_at.clone(),
                user: crate::auth::CurrentUser::from(row).info(),
                worlds,
                profiles,
                instances,
            })
            .collect(),
    ))
}

async fn settings(app: &AppState) -> AppResult<AppSettings> {
    let mut settings = app.store.settings().await?;
    settings.curseforge_key_set = app.curseforge_key.get().is_some();
    Ok(settings)
}

pub async fn get_settings(State(app): State<AppState>, _user: CurrentUser) -> AppResult<Json<AppSettings>> {
    Ok(Json(settings(&app).await?))
}

pub async fn patch_settings(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(patch): Json<UpdateSettingsRequest>,
) -> AppResult<Json<AppSettings>> {
    user.require_admin()?;
    let mut settings = app.store.settings().await?;
    if let Some(address) = patch.public_address {
        let address = address.trim().trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/');
        if address.contains(char::is_whitespace) || address.contains('/') {
            return Err(AppError::bad_request(
                "The public address should be a host name or IP, e.g. play.example.com.",
            ));
        }
        settings.public_address = address.to_owned();
    }
    if let Some(args) = patch.default_java_args {
        settings.default_java_args = args.trim().to_owned();
    }
    if let Some(key) = patch.curseforge_api_key {
        let key = key.trim().to_owned();
        if key.chars().any(char::is_whitespace) {
            return Err(AppError::bad_request("That doesn't look like a CurseForge API key."));
        }
        app.store.put_value(crate::app::CURSEFORGE_KEY, &key).await?;
        app.curseforge_key.set(Some(key));
        // Which sources configurations can use just changed.
        app.events.changed(ResourceKind::Profiles, None);
    }
    app.store.put_settings(&settings).await?;
    app.events.changed(ResourceKind::Settings, None);
    // Addresses are rendered on instances, and every address name hangs off
    // the public address. (After saving: the sync reads the store.)
    app.events.changed(ResourceKind::Instances, None);
    app.dns.changed();
    Ok(Json(self::settings(&app).await?))
}

pub async fn system_info(State(app): State<AppState>, _user: CurrentUser) -> AppResult<Json<SystemInfo>> {
    let root = app.paths.root().to_path_buf();
    let data_dir_bytes = tokio::task::spawn_blocking(move || crate::util::dir_size(&root)).await.unwrap_or(0);
    Ok(Json(SystemInfo {
        version: crate::VERSION.into(),
        runtime: app.runtime.info().await,
        providers: app.software.kind().into(),
        data_dir: app.paths.root().display().to_string(),
        data_dir_bytes,
        host: app.host.latest(),
    }))
}
