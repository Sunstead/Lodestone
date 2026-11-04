//! HTTP API under `/api/v1`. Handlers stay thin: validation and DTO
//! assembly here, lifecycle work in `supervisor`, archives in `backups`.

mod content;
mod instances;
pub(crate) mod profiles;
mod system;
mod worlds;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::Uri;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::auth::{web, CurrentUser};
use crate::error::AppError;

/// Uploads (jars, modpacks) may be large.
const UPLOAD_LIMIT: usize = 1 << 30;

/// Everything under `/api/v1` except `/health` needs a signed-in user.
async fn require_user(State(app): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    let (mut parts, body) = req.into_parts();
    use axum::extract::FromRequestParts;
    CurrentUser::from_request_parts(&mut parts, &app).await?;
    Ok(next.run(Request::from_parts(parts, body)).await)
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/me", get(system::me).patch(system::patch_me))
        .route("/users", get(system::list_users))
        .route("/events", get(system::events))
        .route("/jobs", get(system::list_jobs))
        .route("/jobs/{id}", get(system::get_job))
        .route("/settings", get(system::get_settings).patch(system::patch_settings))
        .route("/system", get(system::system_info))
        .route("/backups", get(worlds::all_backups))
        // worlds
        .route("/worlds", get(worlds::list).post(worlds::create))
        .route("/worlds/{id}", get(worlds::get).patch(worlds::update).delete(worlds::delete))
        .route("/worlds/import", post(worlds::import).layer(DefaultBodyLimit::max(64 << 30)))
        .route("/worlds/{id}/download", get(worlds::download))
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
        .route("/profiles/import", post(content::import_modpack).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)))
        .route("/profiles/{id}/content", get(content::index).post(content::apply))
        .route("/profiles/{id}/content/search", get(content::search))
        .route("/profiles/{id}/content/plan", post(content::plan))
        .route("/profiles/{id}/content/check-updates", post(content::check_updates))
        .route("/profiles/{id}/content/upload", post(content::upload).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)))
        .route("/profiles/{id}/content/projects/{provider}/{project}/versions", get(content::versions))
        .route("/profiles/{id}/content/{entry}", patch(content::patch).delete(content::remove))
        .route("/profiles/{id}/content/{entry}/update", post(content::update))
        // instances
        .route("/instances", get(instances::list).post(instances::create))
        .route("/instances/{id}", get(instances::get).patch(instances::update).delete(instances::delete))
        .route("/instances/{id}/world", post(instances::switch_world))
        .route("/instances/{id}/actions", post(instances::action))
        .route("/instances/{id}/status", get(instances::status))
        .route("/instances/{id}/console", get(instances::console).post(instances::send_command))
        .route("/instances/{id}/console/stream", get(instances::console_stream))
        .route("/instances/{id}/players", get(instances::players).post(instances::player_action))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_user))
        .route("/health", get(system::health))
        .fallback(api_not_found)
        .with_state(state.clone());

    let mut app = Router::new()
        .nest("/api/v1", api)
        .merge(web::router().with_state(state.clone()))
        .layer(middleware::from_fn_with_state(state.clone(), web::csrf));
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
