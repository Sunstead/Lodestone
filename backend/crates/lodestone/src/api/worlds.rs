use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::app::AppState;
use crate::backups;
use crate::error::{AppError, AppResult};
use crate::model::{
    Backup, BackupIndex, BackupKind, CreateWorldRequest, DataPacks, Dimensions, ResourceKind, ServerState,
    UpdateScheduleRequest, UpdateWorldRequest, World, WorldLayout, WorldStatus,
};
use crate::supervisor;
use crate::util::{dir_size, new_id, now_iso, slugify};

const MAX_WORLD_SIZE: u32 = 29_999_984;

pub fn new_world(req: CreateWorldRequest) -> AppResult<World> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request("Give the world a name."));
    }
    if req.max_world_size == 0 || req.max_world_size > MAX_WORLD_SIZE {
        return Err(AppError::bad_request(format!("World border radius must be between 1 and {MAX_WORLD_SIZE}.")));
    }
    let now = now_iso();
    Ok(World {
        id: new_id("world"),
        name: name.to_owned(),
        folder: slugify(name),
        level_type: req.level_type,
        seed: req.seed.map(|seed| seed.trim().to_owned()).filter(|seed| !seed.is_empty()),
        generate_structures: req.generate_structures,
        generated: false,
        layout: WorldLayout::Vanilla,
        data_version: None,
        max_world_size: req.max_world_size,
        allow_nether: req.allow_nether,
        difficulty: req.difficulty,
        gamemode: req.gamemode,
        hardcore: req.hardcore,
        spawn_protection: 16,
        spawn_monsters: true,
        spawn_npcs: true,
        pvp: true,
        allow_flight: false,
        enable_command_block: false,
        data_packs: DataPacks { enabled: vec!["vanilla".into()], disabled: vec![] },
        size_bytes: 0,
        created_at: now.clone(),
        updated_at: now,
        attached_instance_id: None,
    })
}

pub async fn list(State(app): State<AppState>) -> AppResult<Json<Vec<World>>> {
    Ok(Json(app.store.list_worlds().await?))
}

pub async fn get(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<Json<World>> {
    Ok(Json(app.store.get_world(&id).await?))
}

pub async fn create(
    State(app): State<AppState>,
    Json(req): Json<CreateWorldRequest>,
) -> AppResult<(StatusCode, Json<World>)> {
    let world = new_world(req)?;
    app.store.insert_world(&world).await?;
    tokio::fs::create_dir_all(app.paths.world(&world.id)).await?;
    app.events.changed(ResourceKind::Worlds, Some(world.id.clone()));
    Ok((StatusCode::CREATED, Json(world)))
}

pub async fn update(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateWorldRequest>,
) -> AppResult<Json<World>> {
    let mut world = app.store.get_world(&id).await?;
    let touches_generation = patch.level_type.is_some_and(|value| value != world.level_type)
        || patch.generate_structures.is_some_and(|value| value != world.generate_structures)
        || patch.seed.as_ref().is_some_and(|seed| Some(seed.trim()).filter(|s| !s.is_empty()) != world.seed.as_deref());
    if world.generated && touches_generation {
        return Err(AppError::conflict(
            "world_generated",
            "Seed, level type and structures are fixed once a world has generated. Create a new world to change them.",
        ));
    }
    if let Some(name) = patch.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::bad_request("Give the world a name."));
        }
        world.name = name.to_owned();
    }
    if let Some(value) = patch.level_type {
        world.level_type = value;
    }
    if let Some(seed) = patch.seed {
        world.seed = Some(seed.trim().to_owned()).filter(|seed| !seed.is_empty());
    }
    if let Some(value) = patch.max_world_size {
        if value == 0 || value > MAX_WORLD_SIZE {
            return Err(AppError::bad_request(format!("World border radius must be between 1 and {MAX_WORLD_SIZE}.")));
        }
        world.max_world_size = value;
    }
    macro_rules! apply {
        ($($field:ident),*) => { $( if let Some(value) = patch.$field { world.$field = value; } )* };
    }
    apply!(
        generate_structures,
        difficulty,
        gamemode,
        hardcore,
        allow_nether,
        spawn_protection,
        spawn_monsters,
        spawn_npcs,
        pvp,
        allow_flight,
        enable_command_block,
        data_packs
    );
    world.updated_at = now_iso();
    app.store.update_world(&world).await?;
    app.events.changed(ResourceKind::Worlds, Some(world.id.clone()));
    app.events.changed(ResourceKind::Instances, None);
    Ok(Json(world))
}

pub async fn delete(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<StatusCode> {
    app.store.get_world(&id).await?;
    app.store.delete_world(&id).await?;
    let _ = tokio::fs::remove_dir_all(app.paths.world(&id)).await;
    let _ = tokio::fs::remove_dir_all(app.paths.backups(&id)).await;
    app.events.deleted(ResourceKind::Worlds, &id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn duplicate(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<(StatusCode, Json<World>)> {
    let source = app.store.get_world(&id).await?;
    if let Some(instance) = &source.attached_instance_id {
        let slot = app.instances.slot(instance);
        if slot.state() == ServerState::Running {
            // Flush chunks so the copy is consistent.
            let _ = supervisor::rcon(&slot, "save-all flush").await;
        }
    }
    let now = now_iso();
    let copy = World {
        id: new_id("world"),
        name: format!("{} (copy)", source.name),
        attached_instance_id: None,
        created_at: now.clone(),
        updated_at: now,
        ..source.clone()
    };
    let (from, to) = (app.paths.world(&source.id), app.paths.world(&copy.id));
    tokio::task::spawn_blocking(move || copy_dir(&from, &to)).await.map_err(anyhow::Error::from)??;
    app.store.insert_world(&copy).await?;
    app.events.changed(ResourceKind::Worlds, Some(copy.id.clone()));
    Ok((StatusCode::CREATED, Json(copy)))
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    if !from.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if entry.file_name() != "session.lock" {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub async fn status(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<Json<WorldStatus>> {
    let world = app.store.get_world(&id).await?;
    let universe = app.paths.world(&id);
    let level = universe.join(&world.folder);
    let (disk_bytes, dimensions) = {
        let world = world.clone();
        tokio::task::spawn_blocking(move || {
            let dims = if world.generated {
                match world.layout {
                    WorldLayout::Vanilla => {
                        Dimensions { nether: level.join("DIM-1").exists(), end: level.join("DIM1").exists() }
                    }
                    WorldLayout::Bukkit => Dimensions {
                        nether: universe.join(format!("{}_nether", world.folder)).exists(),
                        end: universe.join(format!("{}_the_end", world.folder)).exists(),
                    },
                }
            } else {
                Dimensions { nether: world.allow_nether, end: true }
            };
            (dir_size(&universe), dims)
        })
        .await
        .map_err(anyhow::Error::from)?
    };
    let time = match &world.attached_instance_id {
        Some(instance) => supervisor::world_time(&app, instance).await,
        None => None,
    };
    let players = app.store.world_players(&id).await?;
    Ok(Json(WorldStatus {
        world_id: id,
        attached: world.attached_instance_id.is_some(),
        disk_bytes,
        day: time.as_ref().map(|(day, _)| *day),
        time_of_day: time.map(|(_, label)| label),
        dimensions,
        player_count: players.len() as u32,
        last_played_at: players.iter().map(|player| player.last_seen.clone()).max(),
    }))
}

/* -------------------------------- backups ------------------------------- */

pub async fn all_backups(State(app): State<AppState>) -> AppResult<Json<Vec<Backup>>> {
    Ok(Json(app.store.list_backups(None).await?))
}

pub async fn backups(State(app): State<AppState>, Path(id): Path<String>) -> AppResult<Json<BackupIndex>> {
    Ok(Json(backups::index(&app, &id).await?))
}

pub async fn create_backup(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Backup>)> {
    Ok((StatusCode::ACCEPTED, Json(backups::create(&app, &id, BackupKind::Manual).await?)))
}

pub async fn delete_backup(
    State(app): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    backups::delete(&app, &id, &backup_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restore_backup(
    State(app): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    backups::restore(&app, &id, &backup_id).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn patch_schedule(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UpdateScheduleRequest>,
) -> AppResult<Json<BackupIndex>> {
    Ok(Json(backups::patch_schedule(&app, &id, patch).await?))
}
