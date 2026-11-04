//! Mods and plugins: `/profiles/{id}/content*` and modpack import.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::content::{install, mrpack, plan, SearchQuery, SearchSort, Target};
use crate::error::{AppError, AppResult};
use crate::model::{
    ApplyContentRequest, ContentIndex, ContentPlanRequest, ContentSearchResult, ContentSource, ContentVersion,
    InstallPlan, Job, UpdateContentRequest,
};
use crate::store::Asset;

async fn target(app: &AppState, user: &CurrentUser, id: &str) -> AppResult<Target> {
    user.check(&app.store, Asset::Profile, id).await?;
    Target::for_profile(&app.store.get_profile(id).await?.profile)
}

pub async fn index(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<ContentIndex>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    Ok(Json(install::index(&app, &id).await?))
}

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    provider: Option<ContentSource>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    sort: SearchSort,
}

pub async fn search(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(params): Query<SearchParams>,
) -> AppResult<Json<ContentSearchResult>> {
    let target = target(&app, &user, &id).await?;
    let provider = app.content.for_target(params.provider.unwrap_or(ContentSource::Modrinth), &target)?;
    let query = SearchQuery {
        query: params.q.trim().to_owned(),
        offset: params.offset,
        limit: params.limit.unwrap_or(20).clamp(1, 50),
        sort: params.sort,
    };
    let mut result = provider.search(&target, &query).await?;
    let installed = app.store.list_content(&id).await?;
    for hit in &mut result.hits {
        hit.installed = installed
            .iter()
            .any(|record| record.provider == hit.provider && record.project_id.as_deref() == Some(hit.id.as_str()));
    }
    Ok(Json(result))
}

pub async fn versions(
    State(app): State<AppState>,
    user: CurrentUser,
    Path((id, provider, project)): Path<(String, ContentSource, String)>,
) -> AppResult<Json<Vec<ContentVersion>>> {
    let target = target(&app, &user, &id).await?;
    let provider = app.content.for_target(provider, &target)?;
    let versions = provider.versions(&project, &target).await?;
    Ok(Json(versions.into_iter().map(|resolved| resolved.version).collect()))
}

pub async fn plan(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<ContentPlanRequest>,
) -> AppResult<Json<InstallPlan>> {
    let target = target(&app, &user, &id).await?;
    if req.items.is_empty() {
        return Err(AppError::bad_request("Pick something to install."));
    }
    let installed = app.store.list_content(&id).await?;
    Ok(Json(plan::plan(&app.content, &target, &installed, &req.items).await?))
}

pub async fn apply(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<ApplyContentRequest>,
) -> AppResult<Json<ContentIndex>> {
    target(&app, &user, &id).await?;
    // Finish even if the browser goes away mid-download.
    let task = tokio::spawn(async move { install::apply(&app, &id, &req.items).await });
    Ok(Json(task.await.map_err(anyhow::Error::from)??))
}

pub async fn patch(
    State(app): State<AppState>,
    user: CurrentUser,
    Path((id, entry)): Path<(String, String)>,
    Json(req): Json<UpdateContentRequest>,
) -> AppResult<Json<ContentIndex>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    match req.enabled {
        Some(enabled) => Ok(Json(install::set_enabled(&app, &id, &entry, enabled).await?)),
        None => Ok(Json(install::index(&app, &id).await?)),
    }
}

pub async fn remove(
    State(app): State<AppState>,
    user: CurrentUser,
    Path((id, entry)): Path<(String, String)>,
) -> AppResult<Json<ContentIndex>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    Ok(Json(install::remove(&app, &id, &entry).await?))
}

pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Path((id, entry)): Path<(String, String)>,
) -> AppResult<Json<ContentIndex>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    let task = tokio::spawn(async move { install::update_entry(&app, &id, &entry).await });
    Ok(Json(task.await.map_err(anyhow::Error::from)??))
}

pub async fn check_updates(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<ContentIndex>> {
    target(&app, &user, &id).await?;
    Ok(Json(install::check_updates(&app, &id).await?))
}

/// The first file field of a multipart body: (file name, bytes).
async fn file_field(mut multipart: Multipart) -> AppResult<(String, Vec<u8>)> {
    while let Some(field) = multipart.next_field().await.map_err(|err| AppError::bad_request(err.body_text()))? {
        if let Some(name) = field.file_name().map(str::to_owned) {
            let bytes = field.bytes().await.map_err(|err| AppError::bad_request(err.body_text()))?;
            return Ok((name, bytes.to_vec()));
        }
    }
    Err(AppError::bad_request("Attach a file."))
}

pub async fn upload(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    multipart: Multipart,
) -> AppResult<Json<ContentIndex>> {
    target(&app, &user, &id).await?;
    let (name, bytes) = file_field(multipart).await?;
    Ok(Json(install::upload(&app, &id, &name, bytes).await?))
}

/// The file for an entry that has to be downloaded by hand.
pub async fn provide_file(
    State(app): State<AppState>,
    user: CurrentUser,
    Path((id, entry)): Path<(String, String)>,
    multipart: Multipart,
) -> AppResult<Json<ContentIndex>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    let (name, bytes) = file_field(multipart).await?;
    Ok(Json(install::provide_file(&app, &id, &entry, &name, bytes).await?))
}

/// Imports a `.mrpack` as a new profile. The job's `targetId` is the profile.
pub async fn import_modpack(
    State(app): State<AppState>,
    user: CurrentUser,
    multipart: Multipart,
) -> AppResult<(StatusCode, Json<Job>)> {
    let (_, bytes) = file_field(multipart).await?;
    Ok((StatusCode::ACCEPTED, Json(mrpack::import(&app, &user.id, bytes).await?)))
}
