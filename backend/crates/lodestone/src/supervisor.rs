//! Per-instance supervision: the lifecycle state machine, console buffer,
//! RCON connection, player tracking, crash policy and metrics sampling.
//!
//! ```text
//! stopped ─start─▶ preparing ─files ready─▶ starting ─"Done ("─▶ running
//!    ▲                 │                        │                  │
//!    │               error ◀────── crash (over threshold) ─────────┤
//!    └──────────── stopping ◀────────────── stop ─────────────────┘
//! ```
//!
//! The same code path runs for the fake runtime and real Docker
//! containers: both emit log lines and serve RCON.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use mc_rcon::Rcon;
use serde_json::json;
use tokio::sync::{broadcast, watch};

use crate::app::AppState;
use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;
use crate::logparse;
use crate::model::{
    ConsoleLine, ConsoleSource, ContainerInfo, DiskUsage, InstanceStatus, JobKind, LogLevel, MemoryUsage, NetworkUsage,
    PlayerCount, ResourceKind, ServerSoftwareId, ServerState, World, WorldLayout,
};
use crate::properties;
use crate::runtime::{LaunchSpec, Mount, ProcessEvent, ProcessStats};
use crate::software::{self, Launch};
use crate::store::{InstanceRecord, ProfileRecord};
use crate::util::{now_iso, now_ms, slugify};

const CONSOLE_CAPACITY: usize = 2000;
/// Crashes inside this window count towards a profile's crash threshold.
const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);

/// One instance's live state. Created lazily; survives restarts of the
/// server but not of Lodestone.
pub struct Slot {
    id: String,
    live: Mutex<Live>,
    console: Mutex<VecDeque<ConsoleLine>>,
    console_tx: broadcast::Sender<ConsoleLine>,
    state_tx: watch::Sender<ServerState>,
    /// Serialises start/stop so two clicks cannot race.
    op: tokio::sync::Mutex<()>,
    seq: AtomicU64,
}

#[derive(Default)]
struct Live {
    container_id: Option<String>,
    rcon_addr: Option<SocketAddr>,
    rcon_password: String,
    rcon: Option<Arc<tokio::sync::Mutex<Rcon>>>,
    started_at: Option<DateTime<Utc>>,
    online: HashMap<String, Instant>,
    last_error: Option<String>,
    job_id: Option<String>,
    stop_requested: bool,
    crashes: Vec<Instant>,
    tps: Option<f64>,
    tick_ms: Option<f64>,
    stats: Option<ProcessStats>,
    net_rate: (f64, f64),
    image: String,
    world_id: String,
    software: Option<ServerSoftwareId>,
    version: String,
    max_players: u32,
    /// Recent error-ish output, quoted when the server crashes.
    error_tail: VecDeque<String>,
}

/// What an Instance DTO needs from the live state.
pub struct Snapshot {
    pub state: ServerState,
    pub online: u32,
    pub last_error: Option<String>,
    pub job_id: Option<String>,
}

#[derive(Default)]
pub struct InstanceManager {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}

impl InstanceManager {
    pub fn slot(&self, id: &str) -> Arc<Slot> {
        self.slots
            .lock()
            .unwrap()
            .entry(id.to_owned())
            .or_insert_with(|| {
                let (console_tx, _) = broadcast::channel(512);
                let (state_tx, _) = watch::channel(ServerState::Stopped);
                Arc::new(Slot {
                    id: id.to_owned(),
                    live: Mutex::new(Live::default()),
                    console: Mutex::new(VecDeque::with_capacity(CONSOLE_CAPACITY)),
                    console_tx,
                    state_tx,
                    op: tokio::sync::Mutex::new(()),
                    seq: AtomicU64::new(0),
                })
            })
            .clone()
    }

    pub fn forget(&self, id: &str) {
        self.slots.lock().unwrap().remove(id);
    }

    pub fn snapshot(&self, id: &str, stored_error: Option<&str>) -> Snapshot {
        let slot = self.slot(id);
        let live = slot.live.lock().unwrap();
        Snapshot {
            state: slot.state(),
            online: live.online.len() as u32,
            last_error: live.last_error.clone().or_else(|| stored_error.map(str::to_owned)),
            job_id: live.job_id.clone(),
        }
    }

    pub fn online_players(&self, id: &str) -> Vec<String> {
        let slot = self.slot(id);
        let live = slot.live.lock().unwrap();
        let mut names: Vec<_> = live.online.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Slot {
    pub fn state(&self) -> ServerState {
        *self.state_tx.borrow()
    }

    pub fn subscribe_console(&self) -> broadcast::Receiver<ConsoleLine> {
        self.console_tx.subscribe()
    }

    pub fn subscribe_state(&self) -> watch::Receiver<ServerState> {
        self.state_tx.subscribe()
    }

    pub fn console_tail(&self, tail: usize) -> Vec<ConsoleLine> {
        let console = self.console.lock().unwrap();
        console.iter().skip(console.len().saturating_sub(tail)).cloned().collect()
    }

    fn push(&self, level: LogLevel, source: ConsoleSource, message: impl Into<String>) -> ConsoleLine {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let ts = now_ms();
        let line = ConsoleLine { id: format!("{ts}-{seq}"), ts, level, message: message.into(), source };
        {
            let mut console = self.console.lock().unwrap();
            if console.len() == CONSOLE_CAPACITY {
                console.pop_front();
            }
            console.push_back(line.clone());
        }
        let _ = self.console_tx.send(line.clone());
        line
    }

    fn note(&self, level: LogLevel, message: impl Into<String>) -> ConsoleLine {
        self.push(level, ConsoleSource::Lodestone, message)
    }
}

fn set_state(app: &AppState, slot: &Slot, state: ServerState) {
    if slot.state() != state {
        slot.state_tx.send_replace(state);
        app.events.instance_state(&slot.id, state);
        app.events.changed(ResourceKind::Instances, Some(slot.id.clone()));
    }
}

/* ------------------------------------------------------------------------ */
/* start                                                                    */
/* ------------------------------------------------------------------------ */

/// Whether the instance's owner has accepted the Minecraft EULA. Lodestone
/// only writes `eula.txt` for them once they have.
pub async fn eula_accepted(app: &AppState, instance_id: &str) -> AppResult<bool> {
    if app.config.accept_eula {
        return Ok(true);
    }
    match app.store.owner_of(crate::store::Asset::Instance, instance_id).await? {
        Some(owner) => Ok(app.store.get_user(&owner).await?.eula_accepted_at.is_some()),
        None => Ok(false),
    }
}

/// Starts an instance. Returns once the instance is `preparing`; the rest
/// (downloads, launch, waiting for "Done") continues in the background.
pub async fn start(app: &AppState, id: &str) -> AppResult<()> {
    let slot = app.instances.slot(id);
    let _guard = slot.op.lock().await;
    if slot.state().is_active() {
        return Ok(());
    }
    let record = app.store.get_instance(id).await?;
    let profile = app.store.get_profile(&record.profile_id).await?;
    let world = app.store.get_world(&record.world_id).await?;

    if !eula_accepted(app, id).await? {
        return Err(AppError::conflict(
            "eula_required",
            "Accept the Minecraft EULA (https://aka.ms/MinecraftEULA) in Settings before starting a server.",
        ));
    }
    check_world_compatible(&world, &profile)?;

    {
        let mut live = slot.live.lock().unwrap();
        live.stop_requested = false;
        live.last_error = None;
        live.tps = None;
        live.tick_ms = None;
        live.world_id = world.id.clone();
        live.software = Some(profile.profile.software_id);
        live.version = profile.profile.version.clone();
        live.max_players = profile.properties.get("max-players").and_then(|v| v.parse().ok()).unwrap_or(20);
        live.rcon_password = record.rcon_password.clone();
    }
    if record.last_error.is_some() {
        let mut cleared = record.clone();
        cleared.last_error = None;
        app.store.update_instance(&cleared).await?;
    }
    set_state(app, &slot, ServerState::Preparing);

    let (app, slot) = (app.clone(), slot.clone());
    tokio::spawn(async move {
        if let Err(err) = launch(&app, &slot, record, profile, world).await {
            fail(&app, &slot, err.to_string()).await;
        }
    });
    Ok(())
}

/// `start` for call sites inside the lifecycle itself (crash restarts),
/// boxed to break the async recursion.
fn start_boxed(
    app: AppState,
    id: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send>> {
    Box::pin(async move { start(&app, &id).await })
}

/// Refuses combinations that would damage a world.
pub fn check_world_compatible(world: &World, profile: &ProfileRecord) -> AppResult<()> {
    if let Some(saved) = &world.saved_with {
        let version = &profile.profile.version;
        if crate::logparse::version_at_least(version, saved) == Some(false) {
            return Err(AppError::conflict(
                "world_newer",
                format!(
                    "\"{}\" was last saved by Minecraft {saved}. Opening it with {version} could corrupt it. Use a configuration on {saved} or newer, or restore a backup from before the upgrade.",
                    world.name
                ),
            ));
        }
    }
    let wants = profile.profile.software_id.world_layout();
    if world.generated && world.layout == WorldLayout::Bukkit && wants == WorldLayout::Vanilla {
        return Err(AppError::conflict(
            "layout_mismatch",
            format!(
                "\"{}\" was saved by Paper, which keeps the Nether and End in separate folders. {} would not find them. Run it with a Paper configuration.",
                world.name, profile.profile.software
            ),
        ));
    }
    Ok(())
}

async fn launch(
    app: &AppState,
    slot: &Arc<Slot>,
    record: InstanceRecord,
    profile: ProfileRecord,
    world: World,
) -> AppResult<()> {
    let p = &profile.profile;
    let job = app.jobs.create(JobKind::Prepare, &record.id, format!("Preparing {} {}", p.software, p.version)).await?;
    slot.live.lock().unwrap().job_id = Some(job.id().await);
    slot.note(LogLevel::Info, format!("Preparing {} {} for \"{}\"", p.software, p.version, world.name));

    let prepared = prepare(app, &job, &record, &profile, &world).await;
    let spec = match prepared {
        Ok(spec) => {
            job.succeed(format!("{} {} is ready", p.software, p.version)).await;
            spec
        }
        Err(err) => {
            job.fail(err.to_string()).await;
            return Err(err);
        }
    };
    slot.live.lock().unwrap().job_id = None;

    if slot.live.lock().unwrap().stop_requested {
        slot.note(LogLevel::Info, "Start cancelled.");
        set_state(app, slot, ServerState::Stopped);
        return Ok(());
    }

    set_state(app, slot, ServerState::Starting);
    slot.note(LogLevel::Info, format!("Starting container {} ({})", spec.container_name, spec.image));
    let process = app.runtime.start(&spec).await?;
    {
        let mut live = slot.live.lock().unwrap();
        live.container_id = Some(process.container_id.clone());
        live.rcon_addr = Some(process.rcon_addr);
        live.started_at = Some(Utc::now());
        live.image = spec.image.clone();
        live.error_tail.clear();
    }
    tokio::spawn(watch_process(app.clone(), slot.clone(), process.events, process.container_id));
    Ok(())
}

/// Downloads what is missing and writes the run directory.
async fn prepare(
    app: &AppState,
    job: &JobHandle,
    record: &InstanceRecord,
    profile: &ProfileRecord,
    world: &World,
) -> AppResult<LaunchSpec> {
    let p = &profile.profile;
    job.progress(None, format!("Resolving {} {}", p.software, p.version)).await;
    let Launch::Jar { artifact, java_major } = app.software.resolve(p.software_id, &p.version).await?;
    let java = match java_major {
        Some(java) => java,
        None => software::java_major(&app.software.catalog().await?, p.software_id, &p.version)?,
    };
    let image = software::image_for(java);
    if java != p.java_major {
        // The publisher knows better than our estimate; remember it.
        if let Ok(mut record) = app.store.get_profile(&p.id).await {
            record.profile.java_major = java;
            record.profile.image = image.clone();
            let _ = app.store.update_profile(&record).await;
            app.events.changed(ResourceKind::Profiles, Some(p.id.clone()));
        }
    }
    job.progress(Some(0.0), format!("Downloading {}", artifact.filename)).await;
    let jar = app.cache.fetch(&artifact, Some(job)).await?;

    job.progress(None, format!("Getting Java {java} ({image})")).await;
    app.runtime.ensure_image(&image, job).await?;

    job.progress(None, "Writing server files").await;
    let run_dir = app.paths.instance(&record.id);
    let universe = app.paths.world(&world.id);
    let profile_dir = app.paths.profile(&p.id);
    for dir in [&run_dir, &universe, &profile_dir] {
        tokio::fs::create_dir_all(dir).await?;
    }
    write_run_files(&run_dir, record, profile, world).await?;

    let mut mounts = vec![
        Mount { host: run_dir.clone(), container: "/server".into(), read_only: false },
        Mount { host: jar, container: "/server/server.jar".into(), read_only: true },
        Mount { host: universe, container: "/server/worlds".into(), read_only: false },
    ];
    // The profile's mods or plugins, from its lockfile.
    job.progress(None, "Placing mods and plugins").await;
    crate::content::install::sync_files(app, &p.id, Some(job)).await?;

    // Plugin and mod folders belong to the profile, so their configs persist
    // across instances that share it. Any other folder a modpack brought
    // (`config/`, `defaultconfigs/`, `kubejs/`, ...) is mounted too.
    let mut content_dirs: Vec<String> = match p.software_id.content_kind() {
        crate::model::ContentKind::Plugins => vec!["plugins".into()],
        crate::model::ContentKind::Mods => vec!["mods".into(), "config".into()],
        crate::model::ContentKind::Vanilla => vec![],
    };
    if let Ok(mut entries) = tokio::fs::read_dir(&profile_dir).await {
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let reserved = RESERVED_DIRS.contains(&name.as_str()) || name.starts_with('.');
            if entry.file_type().await?.is_dir() && !reserved && !content_dirs.contains(&name) {
                content_dirs.push(name);
            }
        }
    }
    for name in &content_dirs {
        let host = profile_dir.join(name);
        tokio::fs::create_dir_all(&host).await?;
        mounts.push(Mount { host, container: format!("/server/{name}"), read_only: false });
    }

    let mut command = vec![
        "java".to_owned(),
        format!("-Xms{}M", p.memory.initial_mb.min(p.memory.max_mb)),
        format!("-Xmx{}M", p.memory.max_mb),
    ];
    command.extend(p.java_args.split_whitespace().map(str::to_owned));
    command.extend(["-jar", "server.jar", "--universe", "worlds", "--nogui"].map(str::to_owned));

    let mut env = record.environment.clone();
    env.insert("LODESTONE_RCON_PASSWORD".into(), record.rcon_password.clone());

    Ok(LaunchSpec {
        instance_id: record.id.clone(),
        container_name: format!(
            "lodestone-{}-{}",
            slugify(&record.name),
            &record.id[record.id.len().saturating_sub(6)..]
        ),
        image,
        mounts,
        workdir: "/server".into(),
        command,
        env,
        ports: record.ports,
        // Heap plus room for metaspace, threads and native buffers.
        memory_limit_mb: p.memory.max_mb + (p.memory.max_mb / 4).max(512),
        cpu_cores: p.cpu_cores,
        software: p.software_id,
        version: p.version.clone(),
        level_folder: world.folder.clone(),
    })
}

/// Run-dir paths a profile folder may not shadow.
const RESERVED_DIRS: &[&str] = &["worlds", "logs", "crash-reports", "libraries", "versions", "cache"];

async fn write_run_files(
    run_dir: &std::path::Path,
    record: &InstanceRecord,
    profile: &ProfileRecord,
    world: &World,
) -> AppResult<()> {
    let props = properties::render(world, &profile.properties, &record.rcon_password, record.ports.query.is_some());
    tokio::fs::write(run_dir.join("server.properties"), props).await?;
    // Only written because the server's owner accepted the EULA in Lodestone.
    tokio::fs::write(
        run_dir.join("eula.txt"),
        "# Accepted by this server's owner in Lodestone (https://aka.ms/MinecraftEULA)\neula=true\n",
    )
    .await?;

    let access = &profile.access;
    let ops: Vec<_> = access
        .ops
        .iter()
        .filter_map(|op| {
            op.uuid
                .as_ref()
                .map(|uuid| json!({ "uuid": uuid, "name": op.name, "level": 4, "bypassesPlayerLimit": false }))
        })
        .collect();
    let whitelist: Vec<_> = access
        .whitelist
        .iter()
        .filter_map(|entry| entry.uuid.as_ref().map(|uuid| json!({ "uuid": uuid, "name": entry.name })))
        .collect();
    let bans: Vec<_> = access
        .bans
        .iter()
        .filter_map(|ban| {
            ban.uuid.as_ref().map(|uuid| {
                json!({
                    "uuid": uuid, "name": ban.name, "created": ban.created_at, "source": ban.banned_by,
                    "expires": ban.expires_at.clone().unwrap_or_else(|| "forever".into()), "reason": ban.reason
                })
            })
        })
        .collect();
    tokio::fs::write(run_dir.join("ops.json"), serde_json::to_vec_pretty(&ops)?).await?;
    tokio::fs::write(run_dir.join("whitelist.json"), serde_json::to_vec_pretty(&whitelist)?).await?;
    tokio::fs::write(run_dir.join("banned-players.json"), serde_json::to_vec_pretty(&bans)?).await?;
    Ok(())
}

async fn fail(app: &AppState, slot: &Arc<Slot>, message: String) {
    tracing::warn!(instance = %slot.id, error = %message, "instance failed");
    slot.note(LogLevel::Error, message.clone());
    {
        let mut live = slot.live.lock().unwrap();
        live.last_error = Some(message.clone());
        live.job_id = None;
        live.container_id = None;
        live.rcon = None;
    }
    if let Ok(mut record) = app.store.get_instance(&slot.id).await {
        record.last_error = Some(message);
        let _ = app.store.update_instance(&record).await;
    }
    set_state(app, slot, ServerState::Error);
}

/* ------------------------------------------------------------------------ */
/* the running process                                                      */
/* ------------------------------------------------------------------------ */

async fn watch_process(
    app: AppState,
    slot: Arc<Slot>,
    mut events: tokio::sync::mpsc::Receiver<ProcessEvent>,
    container_id: String,
) {
    let mut exit_code = None;
    while let Some(event) = events.recv().await {
        match event {
            ProcessEvent::Output(raw) => on_output(&app, &slot, &raw).await,
            ProcessEvent::Exited { code } => {
                exit_code = Some(code);
                break;
            }
        }
    }
    on_exit(&app, &slot, &container_id, exit_code.unwrap_or(-1)).await;
}

async fn on_output(app: &AppState, slot: &Arc<Slot>, raw: &str) {
    let parsed = logparse::parse_line(raw);
    let message = parsed.message.to_owned();
    slot.push(parsed.level, ConsoleSource::Server, message.clone());
    if matches!(parsed.level, LogLevel::Error | LogLevel::Warn) || message.starts_with("\tat ") {
        let mut live = slot.live.lock().unwrap();
        if live.error_tail.len() == 12 {
            live.error_tail.pop_front();
        }
        live.error_tail.push_back(message.clone());
    }

    let world_id = slot.live.lock().unwrap().world_id.clone();
    if logparse::is_done(&message) {
        set_state(app, slot, ServerState::Running);
        on_running(app, slot).await;
    } else if let Some((name, uuid)) = logparse::player_uuid(&message) {
        let _ = app.store.player_joined(&world_id, name, Some(uuid)).await;
        remember_uuid(app, slot, name, uuid).await;
    } else if let Some(name) = logparse::joined(&message) {
        slot.live.lock().unwrap().online.insert(name.to_owned(), Instant::now());
        let _ = app.store.player_joined(&world_id, name, None).await;
        app.events.changed(ResourceKind::Instances, Some(slot.id.clone()));
    } else if let Some(name) = logparse::left(&message) {
        let since = slot.live.lock().unwrap().online.remove(name);
        if let Some(since) = since {
            let _ = app.store.player_left(&world_id, name, since.elapsed().as_secs()).await;
        }
        app.events.changed(ResourceKind::Instances, Some(slot.id.clone()));
    }
}

/// Fills in UUIDs on the profile's access lists as players are seen.
async fn remember_uuid(app: &AppState, slot: &Slot, name: &str, uuid: &str) {
    let Ok(record) = app.store.get_instance(&slot.id).await else { return };
    let Ok(mut profile) = app.store.get_profile(&record.profile_id).await else { return };
    let mut changed = false;
    for entry in profile.access.ops.iter_mut().chain(profile.access.whitelist.iter_mut()) {
        if entry.name.eq_ignore_ascii_case(name) && entry.uuid.is_none() {
            entry.uuid = Some(uuid.to_owned());
            changed = true;
        }
    }
    if changed {
        let _ = app.store.update_profile(&profile).await;
    }
}

/// Records what level.dat says: which version last saved the world, and the
/// seed a random-seed world turned out to have.
pub async fn refresh_level(app: &AppState, world_id: &str) {
    let Ok(mut world) = app.store.get_world(world_id).await else { return };
    let path = app.paths.world(world_id).join(&world.folder).join("level.dat");
    let Ok(Some(info)) = tokio::task::spawn_blocking(move || crate::level::read(&path)).await else { return };
    let mut changed = false;
    if info.data_version.is_some() && info.data_version != world.data_version {
        world.data_version = info.data_version;
        changed = true;
    }
    if info.version_name.is_some() && info.version_name != world.saved_with {
        world.saved_with = info.version_name;
        changed = true;
    }
    if world.seed.is_none() {
        if let Some(seed) = info.seed {
            world.seed = Some(seed.to_string());
            changed = true;
        }
    }
    if changed && app.store.update_world(&world).await.is_ok() {
        app.events.changed(ResourceKind::Worlds, Some(world.id));
    }
}

async fn on_running(app: &AppState, slot: &Arc<Slot>) {
    let world_id = slot.live.lock().unwrap().world_id.clone();
    if let Ok(mut world) = app.store.get_world(&world_id).await {
        let software = slot.live.lock().unwrap().software;
        if !world.generated {
            world.generated = true;
            if let Some(software) = software {
                world.layout = software.world_layout();
            }
        } else if software == Some(ServerSoftwareId::Paper) {
            // Paper migrates vanilla worlds to its split layout on first load.
            world.layout = WorldLayout::Bukkit;
        }
        world.updated_at = now_iso();
        let _ = app.store.update_world(&world).await;
        app.events.changed(ResourceKind::Worlds, Some(world.id));
    }
    refresh_level(app, &world_id).await;

    match connect_rcon(slot).await {
        Ok(()) => {
            apply_access_lists(app, slot).await;
            tokio::spawn(sample(app.clone(), slot.clone()));
        }
        Err(err) => {
            slot.note(LogLevel::Warn, format!("Console commands are unavailable: could not reach RCON ({err})."));
        }
    }
}

async fn connect_rcon(slot: &Slot) -> Result<(), String> {
    let (addr, password) = {
        let live = slot.live.lock().unwrap();
        (live.rcon_addr.ok_or("no RCON address")?, live.rcon_password.clone())
    };
    let mut last_err = String::new();
    for attempt in 0..20 {
        match Rcon::connect(&addr.to_string(), &password, Duration::from_secs(5)).await {
            Ok(rcon) => {
                slot.live.lock().unwrap().rcon = Some(Arc::new(tokio::sync::Mutex::new(rcon)));
                return Ok(());
            }
            Err(err) => {
                last_err = err.to_string();
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1).min(5))).await;
            }
        }
    }
    Err(last_err)
}

/// Runs one RCON command, reconnecting once if the connection dropped.
pub async fn rcon(slot: &Slot, command: &str) -> AppResult<String> {
    for attempt in 0..2 {
        let conn = slot.live.lock().unwrap().rcon.clone();
        let conn = match conn {
            Some(conn) => conn,
            None if attempt == 0 && slot.state() == ServerState::Running => {
                connect_rcon(slot).await.map_err(|err| AppError::conflict("rcon_unavailable", err))?;
                continue;
            }
            None => return Err(AppError::conflict("not_running", "The server is not running.")),
        };
        let result = conn.lock().await.command(command).await;
        match result {
            Ok(reply) => return Ok(reply),
            Err(err) => {
                slot.live.lock().unwrap().rcon = None;
                if attempt == 1 {
                    return Err(AppError::conflict("rcon_unavailable", format!("The server did not answer: {err}")));
                }
            }
        }
    }
    unreachable!("loop returns")
}

/// Access-list entries without a UUID cannot go in the JSON files, so the
/// server resolves them itself through commands once it is up.
async fn apply_access_lists(app: &AppState, slot: &Slot) {
    let Ok(record) = app.store.get_instance(&slot.id).await else { return };
    let Ok(profile) = app.store.get_profile(&record.profile_id).await else { return };
    let mut commands = Vec::new();
    for op in profile.access.ops.iter().filter(|op| op.uuid.is_none()) {
        commands.push(format!("op {}", op.name));
    }
    for entry in profile.access.whitelist.iter().filter(|entry| entry.uuid.is_none()) {
        commands.push(format!("whitelist add {}", entry.name));
    }
    for ban in profile.access.bans.iter().filter(|ban| ban.uuid.is_none()) {
        commands.push(format!("ban {} {}", ban.name, ban.reason));
    }
    for command in commands {
        let _ = rcon(slot, &command).await;
    }
}

/// Samples container stats every few seconds and TPS every ~10s.
async fn sample(app: AppState, slot: Arc<Slot>) {
    let container_id = slot.live.lock().unwrap().container_id.clone();
    let Some(container_id) = container_id else { return };
    let (software, version) = {
        let live = slot.live.lock().unwrap();
        (live.software, live.version.clone())
    };
    let tps_command = match software {
        Some(ServerSoftwareId::Paper) => Some("tps"),
        _ if logparse::version_at_least(&version, "1.20.3") == Some(true) => Some("tick query"),
        _ => None,
    };
    let mut previous: Option<(Instant, ProcessStats)> = None;
    let mut tick = 0u32;
    loop {
        if slot.live.lock().unwrap().container_id.as_deref() != Some(container_id.as_str()) || !slot.state().is_active()
        {
            return;
        }
        if let Ok(Some(stats)) = app.runtime.stats(&container_id).await {
            let mut live = slot.live.lock().unwrap();
            if let Some((at, prev)) = previous {
                let secs = at.elapsed().as_secs_f64().max(0.001);
                live.net_rate = (
                    stats.rx_bytes.saturating_sub(prev.rx_bytes) as f64 / secs,
                    stats.tx_bytes.saturating_sub(prev.tx_bytes) as f64 / secs,
                );
            }
            live.stats = Some(stats);
            previous = Some((Instant::now(), stats));
        }
        if tick.is_multiple_of(4) && slot.state() == ServerState::Running {
            if let Some(command) = tps_command {
                if let Ok(reply) = rcon(&slot, command).await {
                    let parsed = if command == "tps" {
                        logparse::parse_paper_tps(&reply).map(|tps| (tps, None))
                    } else {
                        logparse::parse_tick_query(&reply).map(|(tps, mspt)| (tps, Some(mspt)))
                    };
                    if let Some((tps, mspt)) = parsed {
                        let mut live = slot.live.lock().unwrap();
                        live.tps = Some(tps);
                        live.tick_ms = mspt.or(Some(1000.0 / tps.max(0.1)));
                    }
                }
            }
        }
        tick = tick.wrapping_add(1);
        tokio::time::sleep(Duration::from_millis(2500)).await;
    }
}

async fn on_exit(app: &AppState, slot: &Arc<Slot>, container_id: &str, code: i64) {
    let (stop_requested, world_id, online, tail) = {
        let mut live = slot.live.lock().unwrap();
        live.rcon = None;
        live.container_id = None;
        live.rcon_addr = None;
        live.started_at = None;
        live.stats = None;
        live.tps = None;
        live.tick_ms = None;
        (
            live.stop_requested,
            live.world_id.clone(),
            std::mem::take(&mut live.online),
            live.error_tail.iter().cloned().collect::<Vec<_>>(),
        )
    };
    for (name, since) in online {
        let _ = app.store.player_left(&world_id, &name, since.elapsed().as_secs()).await;
    }
    let _ = app.runtime.remove(container_id).await;
    refresh_world_size(app, &world_id).await;
    refresh_level(app, &world_id).await;

    if stop_requested || code == 0 {
        slot.note(LogLevel::Info, "Server stopped.");
        set_state(app, slot, ServerState::Stopped);
        return;
    }

    // A crash. Restart if the profile allows and we are under its threshold.
    let profile = match app.store.get_instance(&slot.id).await {
        Ok(record) => app.store.get_profile(&record.profile_id).await.ok(),
        Err(_) => None,
    };
    let recent = {
        let mut live = slot.live.lock().unwrap();
        live.crashes.retain(|at| at.elapsed() < CRASH_WINDOW);
        live.crashes.push(Instant::now());
        live.crashes.len() as u32
    };
    let detail = if tail.is_empty() { String::new() } else { format!(" Last output: {}", tail.join(" | ")) };
    let message = format!("The server exited unexpectedly (code {code}).{detail}");
    match profile {
        Some(profile) if profile.profile.restart_on_crash && recent <= profile.profile.crash_threshold => {
            slot.note(
                LogLevel::Warn,
                format!(
                    "{message} Restarting ({recent} of {} allowed in 10 minutes).",
                    profile.profile.crash_threshold
                ),
            );
            set_state(app, slot, ServerState::Stopped);
            let app = app.clone();
            let id = slot.id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if let Err(err) = start_boxed(app.clone(), id.clone()).await {
                    tracing::warn!(instance = %id, error = %err, "restart after crash failed");
                }
            });
        }
        Some(profile) if profile.profile.restart_on_crash => {
            fail(
                app,
                slot,
                format!("{message} It crashed {recent} times in 10 minutes, so Lodestone stopped restarting it."),
            )
            .await;
        }
        _ => fail(app, slot, message).await,
    }
}

pub async fn refresh_world_size(app: &AppState, world_id: &str) {
    let dir = app.paths.world(world_id);
    let size = tokio::task::spawn_blocking(move || crate::util::dir_size(&dir)).await.unwrap_or(0);
    if let Ok(mut world) = app.store.get_world(world_id).await {
        if world.size_bytes != size {
            world.size_bytes = size;
            let _ = app.store.update_world(&world).await;
            app.events.changed(ResourceKind::Worlds, Some(world.id));
        }
    }
}

/* ------------------------------------------------------------------------ */
/* stop / restart / commands                                                */
/* ------------------------------------------------------------------------ */

/// Asks the server to stop and returns immediately.
pub async fn stop(app: &AppState, id: &str) -> AppResult<()> {
    let slot = app.instances.slot(id);
    let _guard = slot.op.lock().await;
    request_stop(app, &slot).await
}

async fn request_stop(app: &AppState, slot: &Arc<Slot>) -> AppResult<()> {
    let state = slot.state();
    if !state.is_active() {
        return Ok(());
    }
    let container = {
        let mut live = slot.live.lock().unwrap();
        live.stop_requested = true;
        live.container_id.clone()
    };
    if state == ServerState::Preparing {
        // `launch` notices the flag once preparation finishes.
        return Ok(());
    }
    set_state(app, slot, ServerState::Stopping);
    slot.note(LogLevel::Info, "Stopping the server…");
    if let Some(container) = container {
        // `stop` over RCON saves and exits cleanly; the runtime stop is the
        // safety net that kills the container if it hangs.
        let _ = rcon(slot, "stop").await;
        let runtime = app.runtime.clone();
        let grace = app.config.stop_timeout;
        tokio::spawn(async move {
            if let Err(err) = runtime.stop(&container, grace).await {
                tracing::warn!(error = %err, "runtime stop failed");
            }
        });
    } else {
        set_state(app, slot, ServerState::Stopped);
    }
    Ok(())
}

/// Stops and waits until the instance is no longer active.
pub async fn stop_and_wait(app: &AppState, id: &str) -> AppResult<()> {
    let slot = app.instances.slot(id);
    let mut states = slot.subscribe_state();
    {
        let _guard = slot.op.lock().await;
        request_stop(app, &slot).await?;
    }
    let limit = app.config.stop_timeout + Duration::from_secs(30);
    let waited = tokio::time::timeout(limit, async {
        while states.borrow_and_update().is_active() {
            if states.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
    waited.map_err(|_| AppError::conflict("stop_timeout", "The server did not stop in time."))
}

pub async fn restart(app: &AppState, id: &str) -> AppResult<()> {
    let app = app.clone();
    let id = id.to_owned();
    let slot = app.instances.slot(&id);
    if !slot.state().is_active() {
        return start(&app, &id).await;
    }
    tokio::spawn(async move {
        if let Err(err) = stop_and_wait(&app, &id).await {
            tracing::warn!(instance = %id, error = %err, "restart: stop failed");
            return;
        }
        if let Err(err) = start(&app, &id).await {
            let slot = app.instances.slot(&id);
            slot.note(LogLevel::Error, format!("Restart failed: {err}"));
        }
    });
    Ok(())
}

/// Runs a console command and returns the lines it produced.
pub async fn command(app: &AppState, id: &str, command: &str) -> AppResult<Vec<ConsoleLine>> {
    let slot = app.instances.slot(id);
    let command = command.trim().trim_start_matches('/');
    if command.is_empty() {
        return Err(AppError::bad_request("Type a command first."));
    }
    if slot.state() != ServerState::Running {
        return Err(AppError::conflict("not_running", "Start the server to run commands."));
    }
    let mut lines = vec![slot.push(LogLevel::Info, ConsoleSource::Rcon, format!("> {command}"))];
    let reply = rcon(&slot, command).await?;
    for line in logparse::strip_colors(&reply).lines().filter(|line| !line.trim().is_empty()) {
        lines.push(slot.push(LogLevel::Info, ConsoleSource::Rcon, line.to_owned()));
    }
    let _ = app;
    Ok(lines)
}

/* ------------------------------------------------------------------------ */
/* status                                                                   */
/* ------------------------------------------------------------------------ */

pub fn status(app: &AppState, id: &str, world_bytes: u64) -> InstanceStatus {
    let slot = app.instances.slot(id);
    let state = slot.state();
    let live = slot.live.lock().unwrap();
    let host = app.host.latest();
    let stats = live.stats.unwrap_or_default();
    let uptime = live.started_at.map(|at| (Utc::now() - at).num_seconds().max(0) as u64);
    InstanceStatus {
        state,
        started_at: live.started_at.map(|at| at.to_rfc3339()),
        uptime_seconds: uptime,
        players: PlayerCount { online: live.online.len() as u32, max: live.max_players },
        memory: MemoryUsage { used_bytes: stats.memory_used, limit_bytes: stats.memory_limit },
        cpu_percent: stats.cpu_percent,
        tps: live.tps,
        tick_ms: live.tick_ms,
        disk: DiskUsage { used_bytes: world_bytes, total_bytes: host.disk_total_bytes },
        network: NetworkUsage {
            rx_per_sec: live.net_rate.0,
            tx_per_sec: live.net_rate.1,
            rx_total: stats.rx_bytes,
            tx_total: stats.tx_bytes,
            dropped: stats.dropped,
        },
        host,
        container: ContainerInfo {
            id: live.container_id.clone().unwrap_or_default(),
            name: live.container_id.clone().unwrap_or_default(),
            image: live.image.clone(),
            healthy: state == ServerState::Running && live.rcon.is_some(),
        },
    }
}

/// Day and time of day from a running server attached to `world_id`.
pub async fn world_time(app: &AppState, instance_id: &str) -> Option<(i64, String)> {
    let slot = app.instances.slot(instance_id);
    if slot.state() != ServerState::Running {
        return None;
    }
    let day = logparse::parse_time(&rcon(&slot, "time query day").await.ok()?)?;
    let daytime = logparse::parse_time(&rcon(&slot, "time query daytime").await.ok()?)?;
    Some((day, logparse::daytime_label(daytime)))
}

/* ------------------------------------------------------------------------ */
/* boot                                                                     */
/* ------------------------------------------------------------------------ */

/// Re-attaches to servers that outlived a Lodestone restart, then starts
/// instances whose profile asks for it.
pub async fn reconcile(app: &AppState) -> AppResult<()> {
    for record in app.store.list_instances(None).await? {
        let slot = app.instances.slot(&record.id);
        match app.runtime.adopt(&record.id).await {
            Ok(Some(process)) => {
                let profile = app.store.get_profile(&record.profile_id).await.ok();
                {
                    let mut live = slot.live.lock().unwrap();
                    live.container_id = Some(process.container_id.clone());
                    live.rcon_addr = Some(process.rcon_addr);
                    live.rcon_password = record.rcon_password.clone();
                    live.world_id = record.world_id.clone();
                    live.started_at = Some(Utc::now());
                    if let Some(profile) = profile {
                        live.software = Some(profile.profile.software_id);
                        live.version = profile.profile.version.clone();
                    }
                }
                slot.note(LogLevel::Info, "Lodestone restarted; re-attached to the running server.");
                set_state(app, &slot, ServerState::Running);
                tokio::spawn(watch_process(app.clone(), slot.clone(), process.events, process.container_id));
                let (app, slot) = (app.clone(), slot.clone());
                tokio::spawn(async move { on_running(&app, &slot).await });
            }
            Ok(None) => {
                let profile = app.store.get_profile(&record.profile_id).await?;
                if profile.profile.auto_start {
                    if let Err(err) = start(app, &record.id).await {
                        slot.note(LogLevel::Warn, format!("Auto-start skipped: {err}"));
                    }
                }
            }
            Err(err) => tracing::warn!(instance = %record.id, error = %err, "could not inspect container"),
        }
    }
    Ok(())
}
