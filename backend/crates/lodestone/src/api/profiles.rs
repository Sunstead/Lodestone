use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::model::{
    CreateProfileRequest, Profile, ProfileMemory, ProfilePorts, PropertyFile, ResourceKind, SavePropertiesRequest,
    SoftwareCatalog, UpdateProfileRequest,
};
use crate::properties;
use crate::software;
use crate::store::{AccessLists, Asset, ProfileRecord};
use crate::util::{new_id, now_iso};

fn validate_memory(memory: &ProfileMemory) -> AppResult<()> {
    if memory.max_mb < 512 {
        return Err(AppError::bad_request("Give the server at least 512 MB of memory."));
    }
    if memory.min_mb > memory.max_mb || memory.initial_mb > memory.max_mb {
        return Err(AppError::bad_request("Minimum and initial memory cannot exceed the maximum."));
    }
    Ok(())
}

fn validate_cpu(cpu: Option<f64>) -> AppResult<()> {
    if cpu.is_some_and(|cores| !(cores > 0.0 && cores <= 512.0)) {
        return Err(AppError::bad_request("CPU limit must be a positive number of cores."));
    }
    Ok(())
}

pub async fn new_profile(app: &AppState, req: CreateProfileRequest) -> AppResult<ProfileRecord> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request("Give the configuration a name."));
    }
    validate_memory(&req.memory)?;
    validate_cpu(req.cpu_cores)?;
    let catalog = app.software.catalog().await?;
    let java = software::java_major(&catalog, req.software_id, &req.version)?;
    let now = now_iso();
    Ok(ProfileRecord {
        profile: Profile {
            id: new_id("profile"),
            name: name.to_owned(),
            description: req.description.trim().to_owned(),
            software_id: req.software_id,
            software: req.software_id.label().into(),
            version: req.version,
            java_major: java,
            image: software::image_for(java),
            memory: req.memory,
            cpu_cores: req.cpu_cores,
            java_args: req.java_args.trim().to_owned(),
            auto_start: req.auto_start,
            restart_on_crash: req.restart_on_crash,
            crash_threshold: req.crash_threshold,
            ports: ProfilePorts { rcon: properties::CONTAINER_RCON_PORT, query: None },
            created_at: now.clone(),
            updated_at: now,
            instance_count: 0,
        },
        properties: Default::default(),
        access: AccessLists::default(),
    })
}

pub async fn list(State(app): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Profile>>> {
    Ok(Json(app.store.list_profiles(Some(&user.id)).await?.into_iter().map(|record| record.profile).collect()))
}

pub async fn get(State(app): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<Json<Profile>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    Ok(Json(app.store.get_profile(&id).await?.profile))
}

pub async fn software(State(app): State<AppState>) -> AppResult<Json<SoftwareCatalog>> {
    Ok(Json(app.software.catalog().await?))
}

pub async fn create(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateProfileRequest>,
) -> AppResult<(StatusCode, Json<Profile>)> {
    let record = new_profile(&app, req).await?;
    app.store.insert_profile(&record, &user.id).await?;
    tokio::fs::create_dir_all(app.paths.profile(&record.profile.id)).await?;
    app.events.changed(ResourceKind::Profiles, Some(record.profile.id.clone()));
    Ok((StatusCode::CREATED, Json(record.profile)))
}

pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(patch): Json<UpdateProfileRequest>,
) -> AppResult<Json<Profile>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    let mut record = app.store.get_profile(&id).await?;
    let p = &mut record.profile;
    if let Some(name) = patch.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::bad_request("Give the configuration a name."));
        }
        p.name = name.to_owned();
    }
    if let Some(description) = patch.description {
        p.description = description.trim().to_owned();
    }
    if patch.software_id.is_some() || patch.version.is_some() {
        let software_id = patch.software_id.unwrap_or(p.software_id);
        let version = patch.version.unwrap_or_else(|| p.version.clone());
        let catalog = app.software.catalog().await?;
        let java = software::java_major(&catalog, software_id, &version)?;
        p.software_id = software_id;
        p.software = software_id.label().into();
        p.version = version;
        p.java_major = java;
        p.image = software::image_for(java);
    }
    if let Some(memory) = patch.memory {
        validate_memory(&memory)?;
        p.memory = memory;
    }
    if let Some(cpu) = patch.cpu_cores {
        validate_cpu(cpu)?;
        p.cpu_cores = cpu;
    }
    if let Some(args) = patch.java_args {
        p.java_args = args.trim().to_owned();
    }
    if let Some(value) = patch.auto_start {
        p.auto_start = value;
    }
    if let Some(value) = patch.restart_on_crash {
        p.restart_on_crash = value;
    }
    if let Some(value) = patch.crash_threshold {
        p.crash_threshold = value;
    }
    p.updated_at = now_iso();
    app.store.update_profile(&record).await?;
    app.events.changed(ResourceKind::Profiles, Some(id));
    app.events.changed(ResourceKind::Instances, None);
    Ok(Json(record.profile))
}

pub async fn delete(State(app): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<StatusCode> {
    user.check(&app.store, Asset::Profile, &id).await?;
    app.store.get_profile(&id).await?;
    app.store.delete_profile(&id).await?;
    let _ = tokio::fs::remove_dir_all(app.paths.profile(&id)).await;
    app.events.deleted(ResourceKind::Profiles, &id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn duplicate(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Profile>)> {
    user.check(&app.store, Asset::Profile, &id).await?;
    let source = app.store.get_profile(&id).await?;
    let now = now_iso();
    let mut copy = source.clone();
    copy.profile.id = new_id("profile");
    copy.profile.name = format!("{} (copy)", source.profile.name);
    copy.profile.created_at = now.clone();
    copy.profile.updated_at = now;
    copy.profile.instance_count = 0;
    let (from, to) = (app.paths.profile(&id), app.paths.profile(&copy.profile.id));
    tokio::task::spawn_blocking(move || copy_tree(&from, &to)).await.map_err(anyhow::Error::from)??;
    app.store.insert_profile(&copy, &user.id).await?;
    // The copy gets its own lockfile rows (the files themselves are shared
    // through the cache).
    for mut entry in app.store.list_content(&id).await? {
        entry.id = new_id("content");
        app.store.put_content(&copy.profile.id, &entry).await?;
    }
    app.events.changed(ResourceKind::Profiles, Some(copy.profile.id.clone()));
    Ok((StatusCode::CREATED, Json(copy.profile)))
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    if !from.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn property_file(app: &AppState, record: &ProfileRecord) -> PropertyFile {
    PropertyFile {
        path: app.paths.profile(&record.profile.id).display().to_string(),
        updated_at: record.profile.updated_at.clone(),
        fields: properties::profile_fields(&record.properties),
    }
}

pub async fn properties(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<PropertyFile>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    let record = app.store.get_profile(&id).await?;
    Ok(Json(property_file(&app, &record)))
}

pub async fn save_properties(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<SavePropertiesRequest>,
) -> AppResult<Json<PropertyFile>> {
    user.check(&app.store, Asset::Profile, &id).await?;
    properties::validate_profile_changes(&req.changes)?;
    let mut record = app.store.get_profile(&id).await?;
    for (key, value) in req.changes {
        let default = properties::field(&key).map(|field| field.default_value.as_str());
        if default == Some(value.as_str()) {
            record.properties.remove(&key);
        } else {
            record.properties.insert(key, value);
        }
    }
    record.profile.updated_at = now_iso();
    app.store.update_profile(&record).await?;
    app.events.changed(ResourceKind::Profiles, Some(id));
    app.events.changed(ResourceKind::Instances, None);
    Ok(Json(property_file(&app, &record)))
}
