//! Where server processes actually run. The supervisor drives a [`Runtime`];
//! [`fake::FakeRuntime`] simulates a server in-process for dev and tests,
//! and the Docker runtime runs real servers in stock JRE containers.

pub mod fake;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::jobs::JobHandle;
use crate::model::{InstancePorts, RuntimeInfo, ServerSoftwareId, StringMap};

#[derive(Debug, Clone)]
pub struct Mount {
    pub host: PathBuf,
    pub container: String,
    pub read_only: bool,
}

/// Everything needed to start one server process.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub instance_id: String,
    pub container_name: String,
    pub image: String,
    pub mounts: Vec<Mount>,
    pub workdir: String,
    pub command: Vec<String>,
    pub env: StringMap,
    /// Host ports. The game port is published on every interface, RCON on
    /// 127.0.0.1 only.
    pub ports: InstancePorts,
    pub memory_limit_mb: u32,
    pub cpu_cores: Option<f64>,
    /// For runtimes that need to know what they are pretending to run.
    pub software: ServerSoftwareId,
    pub version: String,
    pub level_folder: String,
}

impl LaunchSpec {
    pub fn host_path(&self, container: &str) -> Option<&PathBuf> {
        self.mounts.iter().find(|mount| mount.container == container).map(|mount| &mount.host)
    }
}

#[derive(Debug)]
pub enum ProcessEvent {
    /// One line of stdout/stderr.
    Output(String),
    /// The process ended.
    Exited { code: i64 },
}

/// A started (or re-adopted) server process.
pub struct Process {
    pub container_id: String,
    pub events: mpsc::Receiver<ProcessEvent>,
    /// Where Lodestone reaches the server's RCON.
    pub rcon_addr: SocketAddr,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessStats {
    pub cpu_percent: f64,
    pub memory_used: u64,
    pub memory_limit: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub dropped: u64,
}

#[async_trait]
pub trait Runtime: Send + Sync {
    fn kind(&self) -> &'static str;

    async fn info(&self) -> RuntimeInfo;

    /// Makes sure the runtime image is present, pulling it when needed.
    async fn ensure_image(&self, image: &str, job: &JobHandle) -> AppResult<()>;

    async fn start(&self, spec: &LaunchSpec) -> AppResult<Process>;

    /// Re-attaches to a process that survived a Lodestone restart.
    async fn adopt(&self, instance_id: &str) -> AppResult<Option<Process>>;

    /// Asks the process to stop (SIGTERM; Minecraft saves on it) and kills
    /// it after `grace`.
    async fn stop(&self, container_id: &str, grace: Duration) -> AppResult<()>;

    /// Removes a stopped container. Data lives in bind mounts, not here.
    async fn remove(&self, container_id: &str) -> AppResult<()>;

    async fn stats(&self, container_id: &str) -> AppResult<Option<ProcessStats>>;
}
