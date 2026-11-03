//! HTTP API under `/api/v1`. Handlers stay thin: validation and DTO
//! assembly here, lifecycle work in `supervisor`, archives in `backups`.

mod instances;
mod profiles;
mod system;
mod worlds;

use axum::http::Uri;
use axum::routing::{get, post};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::error::AppError;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(system::health))
        .route("/events", get(system::events))
        .route("/jobs", get(system::list_jobs))
        .route("/jobs/{id}", get(system::get_job))
        .route("/settings", get(system::get_settings).patch(system::patch_settings))
        .route("/system", get(system::system_info))
        .route("/backups", get(worlds::all_backups))
        // worlds
        .route("/worlds", get(worlds::list).post(worlds::create))
        .route("/worlds/{id}", get(worlds::get).patch(worlds::update).delete(worlds::delete))
        .route("/worlds/{id}/duplicate", post(worlds::duplicate))
        .route("/worlds/{id}/status", get(worlds::status))
        .route("/worlds/{id}/backups", get(worlds::backups).post(worlds::create_backup))
        .route("/worlds/{id}/backups/schedule", axum::routing::patch(worlds::patch_schedule))
        .route("/worlds/{id}/backups/{backup_id}", axum::routing::delete(worlds::delete_backup))
        .route("/worlds/{id}/backups/{backup_id}/restore", post(worlds::restore_backup))
        // profiles
        .route("/profiles", get(profiles::list).post(profiles::create))
        .route("/profiles/software", get(profiles::software))
        .route("/profiles/{id}", get(profiles::get).patch(profiles::update).delete(profiles::delete))
        .route("/profiles/{id}/duplicate", post(profiles::duplicate))
        .route("/profiles/{id}/properties", get(profiles::properties).put(profiles::save_properties))
        .route("/profiles/{id}/mods", get(profiles::mods).post(profiles::mods_not_yet))
        .route(
            "/profiles/{id}/mods/{mod_id}",
            axum::routing::patch(profiles::mods_not_yet).delete(profiles::mods_not_yet),
        )
        .route("/profiles/{id}/mods/{mod_id}/update", post(profiles::mods_not_yet))
        // instances
        .route("/instances", get(instances::list).post(instances::create))
        .route("/instances/{id}", get(instances::get).patch(instances::update).delete(instances::delete))
        .route("/instances/{id}/world", post(instances::switch_world))
        .route("/instances/{id}/actions", post(instances::action))
        .route("/instances/{id}/status", get(instances::status))
        .route("/instances/{id}/console", get(instances::console).post(instances::send_command))
        .route("/instances/{id}/console/stream", get(instances::console_stream))
        .route("/instances/{id}/players", get(instances::players).post(instances::player_action))
        .fallback(api_not_found)
        .with_state(state.clone());

    let mut app = Router::new().nest("/api/v1", api);
    if let Some(dir) = &state.config.static_dir {
        // The built frontend; unknown paths fall back to the SPA shell.
        let spa = ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")));
        app = app.fallback_service(spa);
    }
    app.layer(TraceLayer::new_for_http())
}

async fn api_not_found(uri: Uri) -> AppError {
    AppError::NotFound(format!("No API route for {}.", uri.path()))
}
