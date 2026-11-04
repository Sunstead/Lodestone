//! A simulated Minecraft server. It prints realistic log lines, serves a
//! real RCON socket on the instance's RCON port and answers common
//! commands, so the supervisor, console and player code paths are the same
//! ones a real server exercises.
//!
//! Test hooks (sent as console commands):
//! `lodestone:join <name>`, `lodestone:leave <name>`, `lodestone:crash`.

use std::collections::{BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mc_rcon::Packet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use super::{LaunchSpec, Process, ProcessEvent, ProcessStats, Runtime, TaskOutput, TaskSpec};
use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;
use crate::model::{RuntimeInfo, ServerSoftwareId};

pub struct FakeRuntime {
    /// Multiplier on simulated delays; 0 makes everything immediate.
    speed: f64,
    /// Simulate a player wandering in, for a livelier dev UI.
    chatter: bool,
    processes: Mutex<HashMap<String, FakeHandle>>,
}

struct FakeHandle {
    shutdown: watch::Sender<Option<i64>>,
    memory_limit: u64,
    started: std::time::Instant,
}

impl FakeRuntime {
    pub fn new(speed: f64, chatter: bool) -> Self {
        Self { speed, chatter, processes: Mutex::new(HashMap::new()) }
    }
}

#[async_trait]
impl Runtime for FakeRuntime {
    fn kind(&self) -> &'static str {
        "fake"
    }

    async fn info(&self) -> RuntimeInfo {
        RuntimeInfo {
            kind: "fake".into(),
            available: true,
            version: Some(crate::VERSION.into()),
            message: Some("Simulated servers: nothing is downloaded or run.".into()),
        }
    }

    async fn ensure_image(&self, image: &str, job: &JobHandle) -> AppResult<()> {
        job.progress(None, format!("Using simulated {image}")).await;
        Ok(())
    }

    async fn start(&self, spec: &LaunchSpec) -> AppResult<Process> {
        let rcon_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, spec.ports.rcon));
        let listener = TcpListener::bind(rcon_addr).await.map_err(|err| {
            AppError::conflict(
                "port_in_use",
                format!("RCON port {} is already in use on this host ({err}).", spec.ports.rcon),
            )
        })?;
        let container_id = format!("fake-{}", spec.instance_id);
        let (events_tx, events) = mpsc::channel(1024);
        let (shutdown, shutdown_rx) = watch::channel(None);
        let password = spec.env.get("LODESTONE_RCON_PASSWORD").cloned().unwrap_or_default();
        let world_dir = spec.host_path("/server/worlds").map(|dir| dir.join(&spec.level_folder));

        let sim = Arc::new(Sim {
            speed: self.speed,
            chatter: self.chatter,
            paper: spec.software == ServerSoftwareId::Paper,
            version: spec.version.clone(),
            level: spec.level_folder.clone(),
            events: events_tx,
            state: Mutex::new(GameState::default()),
            shutdown: shutdown.clone(),
        });
        tokio::spawn(sim.clone().run(listener, password, shutdown_rx, world_dir));

        self.processes.lock().unwrap().insert(
            container_id.clone(),
            FakeHandle {
                shutdown,
                memory_limit: spec.memory_limit_mb as u64 * 1024 * 1024,
                started: std::time::Instant::now(),
            },
        );
        Ok(Process { container_id, events, rcon_addr })
    }

    async fn adopt(&self, _instance_id: &str) -> AppResult<Option<Process>> {
        // Simulated servers do not outlive the process that ran them.
        Ok(None)
    }

    async fn stop(&self, container_id: &str, grace: Duration) -> AppResult<()> {
        let shutdown = self.processes.lock().unwrap().get(container_id).map(|handle| handle.shutdown.clone());
        if let Some(shutdown) = shutdown {
            let _ = shutdown.send(Some(0));
            let mut rx = shutdown.subscribe();
            // Like `docker stop`: wait for a graceful exit, then kill.
            let graceful = tokio::time::timeout(grace, async {
                while !rx.borrow().is_some_and(|code| code < 0) {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await;
            if graceful.is_err() {
                let _ = shutdown.send(Some(137));
            }
        }
        Ok(())
    }

    async fn remove(&self, container_id: &str) -> AppResult<()> {
        self.processes.lock().unwrap().remove(container_id);
        Ok(())
    }

    async fn stats(&self, container_id: &str) -> AppResult<Option<ProcessStats>> {
        let processes = self.processes.lock().unwrap();
        Ok(processes.get(container_id).map(|handle| {
            let t = handle.started.elapsed().as_secs_f64();
            let wave = (t / 7.0).sin() * 0.5 + 0.5;
            ProcessStats {
                cpu_percent: 8.0 + wave * 22.0,
                memory_used: (handle.memory_limit as f64 * (0.35 + wave * 0.2)) as u64,
                memory_limit: handle.memory_limit,
                rx_bytes: (t * 42_000.0) as u64,
                tx_bytes: (t * 180_000.0) as u64,
                dropped: 0,
            }
        }))
    }

    /// Pretends to be a mod loader installer: `--installServer <dir>` gets
    /// a `libraries/` tree with an args file, as Forge and NeoForge write.
    async fn run_task(&self, task: &TaskSpec, job: &JobHandle) -> AppResult<TaskOutput> {
        let Some(at) = task.command.iter().position(|arg| arg == "--installServer") else {
            return Ok(TaskOutput { code: 1, tail: vec!["fake runtime: unknown task".into()] });
        };
        let dir = task.command.get(at + 1).and_then(|target| {
            task.mounts.iter().find(|mount| &mount.container == target).map(|mount| mount.host.clone())
        });
        let Some(dir) = dir else {
            return Ok(TaskOutput { code: 1, tail: vec!["fake runtime: install dir isn't mounted".into()] });
        };
        job.progress(None, "Extracting libraries").await;
        let args = dir.join("libraries/net/lodestone/fake-loader/1.0");
        std::fs::create_dir_all(&args)?;
        std::fs::write(args.join("unix_args.txt"), "-cp libraries/fake.jar lodestone.FakeLoader\n")?;
        std::fs::write(dir.join("run.sh"), "#!/bin/sh\n")?;
        Ok(TaskOutput { code: 0, tail: vec!["The server installed successfully".into()] })
    }
}

#[derive(Default)]
struct GameState {
    online: BTreeSet<String>,
    ops: BTreeSet<String>,
    whitelist: BTreeSet<String>,
    banned: BTreeSet<String>,
    whitelist_on: bool,
    ticks: i64,
    saving: bool,
}

struct Sim {
    speed: f64,
    chatter: bool,
    paper: bool,
    version: String,
    level: String,
    events: mpsc::Sender<ProcessEvent>,
    state: Mutex<GameState>,
    /// `Some(code >= 0)`: please exit with code. `Some(-1)`: exited.
    shutdown: watch::Sender<Option<i64>>,
}

impl Sim {
    async fn pause(&self, ms: u64) {
        if self.speed > 0.0 {
            tokio::time::sleep(Duration::from_millis((ms as f64 * self.speed) as u64)).await;
        } else {
            tokio::task::yield_now().await;
        }
    }

    async fn log(&self, level: &str, message: impl AsRef<str>) {
        let time = chrono::Local::now().format("%H:%M:%S");
        let line = if self.paper {
            format!("[{time} {level}]: {}", message.as_ref())
        } else {
            format!("[{time}] [Server thread/{level}]: {}", message.as_ref())
        };
        let _ = self.events.send(ProcessEvent::Output(line)).await;
    }

    async fn run(
        self: Arc<Self>,
        listener: TcpListener,
        password: String,
        mut shutdown: watch::Receiver<Option<i64>>,
        world_dir: Option<std::path::PathBuf>,
    ) {
        let boot = [
            format!("Starting minecraft server version {}", self.version),
            "Loading properties".to_owned(),
            "Default game type: SURVIVAL".to_owned(),
            "Starting Minecraft server on *:25565".to_owned(),
            format!("Preparing level \"{}\"", self.level),
            "Preparing start region for dimension minecraft:overworld".to_owned(),
            "Time elapsed: 812 ms".to_owned(),
            "Starting remote control listener".to_owned(),
            "Thread RCON Listener started".to_owned(),
            "RCON running on 0.0.0.0:25575".to_owned(),
        ];
        for message in boot {
            if shutdown.borrow().is_some() {
                break;
            }
            self.log("INFO", message).await;
            self.pause(180).await;
        }
        if let Some(dir) = &world_dir {
            // Leave something on disk, like a freshly generated world would.
            let _ = std::fs::create_dir_all(dir.join("region"));
            // A real (if tiny) level.dat, keeping the day it had reached.
            let previous = crate::level::read(&dir.join("level.dat"));
            let info = crate::level::LevelInfo {
                data_version: Some(4189),
                version_name: Some(self.version.clone()),
                level_name: Some(self.level.clone()),
                seed: previous.as_ref().and_then(|p| p.seed).or(Some(-4_172_144_997_902_289_642)),
                day_time: Some(previous.as_ref().and_then(|p| p.day_time).unwrap_or(0) + 30_000),
                game_type: Some(0),
                difficulty: Some(2),
                hardcore: Some(false),
            };
            let _ = std::fs::write(dir.join("level.dat"), crate::level::write(&info));
            let _ = std::fs::write(dir.join("region").join("r.0.0.mca"), vec![0u8; 64 * 1024]);
        }
        if shutdown.borrow().is_none() {
            self.log("INFO", "Done (1.042s)! For help, type \"help\"").await;
        }

        let rcon = tokio::spawn(self.clone().serve_rcon(listener, password));
        let chatter = self.chatter.then(|| tokio::spawn(self.clone().chatter()));
        let ticker = tokio::spawn({
            let sim = self.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    sim.state.lock().unwrap().ticks += 1;
                }
            }
        });

        // Wait for a stop request (RCON `stop`, runtime stop, or a crash).
        let code = loop {
            if let Some(code) = *shutdown.borrow_and_update() {
                break code;
            }
            if shutdown.changed().await.is_err() {
                break 0;
            }
        };
        rcon.abort();
        ticker.abort();
        if let Some(task) = chatter {
            task.abort();
        }
        if code == 0 {
            for message in [
                "Stopping server",
                "Saving players",
                "Saving worlds",
                "ThreadedAnvilChunkStorage: All dimensions are saved",
            ] {
                self.log("INFO", message).await;
                self.pause(120).await;
            }
        } else if code != 137 {
            self.log("ERROR", "Encountered an unexpected exception").await;
            let _ =
                self.events.send(ProcessEvent::Output("java.lang.IllegalStateException: simulated crash".into())).await;
            let _ = self
                .events
                .send(ProcessEvent::Output(
                    "\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)".into(),
                ))
                .await;
        }
        let _ = self.events.send(ProcessEvent::Exited { code }).await;
        let _ = self.shutdown.send(Some(-1));
    }

    async fn chatter(self: Arc<Self>) {
        self.pause(4000).await;
        self.join("Steve").await;
        let lines = ["<Steve> anyone up for the nether?", "Steve has made the advancement [Stone Age]", "<Steve> brb"];
        for line in lines.iter().cycle() {
            self.pause(15_000).await;
            self.log("INFO", *line).await;
        }
    }

    async fn join(&self, name: &str) {
        let uuid = fake_uuid(name);
        self.log("INFO", format!("UUID of player {name} is {uuid}")).await;
        self.state.lock().unwrap().online.insert(name.to_owned());
        self.log("INFO", format!("{name} joined the game")).await;
    }

    async fn leave(&self, name: &str, reason: &str) {
        let was_online = self.state.lock().unwrap().online.remove(name);
        if was_online {
            self.log("INFO", format!("{name} lost connection: {reason}")).await;
            self.log("INFO", format!("{name} left the game")).await;
        }
    }

    async fn serve_rcon(self: Arc<Self>, listener: TcpListener, password: String) {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(self.clone().rcon_session(socket, password.clone()));
        }
    }

    async fn rcon_session(self: Arc<Self>, mut socket: TcpStream, password: String) {
        let mut authed = false;
        loop {
            let mut len = [0u8; 4];
            if socket.read_exact(&mut len).await.is_err() {
                return;
            }
            let mut payload = vec![0u8; i32::from_le_bytes(len).clamp(0, 1 << 20) as usize];
            if socket.read_exact(&mut payload).await.is_err() {
                return;
            }
            let Ok(packet) = Packet::decode(&payload) else { return };
            let reply = if packet.kind == 3 {
                authed = packet.body == password;
                Packet { id: if authed { packet.id } else { -1 }, kind: 2, body: String::new() }
            } else if !authed {
                Packet { id: -1, kind: 2, body: String::new() }
            } else {
                let body = self.command(&packet.body).await;
                Packet { id: packet.id, kind: 0, body }
            };
            if socket.write_all(&reply.encode()).await.is_err() {
                return;
            }
        }
    }

    /// Replies like a vanilla server would.
    async fn command(&self, raw: &str) -> String {
        let line = raw.trim().trim_start_matches('/');
        let mut parts = line.split_whitespace();
        let verb = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        let arg = |index: usize| args.get(index).copied().unwrap_or("");
        let rest = |from: usize| args.get(from..).map(|rest| rest.join(" ")).unwrap_or_default();

        match verb {
            "list" => {
                let state = self.state.lock().unwrap();
                let names: Vec<_> = state.online.iter().cloned().collect();
                format!("There are {} of a max of 20 players online: {}", names.len(), names.join(", "))
            }
            "say" => {
                let text = rest(0);
                self.log("INFO", format!("[Server] {text}")).await;
                String::new()
            }
            "stop" => {
                let _ = self.shutdown.send(Some(0));
                "Stopping the server".into()
            }
            "save-all" => {
                self.log("INFO", "Saving the game (this may take a moment!)").await;
                self.log("INFO", "Saved the game").await;
                "Saved the game".into()
            }
            "save-off" => {
                self.state.lock().unwrap().saving = false;
                "Automatic saving is now disabled".into()
            }
            "save-on" => {
                self.state.lock().unwrap().saving = true;
                "Automatic saving is now enabled".into()
            }
            "op" => {
                self.state.lock().unwrap().ops.insert(arg(0).into());
                format!("Made {} a server operator", arg(0))
            }
            "deop" => {
                self.state.lock().unwrap().ops.remove(arg(0));
                format!("Made {} no longer a server operator", arg(0))
            }
            "whitelist" => {
                let mut state = self.state.lock().unwrap();
                match arg(0) {
                    "add" => {
                        state.whitelist.insert(arg(1).into());
                        format!("Added {} to the whitelist", arg(1))
                    }
                    "remove" => {
                        state.whitelist.remove(arg(1));
                        format!("Removed {} from the whitelist", arg(1))
                    }
                    "on" => {
                        state.whitelist_on = true;
                        "Whitelist is now turned on".into()
                    }
                    "off" => {
                        state.whitelist_on = false;
                        "Whitelist is now turned off".into()
                    }
                    "reload" => "Reloaded the whitelist".into(),
                    _ => format!("There are {} whitelisted player(s)", state.whitelist.len()),
                }
            }
            "kick" => {
                let name = arg(0).to_owned();
                let reason = if args.len() > 1 { rest(1) } else { "Kicked by an operator".into() };
                self.leave(&name, &reason).await;
                format!("Kicked {name}: {reason}")
            }
            "ban" => {
                let name = arg(0).to_owned();
                self.state.lock().unwrap().banned.insert(name.clone());
                self.leave(&name, "You are banned from this server").await;
                format!("Banned {name}")
            }
            "pardon" => {
                self.state.lock().unwrap().banned.remove(arg(0));
                format!("Unbanned {}", arg(0))
            }
            "time" if arg(0) == "query" => {
                let ticks = self.state.lock().unwrap().ticks + 1000;
                match arg(1) {
                    "day" => format!("The time is {}", ticks / 24_000),
                    "gametime" => format!("The time is {ticks}"),
                    _ => format!("The time is {}", ticks % 24_000),
                }
            }
            "tps" if self.paper => "§6TPS from last 1m, 5m, 15m: §a20.0, §a20.0, §a19.97".into(),
            "tick" if arg(0) == "query" && !self.paper => {
                "The game is running normally\nTarget tick rate: 20.0 per second.\nAverage time per tick: 3.2ms (Target: 50.0ms)".into()
            }
            "lodestone:join" => {
                self.join(arg(0)).await;
                String::new()
            }
            "lodestone:leave" => {
                self.leave(arg(0), "Disconnected").await;
                String::new()
            }
            "lodestone:crash" => {
                let _ = self.shutdown.send(Some(1));
                String::new()
            }
            "" => String::new(),
            other => format!("Unknown or incomplete command, see below for error\n{other}<--[HERE]"),
        }
    }
}

/// Stable, valid-looking UUID derived from a player name.
fn fake_uuid(name: &str) -> String {
    let hex = crate::cache::HashAlgo::Sha1.digest(name.as_bytes());
    format!("{}-{}-4{}-a{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}
