# Lodestone architecture

Lodestone runs Minecraft servers on one Docker host. People sign in through
Authentik (OIDC) and each owns their own worlds, configurations and servers;
admins come from Authentik groups. It keeps three things apart:

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
  - Forge and NeoForge installers from their Mavens. Each installer runs once per version in a throwaway JRE container into `cache/installs/<key>`, and servers launch with the `@libraries/.../unix_args.txt` it writes.
- Mods and plugins come from Modrinth, Hangar (Paper plugins) or CurseForge (with an API key), pinned by hash in each profile's lockfile, or from an upload.
- Some files can't be downloaded by Lodestone: their authors disabled third-party downloads on CurseForge, or the files are hosted elsewhere on Hangar. These are recorded as manual downloads. The user downloads the file and uploads it, Lodestone checks it against the published hash, and servers won't start until it's there. This is how Prism handles them.
- Every download is checked against its published hash and stored content-addressed in `cache/blobs/<algo>/<hash>`.
- Servers run in stock `eclipse-temurin:<java>-jre` images. The Java major version comes from Mojang's version metadata.
- `eula.txt` is written only after the server's owner accepts the Minecraft EULA in Settings.

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
| `api/` | Axum routes under `/api/v1`. Thin: validation, ownership checks, DTO assembly. |
| `auth/` | OIDC (Authentik), ported from Solstice: offline JWKS verification, the server-side PKCE code flow (`/auth/login`, `/auth/callback`, `/auth/logout`), session cookies, the CSRF guard, `CurrentUser` and ownership checks. `--dev-user` for development. |
| `content/` | Mods and plugins. `ContentProvider`s: Modrinth (sha512, batch update check), Hangar (sha256, Paper only), CurseForge (sha1, API key from Settings or `LODESTONE_CURSEFORGE_API_KEY`), and offline fakes of all three over `shared/fake-content.json`, which the demo shares. Also install plans, the per-profile lockfile, manual downloads, jar uploads, `.mrpack` import, and linking jars into `plugins/` or `mods/` on start. |
| `level`, `worldfiles` | `level.dat` (version that last saved a world, seed, day), world zip import and export. |
| `store` + `migrations/` | SQLite. Assets are JSON documents with an `owner_id`. Relational rules live in the schema (`UNIQUE(world_id)`, `ON DELETE RESTRICT`). Users, sessions (hashed tokens) and sign-in flows. |
| `supervisor` | Per-instance state machine, console ring buffer, RCON, player tracking, crash policy, metrics sampling, boot-time reconcile. |
| `runtime/` | `Runtime` trait. `fake` simulates a server in-process and serves real RCON. `docker` runs containers through bollard. |
| `software/` | `SoftwareProvider` trait: catalog, Java version, what to run (a jar, or a loader installer). `real` talks to Mojang, PaperMC, Fabric, Forge and NeoForge; `fake` is offline. |
| `cache` | Hash-verified, content-addressed downloads. |
| `properties` | Shared schema, ownership validation, `server.properties` rendering and parsing. |
| `backups` | `tar.zst` archives with `save-off`/`save-all flush` around them, atomic restore, scheduler. |
| `jobs`, `events` | Long-running work with persisted progress. A broadcast bus feeds `GET /events` (SSE). |
| `dns` | Address names: an SRV record per named server (`_minecraft._tcp.<name>.<public address>`), kept in Cloudflare by a background sync that only touches records it tagged, or shown for adding by hand. |
| `host`, `ports`, `logparse` | Host metrics, port allocation, log/command-reply parsing. |

## How a server starts

```
start ─▶ preparing ─▶ starting ─"Done ("─▶ running ─stop─▶ stopping ─▶ stopped
             │            │                   │
             └── error ◀──┴──── crash (over the profile's threshold)
```

1. **Prepare** (a `prepare` job with progress):
   - Resolve software and version to an artifact.
   - Make sure the JRE image exists.
   - Fetch the jar into the cache, or for Forge/NeoForge, run the installer once.
   - Write the run dir: `server.properties` merged from world + profile + managed keys, `eula.txt`, and ops/whitelist/ban JSON.
   - Link the profile's enabled mods or plugins from the cache into its `plugins/` or `mods/` folder and remove any other top-level jar there. Config folders beside them are left alone.
2. **Run.** The container bind-mounts:

   | Host | Container | Access |
   |---|---|---|
   | `instances/<id>` | `/server` | read-write |
   | the cached jar (or the loader install's `libraries/`) | `/server/server.jar` (`/server/libraries`) | read-only |
   | `worlds/<worldId>` | `/server/worlds` | read-write |
   | the profile's `plugins/` or `mods/`+`config/`, and any other folder a modpack brought | `/server/<name>` | read-write |

   The command is `java -Xms -Xmx … -jar server.jar --universe worlds --nogui`. Vanilla, Paper, Fabric and Forge all honour `--universe`, so one mount works for both world layouts. The game port is published on every interface. RCON is published on `127.0.0.1` only, with a per-instance random password.

   Containers run with no capabilities, `no-new-privileges` and a process limit. With `LODESTONE_DOCKER_NETWORK` they join a bridge network with traffic between containers off, which a host firewall rule keeps off the LAN. With `LODESTONE_GAME_PORTS`, game ports come from that range only and there's no query port; with `LODESTONE_PUBLIC_SERVERS`, `online-mode` is forced on (`docs/deploy.md` §4).
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
- After each run Lodestone reads `level.dat`. A world refuses a Minecraft version older than the one that last saved it (`world_newer`).

## Accounts and ownership

- **Sign-in:** OIDC through Authentik, as a public client with PKCE. The backend runs the code flow and gives the browser an HttpOnly session cookie; no provider token reaches the browser. Scripts may send an Authentik access token as a bearer token instead.
- **Ownership:** every world, profile and instance has an `owner_id`.
  - Lists return the caller's own.
  - Anything else answers 404, so ids can't be probed.
  - A server is made of its owner's own world and profile.
  - Ports are host-wide, so they're checked across everyone.
- **Admins** are members of `LODESTONE_OIDC_ADMIN_GROUPS`. They change app settings, see the People list, and can open anyone's assets.
- **The EULA** is accepted per user, for their own servers.
- **Events and jobs** over SSE are filtered to what the caller can see.
- **Cookie-authenticated changes** need `X-Lodestone-Request: 1` and a same-origin `Origin`.
- `docs/deploy.md` covers the Authentik setup and the compose file.

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
| Browser E2E: demo, full stack on the fake runtime, and sign-in through a stand-in OIDC provider (`e2e/mock-oidc.mjs`) | `e2e/` | `just e2e` |
| Auth: tokens, sessions, CSRF, ownership, groups (mock OIDC provider) | `backend/crates/lodestone/src/auth/tests.rs` | `cargo test` |
| Real containers: Vanilla, Paper with a real Modrinth plugin and a real Hangar plugin, Fabric, NeoForge (installer), reattach | `backend/crates/lodestone/tests/docker.rs` | `just test-docker` |
| Live publisher APIs (Mojang, PaperMC, Fabric, Forge, NeoForge, Modrinth, Hangar) | `#[ignore]` tests in `software/real.rs`, `content/modrinth.rs`, `content/hangar.rs` | `just test-live` |
| A joinable server, end to end | `scripts/smoke.mjs` | `just smoke` (`--keep` to play) |

## Roadmap

- [x] **M0** Monorepo, tooling, shared property schema, CI.
- [x] **M1** Backend on the fake runtime, end to end: CRUD with one-request (atomic) create, lifecycle, console (SSE), RCON, players, backups, jobs, events, settings/system. Generated contract, contract parity, E2E.
- [x] **M2** Real software providers: Mojang (sha1), Paper Fill v3 (sha256), Fabric (launcher pinned on first download). Exact Java from publisher metadata (e.g. 26.x → Java 25). SQLite-cached catalogs with stale fallback when offline.
- [x] **M2b** Forge (1.17+) and NeoForge: the installer runs once per version in a throwaway container into `cache/installs/`; servers launch with `@unix_args.txt`.
- [x] **M3** Docker runtime: bollard, image pulls with progress, `127.0.0.1`-only RCON, runs as the data dir's owner, adopt-on-boot, container stats. `just test-docker` runs Vanilla/Paper/Fabric/NeoForge and a reattach test; `just smoke` brings up a joinable server.
- [x] **M4** Worlds: level.dat (version guard, seed, offline day/time), zip import and download. *Not done:* converting a Paper-layout world back to vanilla.
- [x] **M5** Modrinth: search, install plans with dependencies, update checks, uploads, `.mrpack` import. Mods panel rewrite.
- [x] **M6** Accounts through Authentik (OIDC) with per-user ownership, a Docker image and compose file (`docs/deploy.md`). *Not done:* a first-run onboarding wizard (Settings covers the EULA and public address).
- [x] **M7** Hangar (Paper plugins) and CurseForge (an API key in Settings; restricted files become manual downloads, checked against their sha1). The CurseForge client is tested against the API's documented shapes; it hasn't run against the live API, which needs a key.
- [x] **M8** Public servers: a fixed game-port range, no public query port, an isolated Docker network and hardened containers, `online-mode` kept on. Address names (`survival.play.example.com`) through SRV records, kept in Cloudflare.
- [ ] Later:
  - layout conversion
  - per-user quotas (servers, memory)
  - the CurseForge modpack format
  - recognising uploaded jars on CurseForge (by murmur2 fingerprint)
  - CurseForge update checks in one request (fingerprints) instead of one per project
