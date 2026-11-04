//! End-to-end API tests: the real router, a real SQLite database and the
//! fake runtime (which serves real RCON), with no Docker or network.

use std::net::TcpListener;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use lodestone::{build_state, router, AppState, Config};
use serde_json::{json, Value};
use tower::ServiceExt;

struct TestApp {
    router: Router,
    state: AppState,
    _dir: tempfile::TempDir,
}

/// A port nothing listens on right now, for the fake server's RCON socket.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

impl TestApp {
    async fn new() -> Self {
        Self::with(|_| {}).await
    }

    async fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::fake(dir.path());
        config.stop_timeout = Duration::from_secs(2);
        tweak(&mut config);
        let state = build_state(config).await.unwrap();
        Self { router: router(state.clone()), state, _dir: dir }
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
        let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
        (status, value)
    }

    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.call(Method::GET, path, None).await;
        assert!(status.is_success(), "GET {path} -> {status}: {body}");
        body
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, Some(body)).await
    }

    async fn create_instance(&self, extra: Value) -> Value {
        let mut body = json!({
            "name": "survival-1",
            "newWorld": world_spec("Survival"),
            "newProfile": profile_spec("paper", "1.21.4"),
            "rconPort": free_port(),
        });
        merge(&mut body, extra);
        let (status, instance) = self.post("/instances", body).await;
        assert_eq!(status, StatusCode::CREATED, "{instance}");
        instance
    }

    /// Polls an instance until `state`, failing after a few seconds.
    async fn wait_state(&self, id: &str, state: &str) -> Value {
        for _ in 0..200 {
            let instance = self.get(&format!("/instances/{id}")).await;
            if instance["state"] == state {
                return instance;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("instance {id} never reached {state}: {}", self.get(&format!("/instances/{id}")).await);
    }

    async fn wait_until(&self, what: &str, mut check: impl FnMut(&Value) -> bool, path: &str) -> Value {
        for _ in 0..200 {
            let value = self.get(path).await;
            if check(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for {what}: {}", self.get(path).await);
    }
}

fn merge(base: &mut Value, extra: Value) {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        for (key, value) in extra {
            base.insert(key, value);
        }
    }
}

fn world_spec(name: &str) -> Value {
    json!({
        "name": name, "levelType": "minecraft:normal", "seed": null, "generateStructures": true,
        "difficulty": "normal", "gamemode": "survival", "hardcore": false, "maxWorldSize": 29999984, "allowNether": true
    })
}

fn profile_spec(software: &str, version: &str) -> Value {
    json!({
        "name": format!("{software} {version}"), "description": "", "softwareId": software, "version": version,
        "memory": { "minMb": 1024, "maxMb": 2048, "initialMb": 1024 }, "cpuCores": null, "javaArgs": "-XX:+UseG1GC",
        "autoStart": false, "restartOnCrash": false, "crashThreshold": 3
    })
}

#[tokio::test]
async fn health_and_unknown_routes() {
    let app = TestApp::new().await;
    assert_eq!(app.get("/health").await["status"], "ok");
    let (status, body) = app.call(Method::GET, "/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn software_catalog_reports_java() {
    let app = TestApp::new().await;
    let catalog = app.get("/profiles/software").await;
    let paper = catalog["software"].as_array().unwrap().iter().find(|s| s["id"] == "paper").unwrap();
    assert_eq!(paper["kind"], "plugins");
    assert_eq!(paper["versions"][0]["javaMajor"], 21);
}

#[tokio::test]
async fn world_and_profile_crud() {
    let app = TestApp::new().await;
    let (status, world) = app.post("/worlds", world_spec("Flat Test")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(world["folder"], "flat-test");
    assert_eq!(world["attachedInstanceId"], Value::Null);

    let (status, patched) = app
        .call(
            Method::PATCH,
            &format!("/worlds/{}", world["id"].as_str().unwrap()),
            Some(json!({ "difficulty": "hard", "pvp": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["difficulty"], "hard");
    assert_eq!(patched["pvp"], false);

    let (status, profile) = app.post("/profiles", profile_spec("fabric", "1.21.1")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(profile["image"], "eclipse-temurin:21-jre");
    assert_eq!(profile["instanceCount"], 0);

    let (status, err) = app.post("/profiles", profile_spec("forge", "1.99")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"]["message"].as_str().unwrap().contains("does not publish"));

    let (status, copy) = app.post(&format!("/worlds/{}/duplicate", world["id"].as_str().unwrap()), json!({})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copy["name"], "Flat Test (copy)");
    assert_eq!(app.get("/worlds").await.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn composite_create_is_atomic() {
    let app = TestApp::new().await;
    // A bad profile must not leave the new world behind.
    let (status, _) = app
        .post(
            "/instances",
            json!({ "name": "x", "newWorld": world_spec("Orphan?"), "newProfile": profile_spec("paper", "0.0") }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(app.get("/worlds").await.as_array().unwrap().is_empty());

    let instance = app.create_instance(json!({})).await;
    assert_eq!(instance["state"], "stopped");
    assert_eq!(instance["profile"]["software"], "Paper");
    assert_eq!(instance["world"]["folder"], "survival");
    assert!(instance["ports"]["minecraft"].as_u64().unwrap() >= 25565);

    let worlds = app.get("/worlds").await;
    assert_eq!(worlds[0]["attachedInstanceId"], instance["id"]);
    assert_eq!(app.get("/profiles").await[0]["instanceCount"], 1);
}

#[tokio::test]
async fn a_world_attaches_to_one_instance_and_guards_deletion() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({})).await;
    let world_id = instance["worldId"].as_str().unwrap();
    let profile_id = instance["profileId"].as_str().unwrap();

    let (status, err) = app
        .post(
            "/instances",
            json!({ "name": "second", "worldId": world_id, "profileId": profile_id, "rconPort": free_port() }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "world_attached");

    let (status, err) = app.call(Method::DELETE, &format!("/worlds/{world_id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    let (status, err) = app.call(Method::DELETE, &format!("/profiles/{profile_id}"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "profile_in_use");

    // Deleting the instance keeps both assets, and frees them.
    let (status, _) = app.call(Method::DELETE, &format!("/instances/{}", instance["id"].as_str().unwrap()), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(app.get(&format!("/worlds/{world_id}")).await["attachedInstanceId"], Value::Null);
    assert_eq!(app.get(&format!("/profiles/{profile_id}")).await["instanceCount"], 0);
}

#[tokio::test]
async fn explicit_port_conflicts_are_reported() {
    let app = TestApp::new().await;
    let first = app.create_instance(json!({ "minecraftPort": 30001 })).await;
    assert_eq!(first["ports"]["minecraft"], 30001);
    let (status, err) = app
        .post("/instances", json!({ "name": "b", "newWorld": world_spec("B"), "newProfile": profile_spec("vanilla", "1.21.4"), "minecraftPort": 30001 }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "port_in_use");
}

#[tokio::test]
async fn starting_requires_the_eula() {
    let app = TestApp::with(|config| config.accept_eula = false).await;
    let instance = app.create_instance(json!({})).await;
    let id = instance["id"].as_str().unwrap();
    let (status, err) = app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "eula_required");

    // Acceptance is per user: their own servers may start.
    let me = app.get("/me").await;
    assert_eq!(me["eulaAccepted"], false);
    let (status, me) = app.call(Method::PATCH, "/me", Some(json!({ "eulaAccepted": true }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["eulaAccepted"], true);
    let (status, _) = app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    assert_eq!(status, StatusCode::OK);
    app.wait_state(id, "running").await;
}

#[tokio::test]
async fn lifecycle_console_and_players() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "start": true })).await;
    let id = instance["id"].as_str().unwrap().to_owned();
    assert!(matches!(instance["state"].as_str(), Some("preparing" | "starting" | "running")));
    app.wait_state(&id, "running").await;

    // The run dir was written for a real server.
    let run_dir = app.state.paths.instance(&id);
    let props = std::fs::read_to_string(run_dir.join("server.properties")).unwrap();
    assert!(props.contains("level-name=survival"));
    assert!(props.contains("enable-rcon=true"));
    assert!(std::fs::read_to_string(run_dir.join("eula.txt")).unwrap().contains("eula=true"));

    // A prepare job ran and succeeded.
    let jobs = app.get("/jobs").await;
    assert!(jobs.as_array().unwrap().iter().any(|job| job["kind"] == "prepare" && job["state"] == "succeeded"));

    // Console commands go over real RCON to the simulated server.
    let (status, lines) = app.post(&format!("/instances/{id}/console"), json!({ "command": "/list" })).await;
    assert_eq!(status, StatusCode::OK, "{lines}");
    assert_eq!(lines[0]["message"], "> list");
    assert!(lines[1]["message"].as_str().unwrap().contains("0 of a max"));

    // A player joins: tracked as online and remembered by the world.
    app.post(&format!("/instances/{id}/console"), json!({ "command": "lodestone:join Steve" })).await;
    app.wait_until("Steve online", |instance| instance["players"]["online"] == 1, &format!("/instances/{id}")).await;
    let players = app.get(&format!("/instances/{id}/players")).await;
    let steve = &players["players"][0];
    assert_eq!(steve["name"], "Steve");
    assert_eq!(steve["online"], true);
    assert!(!steve["uuid"].as_str().unwrap().is_empty());

    // Op is stored on the profile and applied live.
    let (status, players) =
        app.post(&format!("/instances/{id}/players"), json!({ "action": "op", "name": "Steve" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(players["players"][0]["op"], true);
    let console = app.get(&format!("/instances/{id}/console?tail=50")).await;
    assert!(console.as_array().unwrap().iter().any(|line| line["message"] == "Made Steve a server operator"));

    // Status reports metrics from the runtime.
    let status = app.get(&format!("/instances/{id}/status")).await;
    assert_eq!(status["state"], "running");
    assert_eq!(status["players"]["online"], 1);

    // Stop: the player's session ends and the world remembers them.
    app.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    app.wait_state(&id, "stopped").await;
    let world = app.get(&format!("/worlds/{}/status", instance["worldId"].as_str().unwrap())).await;
    assert_eq!(world["playerCount"], 1);
    let world = app.get(&format!("/worlds/{}", instance["worldId"].as_str().unwrap())).await;
    assert_eq!(world["generated"], true);
    assert_eq!(world["layout"], "bukkit");
    assert!(world["sizeBytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn commands_need_a_running_server() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({})).await;
    let (status, err) = app
        .post(&format!("/instances/{}/console", instance["id"].as_str().unwrap()), json!({ "command": "list" }))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "not_running");
}

#[tokio::test]
async fn crashes_restart_until_the_threshold() {
    let app = TestApp::new().await;
    let mut profile = profile_spec("vanilla", "1.21.4");
    profile["restartOnCrash"] = json!(true);
    profile["crashThreshold"] = json!(1);
    let instance = app.create_instance(json!({ "newProfile": profile, "start": true })).await;
    let id = instance["id"].as_str().unwrap().to_owned();
    app.wait_state(&id, "running").await;

    // First crash: restarted automatically.
    app.post(&format!("/instances/{id}/console"), json!({ "command": "lodestone:crash" })).await;
    app.wait_until(
        "a restart",
        |console| {
            console
                .as_array()
                .unwrap()
                .iter()
                .any(|line| line["message"].as_str().unwrap().contains("Restarting (1 of 1"))
        },
        &format!("/instances/{id}/console?tail=200"),
    )
    .await;
    app.wait_state(&id, "running").await;

    // Second crash inside the window: gives up with an explanation.
    app.post(&format!("/instances/{id}/console"), json!({ "command": "lodestone:crash" })).await;
    let failed = app.wait_state(&id, "error").await;
    let error = failed["lastError"].as_str().unwrap();
    assert!(error.contains("stopped restarting"), "{error}");
    assert!(error.contains("simulated crash"), "{error}");
}

#[tokio::test]
async fn world_generation_fields_lock_after_first_start() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "newProfile": profile_spec("vanilla", "1.21.4"), "start": true })).await;
    let id = instance["id"].as_str().unwrap();
    app.wait_state(id, "running").await;
    let world_id = instance["worldId"].as_str().unwrap();
    let (status, err) = app.call(Method::PATCH, &format!("/worlds/{world_id}"), Some(json!({ "seed": "123" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "world_generated");
    let (status, _) =
        app.call(Method::PATCH, &format!("/worlds/{world_id}"), Some(json!({ "difficulty": "peaceful" }))).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn paper_worlds_refuse_vanilla_profiles() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "start": true })).await; // Paper
    let id = instance["id"].as_str().unwrap();
    app.wait_state(id, "running").await;
    app.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    app.wait_state(id, "stopped").await;
    app.call(Method::DELETE, &format!("/instances/{id}"), None).await;

    let (status, err) = app
        .post(
            "/instances",
            json!({ "name": "v", "worldId": instance["worldId"], "newProfile": profile_spec("vanilla", "1.21.4"), "rconPort": free_port() }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "layout_mismatch");
}

#[tokio::test]
async fn switching_worlds_restarts_a_running_instance() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "start": true })).await;
    let id = instance["id"].as_str().unwrap();
    app.wait_state(id, "running").await;
    let (_, other) = app.post("/worlds", world_spec("Second")).await;

    let (status, switched) = app.post(&format!("/instances/{id}/world"), json!({ "worldId": other["id"] })).await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["world"]["name"], "Second");
    app.wait_state(id, "running").await;
    assert_eq!(
        app.get(&format!("/worlds/{}", instance["worldId"].as_str().unwrap())).await["attachedInstanceId"],
        Value::Null
    );
    assert_eq!(
        app.get(&format!("/worlds/{}", other["id"].as_str().unwrap())).await["attachedInstanceId"],
        instance["id"]
    );
}

#[tokio::test]
async fn backups_snapshot_and_restore_a_world() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "start": true })).await;
    let id = instance["id"].as_str().unwrap();
    let world_id = instance["worldId"].as_str().unwrap();
    app.wait_state(id, "running").await;

    let (status, backup) = app.post(&format!("/worlds/{world_id}/backups"), json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(backup["status"], "running");
    let index = app
        .wait_until(
            "backup",
            |index| index["backups"][0]["status"] == "complete",
            &format!("/worlds/{world_id}/backups"),
        )
        .await;
    assert!(index["backups"][0]["sizeBytes"].as_u64().unwrap() > 0);
    // Autosave was paused and resumed around the archive.
    let console = app.get(&format!("/instances/{id}/console?tail=200")).await;
    let _ = console;

    // Damage the world, then restore: the server is restarted around the swap.
    let level = app.state.paths.world(world_id).join("survival");
    std::fs::write(level.join("level.dat"), b"griefed").unwrap();
    let (status, _) =
        app.post(&format!("/worlds/{world_id}/backups/{}/restore", backup["id"].as_str().unwrap()), json!({})).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    app.wait_until(
        "restore job",
        |jobs| jobs.as_array().unwrap().iter().any(|job| job["kind"] == "restore" && job["state"] == "succeeded"),
        "/jobs",
    )
    .await;
    // The fake server rewrites level.dat on start, so check the restore happened
    // through the job and that the world is running again.
    app.wait_state(id, "running").await;
    assert_ne!(std::fs::read(level.join("level.dat")).unwrap(), b"griefed");

    let (status, _) =
        app.call(Method::DELETE, &format!("/worlds/{world_id}/backups/{}", backup["id"].as_str().unwrap()), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(app.get("/backups").await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn backup_schedule_validates_and_runs() {
    let app = TestApp::new().await;
    let (_, world) = app.post("/worlds", world_spec("Scheduled")).await;
    let world_id = world["id"].as_str().unwrap();
    let (status, _) = app
        .call(Method::PATCH, &format!("/worlds/{world_id}/backups/schedule"), Some(json!({ "intervalMinutes": 1 })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, index) = app
        .call(
            Method::PATCH,
            &format!("/worlds/{world_id}/backups/schedule"),
            Some(json!({ "enabled": true, "intervalMinutes": 30, "retention": 2 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(index["schedule"]["enabled"], true);
    assert!(index["schedule"]["nextRunAt"].is_string());
}

#[tokio::test]
async fn profile_properties_respect_ownership() {
    let app = TestApp::new().await;
    let (_, profile) = app.post("/profiles", profile_spec("paper", "1.21.4")).await;
    let path = format!("/profiles/{}/properties", profile["id"].as_str().unwrap());
    let file = app.get(&path).await;
    assert!(file["fields"].as_array().unwrap().iter().all(|field| field["scope"] == "profile"));

    let (status, saved) =
        app.call(Method::PUT, &path, Some(json!({ "changes": { "motd": "Welcome!", "max-players": "50" } }))).await;
    assert_eq!(status, StatusCode::OK);
    let motd = saved["fields"].as_array().unwrap().iter().find(|field| field["key"] == "motd").unwrap();
    assert_eq!(motd["value"], "Welcome!");

    let (status, err) = app.call(Method::PUT, &path, Some(json!({ "changes": { "difficulty": "hard" } }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"]["message"].as_str().unwrap().contains("belongs to the world"));
}

#[tokio::test]
async fn settings_and_system() {
    let app = TestApp::new().await;
    let (status, settings) =
        app.call(Method::PATCH, "/settings", Some(json!({ "publicAddress": "https://play.example.com/" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["publicAddress"], "play.example.com");
    let instance = app.create_instance(json!({ "minecraftPort": 25570 })).await;
    assert_eq!(instance["address"], "play.example.com:25570");

    let system = app.get("/system").await;
    assert_eq!(system["runtime"]["kind"], "fake");
    assert_eq!(system["providers"], "fake");
}

#[tokio::test]
async fn events_announce_changes() {
    let app = TestApp::new().await;
    let mut events = app.state.events.subscribe();
    app.post("/worlds", world_spec("Evented")).await;
    let event = tokio::time::timeout(Duration::from_secs(1), events.recv()).await.unwrap().unwrap();
    let event = serde_json::to_value(event).unwrap();
    assert_eq!(event["type"], "changed");
    assert_eq!(event["resource"], "worlds");
}

/* ------------------------------ content ------------------------------ */

async fn paper_profile(app: &TestApp) -> String {
    let (status, profile) = app.post("/profiles", profile_spec("paper", "1.21.4")).await;
    assert_eq!(status, StatusCode::CREATED, "{profile}");
    profile["id"].as_str().unwrap().to_owned()
}

fn names(index: &Value) -> Vec<String> {
    let mut names: Vec<String> =
        index["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_owned()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn content_search_plan_and_install() {
    let app = TestApp::new().await;
    let id = paper_profile(&app).await;

    let index = app.get(&format!("/profiles/{id}/content")).await;
    assert_eq!(index["kind"], "plugins");
    assert_eq!(index["providers"][0]["id"], "modrinth");

    let results = app.get(&format!("/profiles/{id}/content/search?q=guard")).await;
    assert_eq!(results["hits"][0]["name"], "WorldGuard");
    assert_eq!(results["hits"][0]["kind"], "plugin");
    // Fabric-only mods never show up for Paper.
    let results = app.get(&format!("/profiles/{id}/content/search?q=lithium")).await;
    assert_eq!(results["total"], 0);

    let (status, plan) = app
        .post(
            &format!("/profiles/{id}/content/plan"),
            json!({ "items": [{ "provider": "modrinth", "projectId": "fk-worldguard" }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    let install = plan["install"].as_array().unwrap();
    assert_eq!(install.len(), 2, "WorldGuard plus WorldEdit: {plan}");
    assert_eq!(install[1]["requiredBy"], "WorldGuard");

    let items: Vec<Value> = install
        .iter()
        .map(|item| json!({ "provider": item["provider"], "projectId": item["projectId"], "versionId": item["versionId"] }))
        .collect();
    let (status, index) = app.post(&format!("/profiles/{id}/content"), json!({ "items": items })).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(names(&index), ["WorldEdit", "WorldGuard"]);
    let worldedit = index["entries"].as_array().unwrap().iter().find(|e| e["name"] == "WorldEdit").unwrap().clone();
    assert_eq!(worldedit["dependency"], true);
    assert_eq!(worldedit["requiredBy"], json!(["WorldGuard"]));
    assert_eq!(worldedit["compatible"], true);

    let results = app.get(&format!("/profiles/{id}/content/search?q=world")).await;
    assert!(results["hits"].as_array().unwrap().iter().all(|hit| hit["installed"] == true));

    // An install job ran.
    let jobs = app.get("/jobs").await;
    assert!(jobs.as_array().unwrap().iter().any(|job| job["kind"] == "install" && job["state"] == "succeeded"));
}

#[tokio::test]
async fn content_updates_toggles_and_removal() {
    let app = TestApp::new().await;
    let id = paper_profile(&app).await;
    let (status, index) = app
        .post(
            &format!("/profiles/{id}/content"),
            json!({ "items": [{ "provider": "modrinth", "projectId": "fk-worldedit", "versionId": "fk-worldedit-7.3.9" }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{index}");
    let entry = index["entries"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(index["entries"][0]["update"], Value::Null);

    let (_, index) = app.post(&format!("/profiles/{id}/content/check-updates"), json!({})).await;
    assert_eq!(index["entries"][0]["update"]["versionName"], "7.3.10");
    assert!(index["checkedAt"].is_string());

    let (status, index) = app.post(&format!("/profiles/{id}/content/{entry}/update"), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["entries"][0]["versionName"], "7.3.10");
    assert_eq!(index["entries"][0]["id"], entry, "updated in place");
    assert_eq!(index["entries"][0]["update"], Value::Null);

    let (status, index) =
        app.call(Method::PATCH, &format!("/profiles/{id}/content/{entry}"), Some(json!({ "enabled": false }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(index["entries"][0]["enabled"], false);

    let (status, index) = app.call(Method::DELETE, &format!("/profiles/{id}/content/{entry}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(index["entries"], json!([]));

    // Incompatible versions are refused outright.
    let (status, err) = app
        .post(
            &format!("/profiles/{id}/content"),
            json!({ "items": [{ "provider": "modrinth", "projectId": "fk-coreprotect", "versionId": "fk-coreprotect-22.4" }] }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "incompatible");
}

#[tokio::test]
async fn vanilla_profiles_have_no_content() {
    let app = TestApp::new().await;
    let (_, profile) = app.post("/profiles", profile_spec("vanilla", "1.21.4")).await;
    let id = profile["id"].as_str().unwrap();
    let index = app.get(&format!("/profiles/{id}/content")).await;
    assert_eq!(index["kind"], "vanilla");
    let (status, err) = app.call(Method::GET, &format!("/profiles/{id}/content/search?q=x"), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "content_unsupported");
}

async fn upload(app: &TestApp, path: &str, filename: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
    let boundary = "lodestone-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/java-archive\r\n\r\n"
    )
    .into_bytes();
    body.extend(bytes);
    body.extend(format!("\r\n--{boundary}--\r\n").into_bytes());
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1{path}"))
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }
    out.into_inner()
}

#[tokio::test]
async fn uploads_name_themselves_and_are_placed_on_start() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({})).await;
    let (id, profile) = (instance["id"].as_str().unwrap(), instance["profileId"].as_str().unwrap());

    let jar = zip(&[("plugin.yml", b"name: TreeFeller\nversion: 2.1\nauthor: Ana\nmain: a.B\n")]);
    let (status, index) = upload(&app, &format!("/profiles/{profile}/content/upload"), "TreeFeller.jar", jar).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    let entry = &index["entries"][0];
    assert_eq!((entry["name"].as_str(), entry["provider"].as_str()), (Some("TreeFeller"), Some("upload")));
    assert_eq!(entry["versionName"], "2.1");

    // A mod in a plugin configuration is refused.
    let fabric_mod = zip(&[("fabric.mod.json", br#"{"id":"x","name":"X","version":"1"}"#)]);
    let (status, err) = upload(&app, &format!("/profiles/{profile}/content/upload"), "x.jar", fabric_mod).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    // A stray jar that Lodestone didn't put there is cleared on start.
    let plugins = app.state.paths.profile(profile).join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("stray.jar"), b"x").unwrap();
    std::fs::create_dir_all(plugins.join("TreeFeller")).unwrap();

    app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    app.wait_state(id, "running").await;
    assert!(plugins.join("TreeFeller.jar").is_file());
    assert!(!plugins.join("stray.jar").exists());
    assert!(plugins.join("TreeFeller").is_dir(), "plugin folders are left alone");
}

#[tokio::test]
async fn modpacks_import_as_new_configurations() {
    let app = TestApp::new().await;
    // The fake provider recognises its own jars by hash.
    let lithium = b"lodestone fake content: fk-lithium-0.15.1\n";
    let sha512 = |bytes: &[u8]| {
        use sha2::Digest;
        hex::encode(sha2::Sha512::digest(bytes))
    };
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Cozy Pack", "summary": "Small",
        "dependencies": { "minecraft": "1.21.4", "fabric-loader": "0.16.10" },
        "files": [
            { "path": "mods/lithium.jar", "hashes": { "sha512": sha512(lithium) }, "env": { "server": "required" },
              "downloads": ["https://cdn.modrinth.com/data/x/lithium.jar"], "fileSize": lithium.len() },
            { "path": "mods/sodium.jar", "hashes": { "sha512": "00" }, "env": { "server": "unsupported" },
              "downloads": ["https://cdn.modrinth.com/data/y/sodium.jar"], "fileSize": 1 }
        ]
    })
    .to_string();
    let pack = zip(&[
        ("modrinth.index.json", index.as_bytes()),
        ("overrides/config/lithium.properties", b"mixin.ai=false"),
        ("overrides/mods/bundled.jar", b"jar bytes"),
    ]);
    let (status, job) = upload(&app, "/profiles/import", "cozy.mrpack", pack).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let profile_id = job["targetId"].as_str().unwrap().to_owned();
    for _ in 0..100 {
        let job = app.get(&format!("/jobs/{}", job["id"].as_str().unwrap())).await;
        if job["state"] != "running" {
            assert_eq!(job["state"], "succeeded", "{job}");
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let profile = app.get(&format!("/profiles/{profile_id}")).await;
    assert_eq!((profile["name"].as_str(), profile["softwareId"].as_str()), (Some("Cozy Pack 1.0"), Some("fabric")));
    let content = app.get(&format!("/profiles/{profile_id}/content")).await;
    assert_eq!(names(&content), ["Lithium", "bundled"]);
    let config = app.state.paths.profile(&profile_id).join("config/lithium.properties");
    assert_eq!(std::fs::read_to_string(config).unwrap(), "mixin.ai=false");
}

/* ------------------------------- worlds ------------------------------- */

#[tokio::test]
async fn worlds_remember_their_version_and_refuse_downgrades() {
    let app = TestApp::new().await;
    let instance = app.create_instance(json!({ "start": true })).await;
    let id = instance["id"].as_str().unwrap().to_owned();
    app.wait_state(&id, "running").await;
    app.post(&format!("/instances/{id}/actions"), json!({ "action": "stop" })).await;
    app.wait_state(&id, "stopped").await;

    let world = app.get(&format!("/worlds/{}", instance["worldId"].as_str().unwrap())).await;
    assert_eq!(world["savedWith"], "1.21.4");
    assert_eq!(world["dataVersion"], 4189);
    assert!(world["seed"].is_string(), "a random seed is filled in once known: {world}");

    // Offline, the day still comes from level.dat.
    let status = app.get(&format!("/worlds/{}/status", world["id"].as_str().unwrap())).await;
    assert_eq!(status["day"], 1);
    assert!(status["timeOfDay"].is_string());

    let profile = instance["profileId"].as_str().unwrap();
    let (status, _) =
        app.call(Method::PATCH, &format!("/profiles/{profile}"), Some(json!({ "version": "1.21.1" }))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, err) = app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    assert_eq!(err["error"]["code"], "world_newer");
}

#[tokio::test]
async fn worlds_import_and_download_as_zips() {
    let app = TestApp::new().await;
    let level = lodestone::level::write(&lodestone::level::LevelInfo {
        data_version: Some(3955),
        version_name: Some("1.21.1".into()),
        level_name: Some("Old Peaks".into()),
        seed: Some(42),
        day_time: Some(24_000 * 7),
        game_type: Some(1),
        difficulty: Some(3),
        hardcore: Some(false),
    });
    let world_zip = zip(&[("Old Peaks/level.dat", &level), ("Old Peaks/region/r.0.0.mca", &[1u8; 2048])]);
    let (status, world) = upload(&app, "/worlds/import", "Old Peaks.zip", world_zip).await;
    assert_eq!(status, StatusCode::CREATED, "{world}");
    assert_eq!(world["name"], "Old Peaks");
    assert_eq!(world["folder"], "old-peaks");
    assert_eq!((world["seed"].as_str(), world["gamemode"].as_str()), (Some("42"), Some("creative")));
    assert_eq!((world["generated"].as_bool(), world["savedWith"].as_str()), (Some(true), Some("1.21.1")));
    let id = world["id"].as_str().unwrap();
    assert!(app.state.paths.world(id).join("old-peaks/region/r.0.0.mca").is_file());

    let request = Request::builder().uri(format!("/api/v1/worlds/{id}/download")).body(Body::empty()).unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-disposition"], "attachment; filename=\"old-peaks.zip\"");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names: Vec<&str> = archive.file_names().collect();
    assert!(names.contains(&"old-peaks/level.dat"), "{names:?}");

    let (status, err) = upload(&app, "/worlds/import", "junk.zip", zip(&[("readme.txt", b"hi")])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
}

#[tokio::test]
async fn neoforge_runs_its_installer_once() {
    let app = TestApp::new().await;
    let instance =
        app.create_instance(json!({ "newProfile": profile_spec("neoforge", "1.21.1"), "start": true })).await;
    let id = instance["id"].as_str().unwrap().to_owned();
    app.wait_state(&id, "running").await;
    let install = app.state.paths.installs().join("neoforge-1.21.1-fake");
    assert!(install.join(".lodestone-installed").is_file());
    assert!(install.join("libraries/net/lodestone/fake-loader/1.0/unix_args.txt").is_file());

    // NeoForge configurations take mods.
    let profile = instance["profileId"].as_str().unwrap();
    let index = app.get(&format!("/profiles/{profile}/content")).await;
    assert_eq!(index["kind"], "mods");
    assert_eq!(index["loaders"], json!(["neoforge"]));
}

/* --------------------------- more sources (M7) --------------------------- */

fn source<'a>(index: &'a Value, id: &str) -> &'a Value {
    index["providers"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap()
}

#[tokio::test]
async fn hangar_serves_paper_and_curseforge_waits_for_a_key() {
    let app = TestApp::new().await;
    let paper = paper_profile(&app).await;
    let index = app.get(&format!("/profiles/{paper}/content")).await;
    assert_eq!(source(&index, "modrinth")["available"], true);
    assert_eq!(source(&index, "hangar")["available"], true);
    assert_eq!(source(&index, "hangar")["description"], "Paper plugins");
    let curseforge = source(&index, "curseforge");
    assert_eq!(curseforge["available"], false);
    assert!(curseforge["reason"].as_str().unwrap().contains("API key"));

    let (_, fabric) = app.post("/profiles", profile_spec("fabric", "1.21.4")).await;
    let fabric_index = app.get(&format!("/profiles/{}/content", fabric["id"].as_str().unwrap())).await;
    assert_eq!(source(&fabric_index, "hangar")["available"], false);

    // Hangar search and install.
    let results = app.get(&format!("/profiles/{paper}/content/search?provider=hangar&q=guard")).await;
    let hit = &results["hits"][0];
    assert_eq!((hit["provider"].as_str(), hit["name"].as_str()), (Some("hangar"), Some("WorldGuard")));
    let (status, plan) = app
        .post(
            &format!("/profiles/{paper}/content/plan"),
            json!({ "items": [{ "provider": "hangar", "projectId": hit["id"] }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{plan}");
    assert_eq!(plan["install"].as_array().unwrap().len(), 2);
    let (status, err) =
        app.call(Method::GET, &format!("/profiles/{paper}/content/search?provider=curseforge&q=x"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    // An admin adds a key; it's never sent back.
    let (status, settings) =
        app.call(Method::PATCH, "/settings", Some(json!({ "curseforgeApiKey": "test-key" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["curseforgeKeySet"], true);
    assert!(!settings.to_string().contains("test-key"));
    let index = app.get(&format!("/profiles/{paper}/content")).await;
    assert_eq!(source(&index, "curseforge")["available"], true);
}

#[tokio::test]
async fn restricted_files_are_downloaded_by_hand_and_checked() {
    let app = TestApp::with(|config| config.curseforge_api_key = Some("test-key".into())).await;
    let instance = app.create_instance(json!({})).await;
    let (id, profile) = (instance["id"].as_str().unwrap(), instance["profileId"].as_str().unwrap());

    // EssentialsX's author disabled third-party downloads on (fake) CurseForge.
    let (_, plan) = app
        .post(
            &format!("/profiles/{profile}/content/plan"),
            json!({ "items": [{ "provider": "curseforge", "projectId": "fk-essentialsx" }] }),
        )
        .await;
    let item = &plan["install"][0];
    assert!(item["manualDownload"].as_str().unwrap().starts_with("https://www.curseforge.com/"), "{plan}");
    let (status, index) = app
        .post(
            &format!("/profiles/{profile}/content"),
            json!({ "items": [{ "provider": "curseforge", "projectId": item["projectId"], "versionId": item["versionId"] }] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{index}");
    let entry = index["entries"][0].clone();
    assert!(entry["manualDownload"].is_string());
    let entry_id = entry["id"].as_str().unwrap();

    // Servers won't start without it.
    let (status, err) = app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    assert_eq!(status, StatusCode::OK, "start is accepted; preparing fails: {err}");
    let failed = app.wait_state(id, "error").await;
    assert!(failed["lastError"].as_str().unwrap().contains("downloaded by hand"), "{failed}");

    // The wrong file is refused; the right one is accepted.
    let path = format!("/profiles/{profile}/content/{entry_id}/file");
    let (status, err) = upload(&app, &path, "EssentialsX.jar", b"not it".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"]["message"].as_str().unwrap().contains("checksum"), "{err}");
    let bytes = format!("lodestone fake content: {}\n", item["versionId"].as_str().unwrap()).into_bytes();
    let (status, index) = upload(&app, &path, "EssentialsX-2.21.0.jar", bytes).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["entries"][0]["manualDownload"], Value::Null);

    app.post(&format!("/instances/{id}/actions"), json!({ "action": "start" })).await;
    app.wait_state(id, "running").await;
    let plugins = app.state.paths.profile(profile).join("plugins");
    assert!(plugins.join(entry["filename"].as_str().unwrap()).is_file());
}

#[tokio::test]
async fn the_general_upload_also_fills_a_manual_download() {
    let app = TestApp::with(|config| config.curseforge_api_key = Some("test-key".into())).await;
    let profile = paper_profile(&app).await;
    let (_, index) = app
        .post(
            &format!("/profiles/{profile}/content"),
            json!({ "items": [{ "provider": "curseforge", "projectId": "fk-essentialsx", "versionId": "fk-essentialsx-2.21.0" }] }),
        )
        .await;
    assert!(index["entries"][0]["manualDownload"].is_string());
    let bytes = b"lodestone fake content: fk-essentialsx-2.21.0\n".to_vec();
    let (status, index) = upload(&app, &format!("/profiles/{profile}/content/upload"), "whatever.jar", bytes).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    let entries = index["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "matched the waiting entry instead of adding another");
    assert_eq!(entries[0]["manualDownload"], Value::Null);
}
