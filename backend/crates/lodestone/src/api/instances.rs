use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::{BroadcastStream, IntervalStream};

use super::profiles::new_profile;
use super::worlds::new_world;
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::error::{AppError, AppResult};
use crate::model::{
    ActionRequest, BanEntry, CommandRequest, ConsoleLine, CreateInstanceRequest, Instance, InstancePorts,
    InstanceProfileRef, InstanceStatus, InstanceWorldRef, JobKind, Player, PlayerActionKind, PlayerActionRequest,
    PlayerCount, ResourceKind, ServerAction, ServerPlayers, ServerState, SwitchWorldRequest, UpdateInstanceRequest,
    World,
};
use crate::ports;
use crate::store::{Asset, InstanceRecord, PlayerRef, ProfileRecord};
use crate::supervisor;
use crate::util::{new_id, now_iso, secret};

/* ------------------------------------------------------------------------ */
/* DTO assembly                                                             */
/* ------------------------------------------------------------------------ */

fn build(app: &AppState, record: &InstanceRecord, profile: &ProfileRecord, world: &World, host: &str) -> Instance {
    let snapshot = app.instances.snapshot(&record.id, record.last_error.as_deref());
    let max = profile.properties.get("max-players").and_then(|value| value.parse().ok()).unwrap_or(20);
    Instance {
        id: record.id.clone(),
        name: record.name.clone(),
        profile_id: record.profile_id.clone(),
        world_id: record.world_id.clone(),
        state: snapshot.state,
        players: PlayerCount { online: snapshot.online, max },
        address: if record.ports.minecraft == 25565 {
            host.to_owned()
        } else {
            format!("{host}:{}", record.ports.minecraft)
        },
        ports: record.ports,
        environment: record.environment.clone(),
        created_at: record.created_at.clone(),
        last_error: if snapshot.state == ServerState::Error { snapshot.last_error } else { None },
        job_id: snapshot.job_id,
        profile: InstanceProfileRef {
            name: profile.profile.name.clone(),
            software: profile.profile.software.clone(),
            version: profile.profile.version.clone(),
        },
        world: InstanceWorldRef { name: world.name.clone(), folder: world.folder.clone() },
    }
}

async fn public_host(app: &AppState) -> AppResult<String> {
    let address = app.store.settings().await?.public_address;
    Ok(if address.is_empty() { "localhost".into() } else { address })
}

async fn load(app: &AppState, id: &str) -> AppResult<Instance> {
    let record = app.store.get_instance(id).await?;
    let profile = app.store.get_profile(&record.profile_id).await?;
    let world = app.store.get_world(&record.world_id).await?;
    Ok(build(app, &record, &profile, &world, &public_host(app).await?))
}

pub async fn list(State(app): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<Instance>>> {
    let mine = Some(user.id.as_str());
    let profiles: HashMap<_, _> =
        app.store.list_profiles(mine).await?.into_iter().map(|p| (p.profile.id.clone(), p)).collect();
    let worlds: HashMap<_, _> = app.store.list_worlds(mine).await?.into_iter().map(|w| (w.id.clone(), w)).collect();
    let host = public_host(&app).await?;
    let mut out = Vec::new();
    for record in app.store.list_instances(mine).await? {
        if let (Some(profile), Some(world)) = (profiles.get(&record.profile_id), worlds.get(&record.world_id)) {
            out.push(build(&app, &record, profile, world, &host));
        }
    }
    Ok(Json(out))
}

pub async fn get(State(app): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<Json<Instance>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    Ok(Json(load(&app, &id).await?))
}

/* ------------------------------------------------------------------------ */
/* create / update / delete                                                 */
/* ------------------------------------------------------------------------ */

async fn used_ports(app: &AppState, except: Option<&str>) -> AppResult<HashSet<u16>> {
    // Ports belong to the host, so every user's instances count.
    let mut used = HashSet::new();
    for record in app.store.list_instances(None).await? {
        if Some(record.id.as_str()) == except {
            continue;
        }
        used.insert(record.ports.minecraft);
        used.insert(record.ports.rcon);
        if let Some(query) = record.ports.query {
            used.insert(query);
        }
    }
    Ok(used)
}

fn claim(port: u16, used: &mut HashSet<u16>, what: &str) -> AppResult<u16> {
    if port < 1024 {
        return Err(AppError::bad_request(format!("The {what} port must be 1024 or higher.")));
    }
    if !used.insert(port) {
        return Err(AppError::conflict("port_in_use", format!("Port {port} is already used by another instance.")));
    }
    Ok(port)
}

/// A server is made of its owner's own world and profile, even for admins.
async fn own(app: &AppState, user: &CurrentUser, asset: Asset, id: &str) -> AppResult<()> {
    if app.store.owner_of(asset, id).await?.as_deref() != Some(user.id.as_str()) {
        let what = if asset == Asset::World { "world" } else { "configuration" };
        return Err(AppError::not_found(what, id));
    }
    Ok(())
}

/// Creates an instance and, in the same transaction, a new world and/or
/// profile. Optionally starts it.
pub async fn create(
    State(app): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateInstanceRequest>,
) -> AppResult<(StatusCode, Json<Instance>)> {
    let name = req.name.trim().to_owned();
    if name.is_empty() {
        return Err(AppError::bad_request("Give the instance a name."));
    }
    if req.start == Some(true) && !(user.eula_accepted || app.config.accept_eula) {
        return Err(AppError::conflict(
            "eula_required",
            "Accept the Minecraft EULA (https://aka.ms/MinecraftEULA) in Settings before starting a server.",
        ));
    }

    let (world, new_world_record) = match (req.world_id, req.new_world) {
        (Some(id), None) => {
            own(&app, &user, Asset::World, &id).await?;
            let world = app.store.get_world(&id).await?;
            if world.attached_instance_id.is_some() {
                return Err(AppError::conflict(
                    "world_attached",
                    format!("\"{}\" is already attached to another instance.", world.name),
                ));
            }
            (world, false)
        }
        (None, Some(spec)) => (new_world(spec)?, true),
        _ => return Err(AppError::bad_request("Pick an existing world or describe a new one (not both).")),
    };
    let (profile, new_profile_record) = match (req.profile_id, req.new_profile) {
        (Some(id), None) => {
            own(&app, &user, Asset::Profile, &id).await?;
            (app.store.get_profile(&id).await?, false)
        }
        (None, Some(spec)) => (new_profile(&app, spec).await?, true),
        _ => return Err(AppError::bad_request("Pick an existing configuration or describe a new one (not both).")),
    };
    supervisor::check_world_compatible(&world, &profile)?;

    let mut used = used_ports(&app, None).await?;
    let minecraft = match req.minecraft_port {
        Some(port) => claim(port, &mut used, "Minecraft")?,
        None => {
            let port = ports::allocate(25565, &used, Ipv4Addr::UNSPECIFIED)
                .ok_or_else(|| AppError::conflict("no_free_port", "No free Minecraft port found."))?;
            used.insert(port);
            port
        }
    };
    let rcon = match req.rcon_port {
        Some(port) => claim(port, &mut used, "RCON")?,
        None => {
            let port = ports::allocate(25575, &used, Ipv4Addr::LOCALHOST)
                .ok_or_else(|| AppError::conflict("no_free_port", "No free RCON port found."))?;
            used.insert(port);
            port
        }
    };
    let query = match req.query_port {
        Some(port) => Some(claim(port, &mut used, "query")?),
        None => None,
    };

    let record = InstanceRecord {
        id: new_id("instance"),
        name,
        profile_id: profile.profile.id.clone(),
        world_id: world.id.clone(),
        ports: InstancePorts { minecraft, rcon, query },
        environment: req.environment.unwrap_or_default(),
        rcon_password: secret(24),
        created_at: now_iso(),
        last_error: None,
    };
    app.store
        .create_instance_bundle(
            new_world_record.then_some(&world),
            new_profile_record.then_some(&profile),
            &record,
            &user.id,
        )
        .await?;
    for dir in [app.paths.world(&world.id), app.paths.profile(&profile.profile.id), app.paths.instance(&record.id)] {
        tokio::fs::create_dir_all(dir).await?;
    }
    app.events.changed(ResourceKind::Worlds, Some(world.id.clone()));
    app.events.changed(ResourceKind::Profiles, Some(profile.profile.id.clone()));
    app.events.changed(ResourceKind::Instances, Some(record.id.clone()));

    if req.start == Some(true) {
        supervisor::start(&app, &record.id).await?;
    }
    Ok((StatusCode::CREATED, Json(load(&app, &record.id).await?)))
}

pub async fn update(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(patch): Json<UpdateInstanceRequest>,
) -> AppResult<Json<Instance>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let mut record = app.store.get_instance(&id).await?;
    let ports_change = patch.minecraft_port.is_some() || patch.rcon_port.is_some() || patch.query_port.is_some();
    if ports_change && app.instances.slot(&id).state().is_active() {
        return Err(AppError::conflict("instance_running", "Stop the instance before changing its ports."));
    }
    if let Some(name) = patch.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::bad_request("Give the instance a name."));
        }
        record.name = name.to_owned();
    }
    let mut used = used_ports(&app, Some(&id)).await?;
    let minecraft = claim(patch.minecraft_port.unwrap_or(record.ports.minecraft), &mut used, "Minecraft")?;
    let rcon = claim(patch.rcon_port.unwrap_or(record.ports.rcon), &mut used, "RCON")?;
    let query = match patch.query_port.unwrap_or(record.ports.query) {
        Some(port) => Some(claim(port, &mut used, "query")?),
        None => None,
    };
    record.ports = InstancePorts { minecraft, rcon, query };
    if let Some(env) = patch.environment {
        record.environment = env;
    }
    app.store.update_instance(&record).await?;
    app.events.changed(ResourceKind::Instances, Some(id.clone()));
    Ok(Json(load(&app, &id).await?))
}

/// Deleting an instance removes only its container and run dir. The world
/// and the profile are kept.
pub async fn delete(State(app): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<StatusCode> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let record = app.store.get_instance(&id).await?;
    if app.instances.slot(&id).state().is_active() {
        supervisor::stop_and_wait(&app, &id).await?;
    }
    app.store.delete_instance(&id).await?;
    app.instances.forget(&id);
    let _ = tokio::fs::remove_dir_all(app.paths.instance(&id)).await;
    app.events.deleted(ResourceKind::Instances, &id);
    app.events.changed(ResourceKind::Worlds, Some(record.world_id));
    app.events.changed(ResourceKind::Profiles, Some(record.profile_id));
    Ok(StatusCode::NO_CONTENT)
}

/// Re-points an instance at another world, restarting it if it was running.
pub async fn switch_world(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<SwitchWorldRequest>,
) -> AppResult<Json<Instance>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let mut record = app.store.get_instance(&id).await?;
    if record.world_id == req.world_id {
        return Ok(Json(load(&app, &id).await?));
    }
    // The world must belong to whoever owns the server.
    let owner = app.store.owner_of(Asset::Instance, &id).await?;
    if app.store.owner_of(Asset::World, &req.world_id).await? != owner {
        return Err(AppError::not_found("world", &req.world_id));
    }
    let world = app.store.get_world(&req.world_id).await?;
    if world.attached_instance_id.is_some() {
        return Err(AppError::conflict(
            "world_attached",
            format!("\"{}\" is attached to another instance.", world.name),
        ));
    }
    let profile = app.store.get_profile(&record.profile_id).await?;
    supervisor::check_world_compatible(&world, &profile)?;

    let was_active = app.instances.slot(&id).state().is_active();
    let job = app.jobs.create(JobKind::SwitchWorld, &id, format!("Switching to \"{}\"", world.name)).await?;
    if was_active {
        job.progress(None, "Stopping the server").await;
        if let Err(err) = supervisor::stop_and_wait(&app, &id).await {
            job.fail(err.to_string()).await;
            return Err(err);
        }
    }
    let previous = record.world_id.clone();
    record.world_id = world.id.clone();
    if let Err(err) = app.store.update_instance(&record).await {
        job.fail(err.to_string()).await;
        return Err(err);
    }
    app.events.changed(ResourceKind::Worlds, Some(previous));
    app.events.changed(ResourceKind::Worlds, Some(world.id.clone()));
    app.events.changed(ResourceKind::Instances, Some(id.clone()));
    if was_active {
        job.progress(None, "Starting with the new world").await;
        if let Err(err) = supervisor::start(&app, &id).await {
            job.fail(err.to_string()).await;
            return Err(err);
        }
    }
    job.succeed(format!("Now running \"{}\"", world.name)).await;
    Ok(Json(load(&app, &id).await?))
}

/* ------------------------------------------------------------------------ */
/* lifecycle / console                                                      */
/* ------------------------------------------------------------------------ */

pub async fn action(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<ActionRequest>,
) -> AppResult<Json<Instance>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    app.store.get_instance(&id).await?;
    match req.action {
        ServerAction::Start => supervisor::start(&app, &id).await?,
        ServerAction::Stop => supervisor::stop(&app, &id).await?,
        ServerAction::Restart => supervisor::restart(&app, &id).await?,
    }
    Ok(Json(load(&app, &id).await?))
}

pub async fn status(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<InstanceStatus>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let record = app.store.get_instance(&id).await?;
    let world = app.store.get_world(&record.world_id).await?;
    Ok(Json(supervisor::status(&app, &id, world.size_bytes)))
}

#[derive(Deserialize)]
pub struct TailQuery {
    tail: Option<usize>,
}

pub async fn console(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(query): Query<TailQuery>,
) -> AppResult<Json<Vec<ConsoleLine>>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    app.store.get_instance(&id).await?;
    Ok(Json(app.instances.slot(&id).console_tail(query.tail.unwrap_or(500).min(2000))))
}

pub async fn send_command(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> AppResult<Json<Vec<ConsoleLine>>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    app.store.get_instance(&id).await?;
    Ok(Json(supervisor::command(&app, &id, &req.command).await?))
}

/// SSE: `console` events for every new line, `status` every 3 seconds.
pub async fn console_stream(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let record = app.store.get_instance(&id).await?;
    let slot = app.instances.slot(&id);
    let lines = BroadcastStream::new(slot.subscribe_console())
        .filter_map(|line| async move { Some(Ok(Event::default().event("console").json_data(line.ok()?).ok()?)) });
    let world_id = record.world_id.clone();
    let statuses = IntervalStream::new(tokio::time::interval(Duration::from_secs(3))).filter_map(move |_| {
        let (app, id, world_id) = (app.clone(), id.clone(), world_id.clone());
        async move {
            let size = app.store.get_world(&world_id).await.map(|world| world.size_bytes).unwrap_or(0);
            let status = supervisor::status(&app, &id, size);
            Some(Ok(Event::default().event("status").json_data(status).ok()?))
        }
    });
    Ok(Sse::new(futures::stream::select(lines, statuses))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/* ------------------------------------------------------------------------ */
/* players                                                                  */
/* ------------------------------------------------------------------------ */

pub async fn players(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<ServerPlayers>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let record = app.store.get_instance(&id).await?;
    Ok(Json(player_view(&app, &record).await?))
}

async fn player_view(app: &AppState, record: &InstanceRecord) -> AppResult<ServerPlayers> {
    let profile = app.store.get_profile(&record.profile_id).await?;
    let seen = app.store.world_players(&record.world_id).await?;
    let online: HashSet<String> = app.instances.online_players(&record.id).into_iter().collect();
    let is_in = |list: &[PlayerRef], name: &str| list.iter().any(|entry| entry.name.eq_ignore_ascii_case(name));

    let mut players: BTreeMap<String, Player> = BTreeMap::new();
    for row in seen {
        players.insert(
            row.name.to_lowercase(),
            Player {
                uuid: row.uuid.unwrap_or_default(),
                online: online.contains(&row.name),
                ping_ms: None,
                playtime_seconds: row.playtime_seconds,
                dimension: None,
                op: is_in(&profile.access.ops, &row.name),
                whitelisted: is_in(&profile.access.whitelist, &row.name),
                first_seen: row.first_seen,
                last_seen: row.last_seen,
                name: row.name,
            },
        );
    }
    // People on an access list who have never joined still show up.
    for entry in profile.access.ops.iter().chain(profile.access.whitelist.iter()) {
        players.entry(entry.name.to_lowercase()).or_insert_with(|| Player {
            uuid: entry.uuid.clone().unwrap_or_default(),
            name: entry.name.clone(),
            online: online.contains(&entry.name),
            ping_ms: None,
            playtime_seconds: 0,
            dimension: None,
            op: is_in(&profile.access.ops, &entry.name),
            whitelisted: is_in(&profile.access.whitelist, &entry.name),
            first_seen: String::new(),
            last_seen: String::new(),
        });
    }
    let flag = |key: &str| profile.properties.get(key).is_some_and(|value| value == "true");
    Ok(ServerPlayers {
        players: players.into_values().collect(),
        whitelist_enabled: flag("white-list"),
        whitelist_enforced: flag("enforce-whitelist"),
        bans: profile.access.bans.clone(),
    })
}

fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Updates the profile's access lists (the source of truth) and, when the
/// server is running, applies the change live over RCON.
pub async fn player_action(
    State(app): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<PlayerActionRequest>,
) -> AppResult<Json<ServerPlayers>> {
    user.check(&app.store, Asset::Instance, &id).await?;
    let record = app.store.get_instance(&id).await?;
    let name = req.name.trim().to_owned();
    if !valid_name(&name) {
        return Err(AppError::bad_request("Minecraft names are 1–16 letters, digits or underscores."));
    }
    let slot = app.instances.slot(&id);
    let running = slot.state() == ServerState::Running;
    let reason = req.reason.clone().filter(|reason| !reason.trim().is_empty());

    let mut profile = app.store.get_profile(&record.profile_id).await?;
    let access = &mut profile.access;
    let entry = || PlayerRef { name: name.clone(), uuid: None };
    let remove = |list: &mut Vec<PlayerRef>| list.retain(|entry| !entry.name.eq_ignore_ascii_case(&name));
    let command = match req.action {
        PlayerActionKind::Kick => {
            if !running {
                return Err(AppError::conflict("not_running", "Players can only be kicked while the server runs."));
            }
            format!("kick {name} {}", reason.clone().unwrap_or_default())
        }
        PlayerActionKind::Op => {
            if !access.ops.iter().any(|op| op.name.eq_ignore_ascii_case(&name)) {
                access.ops.push(entry());
            }
            format!("op {name}")
        }
        PlayerActionKind::Deop => {
            remove(&mut access.ops);
            format!("deop {name}")
        }
        PlayerActionKind::WhitelistAdd => {
            if !access.whitelist.iter().any(|op| op.name.eq_ignore_ascii_case(&name)) {
                access.whitelist.push(entry());
            }
            format!("whitelist add {name}")
        }
        PlayerActionKind::WhitelistRemove => {
            remove(&mut access.whitelist);
            format!("whitelist remove {name}")
        }
        PlayerActionKind::Ban => {
            access.bans.retain(|ban| !ban.name.eq_ignore_ascii_case(&name));
            access.bans.push(BanEntry {
                uuid: None,
                name: name.clone(),
                reason: reason.clone().unwrap_or_else(|| "Banned by an operator.".into()),
                banned_by: "Lodestone".into(),
                created_at: now_iso(),
                expires_at: None,
            });
            format!("ban {name} {}", reason.clone().unwrap_or_default())
        }
        PlayerActionKind::Pardon => {
            access.bans.retain(|ban| !ban.name.eq_ignore_ascii_case(&name));
            format!("pardon {name}")
        }
    };
    if req.action != PlayerActionKind::Kick {
        app.store.update_profile(&profile).await?;
    }
    if running {
        let label = format!("> {}", command.trim());
        let _ = supervisor::command(&app, &id, command.trim()).await.map_err(|err| {
            tracing::warn!(error = %err, command = %label, "player action not applied live");
        });
    }
    app.events.changed(ResourceKind::Instances, Some(id.clone()));
    Ok(Json(player_view(&app, &record).await?))
}
