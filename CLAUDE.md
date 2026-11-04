# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository layout

Lodestone runs Minecraft servers on one Docker host as three decoupled assets: **World**, **Profile** (called "Configuration" in the UI) and **Instance**. People sign in through Authentik and each owns their own assets. It's a monorepo (git root is `Lodestone/`):

- `backend/`: Rust workspace. `crates/lodestone` (library, all logic), `crates/lodestone-server` (the `lodestone` binary), `crates/mc-rcon`.
- `frontend/`: Vite + React 19 SPA with TanStack Router (file-based) and TanStack Query.
- `e2e/`: Playwright projects `demo` (Vite only), `fake` (backend on the simulated runtime serving `frontend/dist`) and `oidc` (the same, signing in through `e2e/mock-oidc.mjs`).
- `shared/`: read by both sides (`include_str!` in Rust, imports in the demo).
  - `server-properties.schema.json`: `server.properties` field metadata.
  - `fake-content.json`: the offline mod/plugin catalog.
- `scripts/`: `dev.mjs` (backend + Vite together) and `with-backend.mjs` (runs a command against a throwaway backend).
- `docs/architecture.md` is the current architecture and roadmap. `docs/deploy.md` covers Authentik and the compose file. `docs/decoupling-plan.md` explains why the model is split.
- `Dockerfile` + `compose.example.yaml`: the production image (backend + built UI).

## Commands

`just` recipes (root `package.json` mirrors the main ones for machines without `just`):

```bash
just setup          # npm installs + Playwright chromium
just dev-demo       # frontend only, demo data
just dev-fake       # backend (simulated servers, no Docker/network) + Vite on :3000
just dev            # backend with Docker + real downloads + Vite
just test           # cargo test + clippy -D warnings + tsc + vitest
just test-contract  # same scenarios against the demo AND a live backend
just e2e            # build:live + Playwright
just gen-types      # regenerate frontend/src/api/generated from Rust
```

To run a single test:
- Rust: `cd backend && cargo test --test api lifecycle_console_and_players`, or `cargo test -p lodestone logparse::`.
- Vitest: `cd frontend && npx vitest run src/api/contract.test.ts -t "backs up"`.
- Playwright: `cd e2e && npx playwright test --project=fake`.

The backend binary takes these flags (each also has a `LODESTONE_*` env var):
- `--runtime docker|fake`
- `--providers real|fake`
- `--data-dir` (default `./.lodestone-dev`)
- Sign-in: `--oidc-issuer` + `--public-url` (+ `--oidc-admin-groups`, `--oidc-allowed-groups`), or `--dev-user <name>`. With neither, it refuses to start.
- `--accept-eula` (dev/test only)
- `--fake-speed`
- `--static-dir frontend/dist`

The dev scripts pass `--dev-user dev` unless `LODESTONE_OIDC_ISSUER` is set.

## Architecture essentials

- **Contract source of truth is Rust.** Wire types live in `backend/crates/lodestone/src/model.rs` with `#[derive(TS)]`. `cargo test` writes them to `frontend/src/api/generated/` (path set in `backend/.cargo/config.toml`), and `frontend/src/api/types.ts` re-exports them. Never edit generated files. Change the Rust type and regenerate; CI fails on drift. ts-rs details:
  - `u64`/`i64` need `#[ts(type = "number")]`.
  - Optional request fields use `#[ts(optional)]`.
- **The demo must keep implementing the contract.** `frontend/src/api/client.ts` branches every call on `mode` (`live` → HTTP, `demo` → `demo.ts`). When adding or changing an endpoint:
  - Update both branches.
  - Add the scenario to `frontend/src/api/contract.test.ts`, which runs against both.
- **Modes:** `VITE_LODESTONE_MODE=demo|live|auto`. Production builds default to `live` and never silently fall back to demo data.
- **Query invalidation:** keys in `queries.ts` are prefixed `worlds` / `profiles` / `instances`. Instances render world and profile names, so world/profile mutations also invalidate `instances`. In live mode, `useEventStream()` maps backend SSE `LodestoneEvent`s to keys via `keysForEvent`.
- **Backend layering:** `api/*` handlers are thin; lifecycle work is in `supervisor.rs`, archives in `backups.rs`. Relational rules are enforced by SQLite constraints in `migrations/` and mapped to 409s in `store.rs`:
  - a world attaches to one instance: `UNIQUE(world_id)`
  - no deleting worlds or profiles that are in use: `ON DELETE RESTRICT`
- **Errors:** `AppError` renders `{ error: { code, message } }`. Messages are written for the UI user, and `code` is stable (`eula_required`, `world_attached`, `layout_mismatch`, `world_newer`, `content_unsupported`, `incompatible`, `not_running`, `unauthorized`, `forbidden`, …). The frontend surfaces `ApiError.message` directly; a 401 sends the browser to `/auth/login`.
- **Auth and ownership** (`auth/`, ported from Solstice):
  - Every `/api/v1` route except `/health` needs a `CurrentUser` (route layer in `api/mod.rs`).
  - Handlers that take an asset id call `user.check(&app.store, Asset::…, &id)`, which answers 404 for other people's ids (admins pass). Lists pass `Some(&user.id)` to `store.list_*`; background code passes `None`.
  - New assets are inserted with an owner. A server's world and profile must share its owner.
  - Cookie-authenticated changes need `X-Lodestone-Request: 1` (the client always sends it).
  - The EULA is per user (`PATCH /me`), not an app setting.
- **Content** (`content/`):
  - A profile's mods and plugins are a lockfile (`profile_content`) pinned by sha512. Jars live in the blob cache and are linked into `profiles/<id>/plugins|mods` by `install::sync_files` when a server prepares.
  - Install plans (`content/plan.rs`) resolve dependencies; the UI shows the plan, then applies its `install` list.
  - The demo mirrors all of this in `frontend/src/api/demo-content.ts`.
- **Runtimes:** `runtime::Runtime` is the seam.
  - `FakeRuntime` prints realistic log lines, writes a real `level.dat`, serves **real RCON** on the instance's RCON port, and simulates loader installers (`run_task`). So the supervisor, console and player code paths are identical to real servers.
  - Test hooks are sent as console commands: `lodestone:join <name>`, `lodestone:leave <name>`, `lodestone:crash`.
- **Nothing Minecraft-related is bundled.** Jars come from publishers at runtime via `software::SoftwareProvider` and are hash-verified by `cache::BlobCache`. They run in stock `eclipse-temurin:<java>-jre` images.
  - Forge and NeoForge resolve to `Launch::Installer`. The installer runs once into `cache/installs/<key>`, and servers launch with `@libraries/.../unix_args.txt`.
  - `eula.txt` is only written after the server's owner accepts the EULA.
- **Property ownership:** each `server.properties` key belongs to exactly one of world/profile/instance (`scope` in the shared schema). RCON, ports and `server-ip` are `readOnly` because Lodestone manages them.

## Frontend conventions

- UI components come from `@sunstead/ui` (GitHub-hosted, source-only), imported by deep path. It's excluded from Vite `optimizeDeps` on purpose: pre-bundling duplicates React contexts. Keep it that way.
- Tailwind v4 via `src/styles.css` (bevel tokens `--bevel-*`, `--radius: 0`). SVGs import as components with `?react`.
- Path aliases: `#/*` and `@/*` both map to `src/*`.
- Navigation is a single route (`src/routes/index.tsx`) driven by `?instance=` / `?profile=` / `?world=` / `?page=`. Only one asset param is set at a time. Pages are registered in `PAGES` in `components/server-navigation.tsx` and live in `src/routes/-pages/`.
- `src/routeTree.gen.ts` is generated by the router plugin. `src/router.tsx` and `components/api-provider.tsx` are unused scaffolding.
