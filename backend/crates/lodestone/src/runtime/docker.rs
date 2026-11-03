//! Runs servers as Docker containers built from stock `eclipse-temurin`
//! JRE images. Containers are disposable: everything that matters lives
//! in bind mounts under the data dir, so a container can be removed after
//! every stop and recreated on the next start.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StatsOptions, StopContainerOptions, WaitContainerOptions,
};
use bollard::errors::Error as DockerError;
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, PortBinding};
use bollard::Docker;
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{LaunchSpec, Process, ProcessEvent, ProcessStats, Runtime};
use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;
use crate::model::RuntimeInfo;
use crate::properties::{CONTAINER_GAME_PORT, CONTAINER_QUERY_PORT, CONTAINER_RCON_PORT};

/// Label that marks containers Lodestone owns, valued with the instance id.
pub const INSTANCE_LABEL: &str = "lodestone.instance";

pub struct DockerRuntime {
    docker: Option<Docker>,
    connect_error: Option<String>,
    /// Rewrites data-dir paths to host paths when Lodestone itself runs in
    /// a container: `(data_dir, host_data_dir)`.
    path_map: Option<(PathBuf, PathBuf)>,
    /// `uid:gid` that owns the data dir, so server files stay editable.
    user: Option<String>,
}

impl DockerRuntime {
    pub fn connect(data_dir: &Path, host_data_dir: Option<PathBuf>) -> Self {
        let (docker, connect_error) = match connect() {
            Ok(docker) => (Some(docker), None),
            Err(err) => (None, Some(err)),
        };
        Self {
            docker,
            connect_error,
            path_map: host_data_dir.map(|host| (data_dir.to_path_buf(), host)),
            user: owner_of(data_dir),
        }
    }

    fn docker(&self) -> AppResult<&Docker> {
        self.docker.as_ref().ok_or_else(|| {
            AppError::conflict(
                "docker_unavailable",
                format!(
                    "Docker is not reachable ({}). Start Docker Desktop or the Docker service, then try again.",
                    self.connect_error.as_deref().unwrap_or("unknown error")
                ),
            )
        })
    }

    fn host_path(&self, path: &Path) -> String {
        match &self.path_map {
            Some((data, host)) => match path.strip_prefix(data) {
                Ok(rest) => host.join(rest).display().to_string(),
                Err(_) => path.display().to_string(),
            },
            None => path.display().to_string(),
        }
    }

    /// Streams a container's output and exit into a [`Process`] channel.
    fn follow(&self, docker: Docker, container_id: String, tail: &str) -> mpsc::Receiver<ProcessEvent> {
        let (tx, rx) = mpsc::channel(1024);
        let tail = tail.to_owned();
        tokio::spawn(async move {
            let options =
                LogsOptions::<String> { follow: true, stdout: true, stderr: true, tail, ..Default::default() };
            let mut logs = docker.logs(&container_id, Some(options));
            let mut partial = String::new();
            while let Some(chunk) = logs.next().await {
                let Ok(chunk) = chunk else { break };
                let bytes = match chunk {
                    LogOutput::StdOut { message } | LogOutput::StdErr { message } | LogOutput::Console { message } => {
                        message
                    }
                    LogOutput::StdIn { .. } => continue,
                };
                partial.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(newline) = partial.find('\n') {
                    let line: String = partial.drain(..=newline).collect();
                    if tx.send(ProcessEvent::Output(line.trim_end().to_owned())).await.is_err() {
                        return;
                    }
                }
            }
            if !partial.trim().is_empty() {
                let _ = tx.send(ProcessEvent::Output(partial.trim_end().to_owned())).await;
            }
            // The log stream ends when the container stops; collect the code.
            let code = match docker.wait_container(&container_id, None::<WaitContainerOptions<String>>).next().await {
                Some(Ok(response)) => response.status_code,
                Some(Err(DockerError::DockerContainerWaitError { code, .. })) => code,
                _ => -1,
            };
            let _ = tx.send(ProcessEvent::Exited { code }).await;
        });
        rx
    }
}

/// `DOCKER_HOST`, the standard socket, then Docker Desktop's per-user socket.
fn connect() -> Result<Docker, String> {
    if std::env::var_os("DOCKER_HOST").is_some() || Path::new("/var/run/docker.sock").exists() {
        return Docker::connect_with_socket_defaults().map_err(|err| err.to_string());
    }
    if let Some(home) = std::env::var_os("HOME") {
        let desktop = Path::new(&home).join(".docker/run/docker.sock");
        if desktop.exists() {
            return Docker::connect_with_unix(&desktop.display().to_string(), 120, bollard::API_DEFAULT_VERSION)
                .map_err(|err| err.to_string());
        }
    }
    Err("no Docker socket found".into())
}

#[cfg(unix)]
fn owner_of(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some(format!("{}:{}", meta.uid(), meta.gid()))
}

#[cfg(not(unix))]
fn owner_of(_path: &Path) -> Option<String> {
    None
}

fn docker_err(context: &str, err: DockerError) -> AppError {
    AppError::Internal(anyhow::anyhow!("{context}: {err}"))
}

fn binding(ip: Ipv4Addr, port: u16) -> Option<Vec<PortBinding>> {
    Some(vec![PortBinding { host_ip: Some(ip.to_string()), host_port: Some(port.to_string()) }])
}

#[async_trait]
impl Runtime for DockerRuntime {
    fn kind(&self) -> &'static str {
        "docker"
    }

    async fn info(&self) -> RuntimeInfo {
        let version = match &self.docker {
            Some(docker) => docker.version().await.map_err(|err| err.to_string()),
            None => Err(self.connect_error.clone().unwrap_or_default()),
        };
        match version {
            Ok(version) => {
                RuntimeInfo { kind: "docker".into(), available: true, version: version.version, message: None }
            }
            Err(err) => RuntimeInfo {
                kind: "docker".into(),
                available: false,
                version: None,
                message: Some(format!("Docker is not reachable: {err}. Start Docker, then recheck.")),
            },
        }
    }

    async fn ensure_image(&self, image: &str, job: &JobHandle) -> AppResult<()> {
        let docker = self.docker()?;
        if docker.inspect_image(image).await.is_ok() {
            return Ok(());
        }
        job.progress(None, format!("Downloading Java runtime {image}")).await;
        let options = CreateImageOptions { from_image: image, ..Default::default() };
        let mut pull = docker.create_image(Some(options), None, None);
        let mut layers: HashMap<String, (i64, i64)> = HashMap::new();
        let mut last = std::time::Instant::now();
        while let Some(update) = pull.next().await {
            let update = update.map_err(|err| docker_err(&format!("pulling {image}"), err))?;
            if let (Some(id), Some(detail)) = (update.id, update.progress_detail) {
                if let (Some(current), Some(total)) = (detail.current, detail.total) {
                    layers.insert(id, (current, total));
                }
            }
            if last.elapsed() > Duration::from_millis(500) && !layers.is_empty() {
                last = std::time::Instant::now();
                let (done, total) = layers.values().fold((0, 0), |(d, t), (c, n)| (d + c, t + n));
                let progress = (total > 0).then(|| done as f64 / total as f64);
                job.progress(progress, format!("Downloading Java runtime {image}")).await;
            }
        }
        Ok(())
    }

    async fn start(&self, spec: &LaunchSpec) -> AppResult<Process> {
        let docker = self.docker()?.clone();
        // A container left over from a crash of Lodestone would block the name.
        let _ = docker
            .remove_container(&spec.container_name, Some(RemoveContainerOptions { force: true, ..Default::default() }))
            .await;

        let game = format!("{CONTAINER_GAME_PORT}/tcp");
        let rcon = format!("{CONTAINER_RCON_PORT}/tcp");
        let query = format!("{CONTAINER_QUERY_PORT}/udp");
        let mut exposed = HashMap::from([(game.clone(), HashMap::new()), (rcon.clone(), HashMap::new())]);
        let mut ports = HashMap::from([
            (game, binding(Ipv4Addr::UNSPECIFIED, spec.ports.minecraft)),
            // RCON is for Lodestone only: never published beyond this host.
            (rcon, binding(Ipv4Addr::LOCALHOST, spec.ports.rcon)),
        ]);
        if let Some(port) = spec.ports.query {
            exposed.insert(query.clone(), HashMap::new());
            ports.insert(query, binding(Ipv4Addr::UNSPECIFIED, port));
        }
        let binds = spec
            .mounts
            .iter()
            .map(|mount| {
                format!(
                    "{}:{}{}",
                    self.host_path(&mount.host),
                    mount.container,
                    if mount.read_only { ":ro" } else { "" }
                )
            })
            .collect();
        let mut env: Vec<String> = spec.env.iter().map(|(key, value)| format!("{key}={value}")).collect();
        // Running as the data dir's owner leaves no root-owned files behind;
        // that user has no home in the image, so give Java one.
        env.push("HOME=/server".into());

        let config = Config {
            image: Some(spec.image.clone()),
            cmd: Some(spec.command.clone()),
            working_dir: Some(spec.workdir.clone()),
            env: Some(env),
            user: self.user.clone(),
            labels: Some(HashMap::from([(INSTANCE_LABEL.to_owned(), spec.instance_id.clone())])),
            exposed_ports: Some(exposed),
            open_stdin: Some(true),
            attach_stdin: Some(false),
            tty: Some(false),
            host_config: Some(HostConfig {
                binds: Some(binds),
                port_bindings: Some(ports),
                memory: Some(spec.memory_limit_mb as i64 * 1024 * 1024),
                nano_cpus: spec.cpu_cores.map(|cores| (cores * 1e9) as i64),
                init: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = docker
            .create_container(
                Some(CreateContainerOptions { name: spec.container_name.clone(), platform: None }),
                config,
            )
            .await
            .map_err(|err| docker_err("creating the container", err))?;
        docker.start_container(&created.id, None::<StartContainerOptions<String>>).await.map_err(|err| match err {
            DockerError::DockerResponseServerError { message, .. }
                if message.contains("port is already allocated") || message.contains("address already in use") =>
            {
                AppError::conflict(
                    "port_in_use",
                    format!("A port this server needs is already in use on the host: {message}"),
                )
            }
            other => docker_err("starting the container", other),
        })?;
        let events = self.follow(docker, created.id.clone(), "all");
        Ok(Process {
            container_id: created.id,
            events,
            rcon_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, spec.ports.rcon)),
        })
    }

    async fn adopt(&self, instance_id: &str) -> AppResult<Option<Process>> {
        let Ok(docker) = self.docker() else { return Ok(None) };
        let filters = HashMap::from([("label".to_owned(), vec![format!("{INSTANCE_LABEL}={instance_id}")])]);
        let containers = docker
            .list_containers(Some(ListContainersOptions { all: true, filters, ..Default::default() }))
            .await
            .map_err(|err| docker_err("listing containers", err))?;
        for container in containers {
            let Some(id) = container.id else { continue };
            if container.state.as_deref() != Some("running") {
                let _ = docker
                    .remove_container(&id, Some(RemoveContainerOptions { force: true, ..Default::default() }))
                    .await;
                continue;
            }
            let rcon_port = container
                .ports
                .unwrap_or_default()
                .iter()
                .find(|port| port.private_port == CONTAINER_RCON_PORT)
                .and_then(|port| port.public_port)
                .unwrap_or(CONTAINER_RCON_PORT);
            let events = self.follow(docker.clone(), id.clone(), "200");
            return Ok(Some(Process {
                container_id: id,
                events,
                rcon_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, rcon_port)),
            }));
        }
        Ok(None)
    }

    async fn stop(&self, container_id: &str, grace: Duration) -> AppResult<()> {
        let docker = self.docker()?;
        match docker.stop_container(container_id, Some(StopContainerOptions { t: grace.as_secs() as i64 })).await {
            Ok(()) => Ok(()),
            // Already stopped or gone: that is what we wanted.
            Err(DockerError::DockerResponseServerError { status_code: 304 | 404, .. }) => Ok(()),
            Err(err) => Err(docker_err("stopping the container", err)),
        }
    }

    async fn remove(&self, container_id: &str) -> AppResult<()> {
        let docker = self.docker()?;
        match docker
            .remove_container(container_id, Some(RemoveContainerOptions { force: true, ..Default::default() }))
            .await
        {
            Ok(()) | Err(DockerError::DockerResponseServerError { status_code: 404 | 409, .. }) => Ok(()),
            Err(err) => Err(docker_err("removing the container", err)),
        }
    }

    async fn stats(&self, container_id: &str) -> AppResult<Option<ProcessStats>> {
        let docker = self.docker()?;
        let mut stream = docker.stats(container_id, Some(StatsOptions { stream: false, one_shot: false }));
        let Some(Ok(stats)) = stream.next().await else { return Ok(None) };
        let cpu_delta =
            stats.cpu_stats.cpu_usage.total_usage.saturating_sub(stats.precpu_stats.cpu_usage.total_usage) as f64;
        let system_delta = stats
            .cpu_stats
            .system_cpu_usage
            .unwrap_or(0)
            .saturating_sub(stats.precpu_stats.system_cpu_usage.unwrap_or(0)) as f64;
        let cpus = stats.cpu_stats.online_cpus.unwrap_or(1).max(1) as f64;
        let cpu_percent = if system_delta > 0.0 { cpu_delta / system_delta * cpus * 100.0 } else { 0.0 };
        // Page cache is reclaimable; report what the JVM actually holds.
        let cache = match stats.memory_stats.stats {
            Some(bollard::container::MemoryStatsStats::V1(v1)) => v1.total_inactive_file,
            Some(bollard::container::MemoryStatsStats::V2(v2)) => v2.inactive_file,
            None => 0,
        };
        let (rx, tx, dropped) =
            stats.networks.unwrap_or_default().values().fold((0, 0, 0), |(rx, tx, dropped), net| {
                (rx + net.rx_bytes, tx + net.tx_bytes, dropped + net.rx_dropped + net.tx_dropped)
            });
        Ok(Some(ProcessStats {
            cpu_percent,
            memory_used: stats.memory_stats.usage.unwrap_or(0).saturating_sub(cache),
            memory_limit: stats.memory_stats.limit.unwrap_or(0),
            rx_bytes: rx,
            tx_bytes: tx,
            dropped,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_paths_for_containerised_lodestone() {
        let runtime = DockerRuntime {
            docker: None,
            connect_error: Some("test".into()),
            path_map: Some(("/data".into(), "/srv/lodestone".into())),
            user: None,
        };
        assert_eq!(runtime.host_path(Path::new("/data/worlds/w1")), "/srv/lodestone/worlds/w1");
        assert_eq!(runtime.host_path(Path::new("/elsewhere/x")), "/elsewhere/x");
    }

    #[test]
    fn unreachable_docker_explains_itself() {
        let runtime =
            DockerRuntime { docker: None, connect_error: Some("no socket".into()), path_map: None, user: None };
        let err = runtime.docker().unwrap_err();
        assert!(err.to_string().contains("Start Docker"));
    }
}
