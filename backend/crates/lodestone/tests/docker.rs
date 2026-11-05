//! Real servers in real containers: Docker plus downloads from Mojang,
//! PaperMC and Fabric. Slow (first run pulls JRE images and jars), so it
//! only builds with `--features docker-tests`:
//!
//! ```text
//! just test-docker
//! ```
//!
//! The download cache is kept in `backend/target/docker-tests/cache` so
//! reruns skip the downloads.
#![cfg(feature = "docker-tests")]

use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use lodestone::{build_state, router, supervisor, AppState, Config, ProviderKind, RuntimeKind};
use serde_json::{json, Value};
use tower::ServiceExt;

const BOOT_TIMEOUT: Duration = Duration::from_secs(600);
/// Every test runs servers the way a public host does: on their own network,
/// with public DNS, so downloads, Mojang and installers are proven to work
/// from there.
const NETWORK: &str = "lodestone-test";

fn free_port() -> u16 {
    TcpListener::bind("0.0.0.0:0").unwrap().local_addr().unwrap().port()
}

/// A data dir per test that shares one download cache across runs.
fn data_dir(name: &str) -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/docker-tests");
    let dir = base.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("cache")).unwrap();
    let shared = base.join("cache");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::remove_dir_all(dir.join("cache")).unwrap();
    std::os::unix::fs::symlink(std::fs::canonicalize(&shared).unwrap(), dir.join("cache")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn config(dir: PathBuf) -> Config {
    Config {
        runtime: RuntimeKind::Docker,
        providers: ProviderKind::Real,
        stop_timeout: Duration::from_secs(60),
        docker_network: Some(NETWORK.into()),
        container_dns: vec!["1.1.1.1".into(), "9.9.9.9".into()],
        ..Config::fake(dir)
    }
}

struct Lodestone {
    router: Router,
    state: AppState,
}

impl Lodestone {
    async fn boot(dir: PathBuf) -> Self {
        let state = build_state(config(dir)).await.unwrap();
        supervisor::reconcile(&state).await.unwrap();
        Self { router: router(state.clone()), state }
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(format!("/api/v1{path}"));
        let body = match body {
            Some(body) => {
                request = request.header("content-type", "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self.router.clone().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.call(Method::GET, path, None).await;
        assert!(status.is_success(), "GET {path} -> {status}: {body}");
        body
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let (status, value) = self.call(Method::POST, path, Some(body)).await;
        assert!(status.is_success(), "POST {path} -> {status}: {value}");
        value
    }

    async fn wait_state(&self, id: &str, state: &str) -> Value {
        let deadline = std::time::Instant::now() + BOOT_TIMEOUT;
        let mut last_job = String::new();
        loop {
            let instance = self.get(&format!("/instances/{id}")).await;
            if instance["state"] == state {
                return instance;
            }
            if instance["state"] == "error" {
                let console = self.get(&format!("/instances/{id}/console?tail=40")).await;
                panic!("instance failed: {}\n{console:#}", instance["lastError"]);
            }
            let jobs = self.get("/jobs").await;
            let message = jobs[0]["message"].as_str().unwrap_or_default().to_owned();
            if message != last_job {
                eprintln!("  [{}] {message} {}", instance["state"], jobs[0]["progress"]);
                last_job = message;
            }
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {state}: {instance}");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn create(&self, software: &str, version: &str) -> Value {
        let (status, instance) = self
            .call(
                Method::POST,
                "/instances",
                Some(json!({
                    "name": format!("{software}-test"),
                    "newWorld": {
                        "name": format!("{software} world"), "levelType": "minecraft:flat", "seed": "lodestone",
                        "generateStructures": false, "difficulty": "peaceful", "gamemode": "creative",
                        "hardcore": false, "maxWorldSize": 1000, "allowNether": true
                    },
                    "newProfile": {
                        "name": format!("{software} {version}"), "description": "", "softwareId": software, "version": version,
                        "memory": { "minMb": 512, "maxMb": 1536, "initialMb": 512 }, "cpuCores": null, "javaArgs": "",
                        "autoStart": false, "restartOnCrash": false, "crashThreshold": 1
                    },
                    "minecraftPort": free_port(),
                    "rconPort": free_port(),
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{instance}");
        instance
    }
}

async fn run_server(software: &str, version: &str, nether_folder: &str) {
    run_server_with(software, software, version, nether_folder, None).await;
}

/// `plugin`: (source, project slug, the name it logs as) to install first.
async fn run_server_with(
    name: &str,
    software: &str,
    version: &str,
    nether_folder: &str,
    plugin: Option<(&str, &str, &str)>,
) {
    let dir = data_dir(name);
    let app = Lodestone::boot(dir.clone()).await;
    assert_eq!(app.get("/system").await["runtime"]["available"], true, "Docker must be running");

    let instance = app.create(software, version).await;
    let id = instance["id"].as_str().unwrap().to_owned();
    let world_id = instance["worldId"].as_str().unwrap().to_owned();
    let profile_id = instance["profileId"].as_str().unwrap().to_owned();
    if let Some((source, slug, _)) = plugin {
        // A real install plan and download from the source.
        let plan = app
            .post(
                &format!("/profiles/{profile_id}/content/plan"),
                json!({ "items": [{ "provider": source, "projectId": slug }] }),
            )
            .await;
        let items: Vec<Value> = plan["install"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| json!({ "provider": i["provider"], "projectId": i["projectId"], "versionId": i["versionId"] }))
            .collect();
        assert!(!items.is_empty(), "{plan}");
        app.post(&format!("/profiles/{profile_id}/content"), json!({ "items": items })).await;
    }
    app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    app.wait_state(&id, "running").await;
    if let Some((_, _, logs_as)) = plugin {
        let console = app.get(&format!("/instances/{id}/console?tail=2000")).await;
        let enabled = console
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line["message"].as_str().is_some_and(|m| m.contains(&format!("Enabling {logs_as}"))));
        assert!(enabled, "{logs_as} didn't load");
    }

    // RCON works end to end.
    let lines = app.post(&format!("/instances/{id}/console"), json!({ "command": "list" })).await;
    assert!(lines[1]["message"].as_str().unwrap().contains("players online"), "{lines}");

    // Metrics come from the container.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let status = app.get(&format!("/instances/{id}/status")).await;
    assert!(status["memory"]["usedBytes"].as_u64().unwrap() > 0, "{status}");

    // A consistent backup while the server runs.
    let backup = app.post(&format!("/worlds/{world_id}/backups"), json!({})).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        let index = app.get(&format!("/worlds/{world_id}/backups")).await;
        let entry = index["backups"].as_array().unwrap().iter().find(|b| b["id"] == backup["id"]).cloned().unwrap();
        if entry["status"] == "complete" {
            break;
        }
        assert_ne!(entry["status"], "failed", "{entry}");
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    app.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    app.wait_state(&id, "stopped").await;

    // The world is on disk, outside any container, in the right layout.
    let universe = dir.join("worlds").join(&world_id);
    let folder = app.get(&format!("/worlds/{world_id}")).await["folder"].as_str().unwrap().to_owned();
    assert!(universe.join(&folder).join("level.dat").is_file(), "level.dat missing in {universe:?}");
    assert!(universe.join(nether_folder.replace("{folder}", &folder)).is_dir(), "nether folder missing");
    // Lodestone read which version saved it.
    assert_eq!(app.get(&format!("/worlds/{world_id}")).await["savedWith"], version);

    // Deleting the instance keeps the world.
    app.call(Method::DELETE, &format!("/instances/{id}"), None).await;
    assert!(universe.join(&folder).join("level.dat").is_file());
}

#[tokio::test]
async fn vanilla_server_runs_in_docker() {
    run_server("vanilla", "1.21.4", "{folder}/DIM-1").await;
}

#[tokio::test]
async fn paper_server_runs_in_docker() {
    run_server_with("paper", "paper", "1.21.4", "{folder}_nether", Some(("modrinth", "chunky", "Chunky"))).await;
}

#[tokio::test]
async fn paper_runs_a_hangar_plugin_in_docker() {
    run_server_with("paper-hangar", "paper", "1.21.4", "{folder}_nether", Some(("hangar", "Chunky", "Chunky"))).await;
}

#[tokio::test]
async fn neoforge_server_runs_in_docker() {
    run_server("neoforge", "1.21.1", "{folder}/DIM-1").await;
}

#[tokio::test]
async fn fabric_server_runs_in_docker() {
    run_server("fabric", "1.21.4", "{folder}/DIM-1").await;
}

/// Lodestone restarts while a server keeps running in its container.
#[tokio::test]
async fn reattaches_after_a_lodestone_restart() {
    let dir = data_dir("reattach");
    let first = Lodestone::boot(dir.clone()).await;
    let instance = first.create("vanilla", "1.21.4").await;
    let id = instance["id"].as_str().unwrap().to_owned();
    first.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    first.wait_state(&id, "running").await;
    drop(first);

    let second = Lodestone::boot(dir).await;
    second.wait_state(&id, "running").await;
    let lines = second.post(&format!("/instances/{id}/console"), json!({ "command": "list" })).await;
    assert!(lines[1]["message"].as_str().unwrap().contains("players online"), "{lines}");
    second.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    second.wait_state(&id, "stopped").await;
    let _ = &second.state;
}

fn docker(args: &[&str]) -> (bool, String) {
    let output = std::process::Command::new("docker").args(args).output().expect("the docker CLI");
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success(), text)
}

/// The container hardening and the network's isolation, on a real server.
#[tokio::test]
async fn servers_are_hardened_and_isolated() {
    let app = Lodestone::boot(data_dir("isolated")).await;
    let instance = app.create("vanilla", "1.21.4").await;
    let id = instance["id"].as_str().unwrap().to_owned();
    app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    app.wait_state(&id, "running").await;

    let (_, container) = docker(&["ps", "-q", "--filter", &format!("label=lodestone.instance={id}")]);
    assert!(!container.is_empty(), "no container for {id}");
    let inspect = |format: &str| docker(&["inspect", "-f", format, &container]).1;
    assert_eq!(inspect("{{.HostConfig.NetworkMode}}"), NETWORK);
    assert_eq!(inspect("{{json .HostConfig.CapDrop}}"), r#"["ALL"]"#);
    assert!(inspect("{{json .HostConfig.SecurityOpt}}").contains("no-new-privileges"));
    assert_eq!(inspect("{{.HostConfig.PidsLimit}}"), "4096");
    assert_eq!(
        docker(&["network", "inspect", "-f", "{{index .Options \"com.docker.network.bridge.enable_icc\"}}", NETWORK]).1,
        "false"
    );

    // Another container on the same network can't reach the server...
    let server_ip = inspect(&format!("{{{{(index .NetworkSettings.Networks \"{NETWORK}\").IPAddress}}}}"));
    assert!(!server_ip.is_empty());
    let image = inspect("{{.Config.Image}}");
    let probe = |target: &str| {
        let script = format!("timeout 5 bash -c '</dev/tcp/{target}' && echo open || echo closed");
        docker(&["run", "--rm", "--network", NETWORK, "--cap-drop", "ALL", &image, "bash", "-c", &script]).1
    };
    assert_eq!(probe(&format!("{server_ip}/25565")), "closed");
    // ...while the internet still works from there.
    assert_eq!(probe("1.1.1.1/443"), "open");

    // The game port is still published to the host.
    let port = instance["ports"]["minecraft"].as_u64().unwrap() as u16;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(), "game port {port} not reachable");

    app.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    app.wait_state(&id, "stopped").await;
}
