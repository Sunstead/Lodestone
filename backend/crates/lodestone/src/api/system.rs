use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::model::{AppSettings, HealthResponse, Job, ResourceKind, SystemInfo, UpdateSettingsRequest};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok".into(), version: crate::VERSION.into() })
}

/// One SSE stream for every change: `lodestone` events carry a `LodestoneEvent`.
pub async fn events(State(app): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(app.events.subscribe()).filter_map(|event| async move {
        // A lagging client skips events; the UI refetches on the next one.
        let event = event.ok()?;
        Some(Ok(Event::default().event("lodestone").json_data(event).ok()?))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn list_jobs(State(app): State<AppState>) -> AppResult<Json<Vec<Job>>> {
    Ok(Json(app.store.recent_jobs(50).await?))
}

pub async fn get_job(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<Json<Job>> {
    Ok(Json(app.store.get_job(&id).await?))
}

pub async fn get_settings(State(app): State<AppState>) -> AppResult<Json<AppSettings>> {
    Ok(Json(app.store.settings().await?))
}

pub async fn patch_settings(
    State(app): State<AppState>,
    Json(patch): Json<UpdateSettingsRequest>,
) -> AppResult<Json<AppSettings>> {
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
    if let Some(accepted) = patch.eula_accepted {
        settings.eula_accepted = accepted;
    }
    if let Some(args) = patch.default_java_args {
        settings.default_java_args = args.trim().to_owned();
    }
    app.store.put_settings(&settings).await?;
    app.events.changed(ResourceKind::Settings, None);
    // Addresses are rendered on instances.
    app.events.changed(ResourceKind::Instances, None);
    Ok(Json(settings))
}

pub async fn system_info(State(app): State<AppState>) -> AppResult<Json<SystemInfo>> {
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
