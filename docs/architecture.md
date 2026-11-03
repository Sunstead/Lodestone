# Lodestone architecture

Lodestone runs Minecraft servers on one host for one admin. It keeps three
things apart:

| Asset | What it is | Lifetime |
|---|---|---|
| **World** | Level data on disk plus world-scoped rules (seed, difficulty, gamemode, data packs). Owns its backups. | Survives every server. Attached to at most one instance at a time. |
| **Profile** ("Configuration" in the UI) | Software and version, mods and plugins, profile-scoped `server.properties`, memory/CPU, JVM args, ops/whitelist/bans. | Reusable by any number of instances. |
| **Instance** | One profile running one world in one container: ports, console, players, metrics. | Disposable. Deleting it keeps the world and the profile. |

`docs/decoupling-plan.md` explains why the model is split this way.

## Nothing is bundled

Lodestone ships no Minecraft code, mods or Java runtimes:

- Server jars are resolved and downloaded from their publishers when a server first needs them:
  - Mojang's piston-meta for vanilla
  - PaperMC's Fill API for Paper
  - Fabric meta for Fabric
  - Forge and NeoForge from their Mavens
- Every download is checked against its published hash and stored content-addressed in `cache/blobs/<algo>/<hash>`.
- Servers run in stock `eclipse-temurin:<java>-jre` images. The Java major version comes from Mojang's version metadata.
- `eula.txt` is written only after the admin accepts the Minecraft EULA in Settings.

## Repository

```
backend/    Rust workspace
  crates/lodestone/          library: everything
  crates/lodestone-server/   `lodestone` binary (CLI → lodestone::run)
  crates/mc-rcon/            async RCON client
frontend/   Vite + React + TanStack Router/Query
  src/api/generated/         TypeScript contract generated from Rust (ts-rs)
  src/api/demo.ts            in-browser demo backend implementing the same contract
e2e/        Playwright (projects: demo, fake)
shared/     server-properties.schema.json (field metadata for both sides)
scripts/    dev.mjs, with-backend.mjs
```

## Backend modules (`backend/crates/lodestone/src`)

| Module | Responsibility |
|---|---|
| `model` | Every wire type. `cargo test` exports them to `frontend/src/api/generated/`. |
| `api/` | Axum routes under `/api/v1`. Thin: validation and DTO assembly. |
| `store` + `migrations/` | SQLite. Assets are JSON documents. Relational rules live in the schema (`UNIQUE(world_id)`, `ON DELETE RESTRICT`). |
| `supervisor` | Per-instance state machine, console ring buffer, RCON, player tracking, crash policy, metrics sampling, boot-time reconcile. |
| `runtime/` | `Runtime` trait. `fake` simulates a server in-process and serves real RCON. `docker` (M3) runs containers through bollard. |
| `software/` | `SoftwareProvider` trait: catalog, Java version, artifact to run. `fake` is offline. Real providers arrive in M2. |
| `cache` | Hash-verified, content-addressed downloads. |
| `properties` | Shared schema, ownership validation, `server.properties` rendering and parsing. |
| `backups` | `tar.zst` archives with `save-off`/`save-all flush` around them, atomic restore, scheduler. |
| `jobs`, `events` | Long-running work with persisted progress. A broadcast bus feeds `GET /events` (SSE). |
| `host`, `ports`, `logparse` | Host metrics, port allocation, log/command-reply parsing. |

## How a server starts

```
start ─▶ preparing ─▶ starting ─"Done ("─▶ running ─stop─▶ stopping ─▶ stopped
             │            │                   │
             └── error ◀──┴──── crash (over the profile's threshold)
```

1. **Prepare** (a `prepare` job with progress):
   - Resolve software and version to an artifact.
   - Fetch the artifact into the cache.
   - Make sure the JRE image exists.
   - Write the run dir: `server.properties` merged from world + profile + managed keys, `eula.txt`, and ops/whitelist/ban JSON.
2. **Run.** The container bind-mounts:

   | Host | Container | Access |
   |---|---|---|
   | `instances/<id>` | `/server` | read-write |
   | the cached jar | `/server/server.jar` | read-only |
   | `worlds/<worldId>` | `/server/worlds` | read-write |
   | the profile's `plugins/` or `mods/`+`config/` | `/server/plugins`, `/server/mods`, `/server/config` | read-write |

   The command is `java -Xms -Xmx … -jar server.jar --universe worlds --nogui`. Vanilla, Paper, Fabric and Forge all honour `--universe`, so one mount works for both world layouts. The game port is published on every interface. RCON is published on `127.0.0.1` only, with a per-instance random password.
3. **Supervise:**
   - Parse log lines: `Done (` means running; joins and leaves track players and playtime per world.
   - Sample container stats.
   - Read TPS from Paper's `tps` or vanilla's `tick query` (1.20.3+). Otherwise TPS is reported as `null`.
   - On a crash, restart up to `crashThreshold` times in 10 minutes. After that the instance goes to `error`, with the last error lines attached.
4. **Stop.** RCON `stop` saves and exits. The runtime's stop (SIGTERM, then kill after the timeout) is the safety net.

**World safety:**
- A world attaches to one instance (enforced by the database).
- Generation fields lock once the world has generated.
- A Paper-layout world refuses non-Paper profiles (`layout_mismatch`).
- DataVersion downgrade guards come in M4.

## Frontend modes

`VITE_LODESTONE_MODE`:

- `demo`: the in-browser fake.
- `live`: the backend. If it is down, the UI says so and keeps retrying.
- `auto` (the dev default): probe the backend, else demo.

Production builds default to `live`.

In live mode, `useEventStream()` subscribes to `GET /events` and invalidates the matching TanStack Query keys.

## Testing

| Layer | Where | Run |
|---|---|---|
| Rust unit tests | `#[cfg(test)]` in each module | `cargo test` |
| API integration: real router, SQLite, fake runtime with real RCON | `backend/crates/lodestone/tests/api.rs` | `cargo test` |
| Frontend unit tests | `frontend/src/**/*.test.ts` | `npm test` |
| **Contract parity**: the same scenarios against demo and a live backend | `frontend/src/api/contract.test.ts` | `just test-contract` |
| Browser E2E (demo, full stack on the fake runtime) | `e2e/` | `just e2e` |
| Real containers | `--features docker-tests` (M3) | `just test-docker` |

## Roadmap

- [x] **M0** Monorepo, tooling, shared property schema, CI.
- [x] **M1** Backend on the fake runtime, end to end: CRUD with one-request (atomic) create, lifecycle, console (SSE), RCON, players, backups, jobs, events, settings/system. Generated contract, contract parity, E2E.
- [ ] **M2** Real software providers: Mojang, Paper, Fabric, then Forge/NeoForge with an installer job.
- [ ] **M3** Docker runtime: bollard, image pulls, adopt-on-boot, stats, `just smoke` runs a joinable server.
- [ ] **M4** Worlds: level.dat (DataVersion guard, offline day/time), import/download, layout conversion.
- [ ] **M5** Modrinth: search, install plan with dependencies, update check, upload, `.mrpack` import. Mods panel rewrite.
- [ ] **M6** Auth (single admin), onboarding wizard, frontend embedded in the binary, Docker image and compose file.
- [ ] **M7** CurseForge (API key, manual download for restricted files) and Hangar.
