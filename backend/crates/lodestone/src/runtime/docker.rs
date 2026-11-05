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
use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
use bollard::Docker;
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{LaunchSpec, Process, ProcessEvent, ProcessStats, Runtime, TaskOutput, TaskSpec};
use crate::error::{AppError, AppResult};
use crate::jobs::JobHandle;
use crate::model::RuntimeInfo;
use crate::properties::{CONTAINER_GAME_PORT, CONTAINER_QUERY_PORT, CONTAINER_RCON_PORT};

/// Label that marks containers Lodestone owns, valued with the instance id.
pub const INSTANCE_LABEL: &str = "lodestone.instance";

/// Servers run mods and plugins anyone with an account can upload, so their
/// containers get no capabilities, can't gain privileges, and can't fork
/// without bound. Java needs none of what's dropped: it runs as the data
/// dir's owner and listens on ports above 1024.
const PIDS_LIMIT: i64 = 4096;
const ICC_OPTION: &str = "com.docker.network.bridge.enable_icc";
const BRIDGE_NAME_OPTION: &str = "com.docker.network.bridge.name";

/// Where server containers connect.
#[derive(Debug, Clone, Default)]
pub struct DockerOptions {
    /// A dedicated bridge network, created when missing with traffic between
    /// containers off. Its Linux bridge has the same name, so the host
    /// firewall can match it (`docs/deploy.md`). `None`: Docker's default.
    pub network: Option<String>,
    /// DNS servers for the containers. `[]`: Docker's default.
    pub dns: Vec<String>,
}

/// A network name that's also a valid Linux interface name (the bridge).
pub fn check_network_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= 15
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "`{name}`: the Docker network's name is also its bridge's, so use 1-15 letters, digits, '-', '_' or '.'"
        ))
    }
}

pub struct DockerRuntime {
    docker: Option<Docker>,
    connect_error: Option<String>,
    /// Rewrites data-dir paths to host paths when Lodestone itself runs in
    /// a container: `(data_dir, host_data_dir)`.
    path_map: Option<(PathBuf, PathBuf)>,
    /// `uid:gid` that owns the data dir, so server files stay editable.
    user: Option<String>,
    options: DockerOptions,
}

impl DockerRuntime {
    pub fn connect(data_dir: &Path, host_data_dir: Option<PathBuf>, options: DockerOptions) -> Self {
        let (docker, connect_error) = match connect() {
            Ok(docker) => (Some(docker), None),
            Err(err) => (None, Some(err)),
        };
        Self {
            docker,
            connect_error,
            path_map: host_data_dir.map(|host| (data_dir.to_path_buf(), host)),
            user: owner_of(data_dir),
            options,
        }
    }

    /// Makes sure the servers' network exists and isolates containers from
    /// each other. One Lodestone didn't create is accepted only if it does.
    async fn ensure_network(&self, docker: &Docker) -> AppResult<()> {
        let Some(name) = &self.options.network else { return Ok(()) };
        let inspect = || docker.inspect_network(name, None::<InspectNetworkOptions<String>>);
        let network = match inspect().await {
            Ok(network) => network,
            Err(DockerError::DockerResponseServerError { status_code: 404, .. }) => {
                let options = CreateNetworkOptions {
                    name: name.as_str(),
                    driver: "bridge",
                    options: HashMap::from([(ICC_OPTION, "false"), (BRIDGE_NAME_OPTION, name.as_str())]),
                    labels: HashMap::from([("lodestone", "servers")]),
                    ..Default::default()
                };
                match docker.create_network(options).await {
                    Ok(_) => tracing::info!(network = %name, "created the servers' Docker network"),
                    // Another start created it first.
                    Err(DockerError::DockerResponseServerError { status_code: 409, .. }) => {}
                    Err(err) => return Err(docker_err(&format!("creating the Docker network {name}"), err)),
                }
                inspect().await.map_err(|err| docker_err(&format!("inspecting the Docker network {name}"), err))?
            }
            Err(err) => return Err(docker_err(&format!("inspecting the Docker network {name}"), err)),
        };
        let options = network.options.unwrap_or_default();
        let isolated = network.driver.as_deref() == Some("bridge")
            && options.get(ICC_OPTION).map(String::as_str) == Some("false")
            && options.get(BRIDGE_NAME_OPTION) == Some(name);
        if !isolated {
            return Err(AppError::conflict(
                "network_unsafe",
                format!(
                    "The Docker network \"{name}\" isn't one Lodestone can use: servers need a bridge network named \
                     \"{name}\" with traffic between containers off. Remove it (docker network rm {name}) and \
                     Lodestone will create it."
                ),
            ));
        }
        Ok(())
    }

    /// What every container Lodestone runs shares: the network, DNS and the
    /// hardening above.
    fn host_config(&self, binds: Vec<String>, memory_mb: u32) -> HostConfig {
        HostConfig {
            binds: Some(binds),
            memory: Some(memory_mb as i64 * 1024 * 1024),
            init: Some(true),
            network_mode: self.options.network.clone(),
            dns: (!self.options.dns.is_empty()).then(|| self.options.dns.clone()),
            cap_drop: Some(vec!["ALL".into()]),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            pids_limit: Some(PIDS_LIMIT),
            ..Default::default()
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
        self.ensure_network(&docker).await?;
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
                port_bindings: Some(ports),
                nano_cpus: spec.cpu_cores.map(|cores| (cores * 1e9) as i64),
                ..self.host_config(binds, spec.memory_limit_mb)
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

    async fn run_task(&self, task: &TaskSpec, job: &JobHandle) -> AppResult<TaskOutput> {
        let docker = self.docker()?.clone();
        // Installers run third-party code and download libraries: the same
        // network and limits as servers.
        self.ensure_network(&docker).await?;
        let name = format!("lodestone-task-{}-{}", task.name, crate::util::secret(6).to_lowercase());
        let binds = task
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
        let config = Config {
            image: Some(task.image.clone()),
            cmd: Some(task.command.clone()),
            working_dir: Some(task.workdir.clone()),
            env: Some(vec![format!("HOME={}", task.workdir)]),
            user: self.user.clone(),
            tty: Some(false),
            host_config: Some(self.host_config(binds, task.memory_limit_mb)),
            ..Default::default()
        };
        let created = docker
            .create_container(Some(CreateContainerOptions { name: name.clone(), platform: None }), config)
            .await
            .map_err(|err| docker_err("creating the task container", err))?;
        let result = async {
            docker
                .start_container(&created.id, None::<StartContainerOptions<String>>)
                .await
                .map_err(|err| docker_err("starting the task container", err))?;
            let mut events = self.follow(docker.clone(), created.id.clone(), "all");
            let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
            let mut code = -1;
            while let Some(event) = events.recv().await {
                match event {
                    ProcessEvent::Output(line) => {
                        let line = line.trim().to_owned();
                        if line.is_empty() {
                            continue;
                        }
                        job.progress(None, line.chars().take(120).collect::<String>()).await;
                        if tail.len() == 20 {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                    ProcessEvent::Exited { code: exit } => {
                        code = exit;
                        break;
                    }
                }
            }
            Ok(TaskOutput { code, tail: tail.into_iter().collect() })
        }
        .await;
        let _ = docker
            .remove_container(&created.id, Some(RemoveContainerOptions { force: true, ..Default::default() }))
            .await;
        result
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
            options: DockerOptions::default(),
        };
        assert_eq!(runtime.host_path(Path::new("/data/worlds/w1")), "/srv/lodestone/worlds/w1");
        assert_eq!(runtime.host_path(Path::new("/elsewhere/x")), "/elsewhere/x");
    }

    #[test]
    fn unreachable_docker_explains_itself() {
        let runtime = DockerRuntime {
            docker: None,
            connect_error: Some("no socket".into()),
            path_map: None,
            user: None,
            options: DockerOptions::default(),
        };
        let err = runtime.docker().unwrap_err();
        assert!(err.to_string().contains("Start Docker"));
    }

    #[test]
    fn containers_are_hardened_and_isolated() {
        let runtime = DockerRuntime {
            docker: None,
            connect_error: None,
            path_map: None,
            user: None,
            options: DockerOptions { network: Some("lodestone".into()), dns: vec!["1.1.1.1".into()] },
        };
        let config = runtime.host_config(vec![], 1024);
        assert_eq!(config.network_mode.as_deref(), Some("lodestone"));
        assert_eq!(config.dns, Some(vec!["1.1.1.1".to_owned()]));
        assert_eq!(config.cap_drop, Some(vec!["ALL".to_owned()]));
        assert_eq!(config.security_opt, Some(vec!["no-new-privileges:true".to_owned()]));
        assert_eq!(config.pids_limit, Some(PIDS_LIMIT));
        assert_eq!(config.memory, Some(1024 * 1024 * 1024));

        let default = DockerRuntime { options: DockerOptions::default(), ..runtime };
        let config = default.host_config(vec![], 1024);
        assert_eq!((config.network_mode, config.dns), (None, None));
        assert_eq!(config.cap_drop, Some(vec!["ALL".to_owned()]));
    }

    #[test]
    fn network_names_are_bridge_names() {
        assert!(check_network_name("lodestone").is_ok());
        assert!(check_network_name("ls-servers_1.0").is_ok());
        assert!(check_network_name("").is_err());
        assert!(check_network_name("a-name-that-is-too-long").is_err());
        assert!(check_network_name("bad name").is_err());
    }
}
