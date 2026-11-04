# Decoupling Worlds, Profiles, and Instances — UI Plan

## The problem in one sentence

Today a single `serverId` and the endpoint tree `/servers/:id/*` represent all three concepts at once, so a "server" is simultaneously a world (a folder of region files), a profile (software, version, mods, settings), and a running container — which makes reuse across worlds impossible and forces users to recreate everything to change one part.

## Target model

Three independent assets with their own lifecycles, joined by an instance:

| Concept | Persistence | Owns | Lifecycle |
|---|---|---|---|
| **World** | Persistent, independent | Region/level data, seed, level-type, dimensions, difficulty, world-specific gameplay policy, per-world datapacks | Created once, survives every container. Can be backed up, duplicated, archived, deleted. |
| **Profile** | Persistent, independent | Software + version, plugins/mods, `server.properties` settings, resource limits (heap/CPU), JVM args, supervision policy, network/access config | Editable; can be duplicated and reused across worlds. Version bumps recreate containers without touching worlds. |
| **Instance** | Ephemeral, derived | A profile **+** a selected world running in one isolated Docker container: container id/health, port bindings, runtime state, live metrics, console, env | Created/destroyed freely. Destroying an instance never loses world or profile data. |

The linkage is exactly one field the model already has implicitly: `Settings.worldVolume` is a **mount path string**, which is why nothing can be reused. It becomes an explicit `worldId` foreign key on the instance.

---

## 1. Wire model changes (`src/api/types.ts`)

Introduce three entities and re-point the existing ones.

### New: `World`

```ts
export interface World {
  id: string;
  name: string;                 // user-facing, e.g. "Survival Season 2"
  folder: string;               // resolved level-name on disk, e.g. "world"
  levelType: LevelType;         // 'minecraft:normal' | 'minecraft:flat' | ... (generation-only)
  seed: string | null;          // generation-only; null = random
  generateStructures: boolean;  // generation-only
  maxWorldSize: number;
  allowNether: boolean;
  difficulty: Difficulty;       // world-level, was ServerStatus.world.difficulty
  gamemode: GameMode;           // world default
  hardcore: boolean;
  spawnProtection: number;
  spawnMonsters: boolean;
  spawnNpcs: boolean;
  pvp: boolean;
  allowFlight: boolean;
  enableCommandBlock: boolean;
  dataPacks: { enabled: string[]; disabled: string[] };
  sizeBytes: number;            // was ServerStatus.disk.worldBytes
  createdAt: string;
  updatedAt: string;
  /** Instance currently mounting this world, if any. Enforces 1:1 attachment. */
  attachedInstanceId: string | null;
}
```

Note `seed`/`levelType`/`generateStructures` keep the existing "generation-only" semantics already documented in the create dialog ("Only used the first time the world generates.") — they are now clearly world-scoped and immutable-after-generation in the UI.

### New: `Profile`

```ts
export interface Profile {
  id: string;
  name: string;                 // "Paper 1.21.4 · Performance"
  description: string;
  softwareId: ServerSoftwareId;
  version: string;
  image: string;                // derived tag
  memory: { minMb: number; maxMb: number; initialMb: number };
  cpuCores: number | null;      // new: resource limit, currently absent
  javaArgs: string;
  autoStart: boolean;
  restartOnCrash: boolean;
  crashThreshold: number;
  /** server.properties that belong to the software/behaviour, not the world. */
  properties: Record<string, string>;
  mods: ModIndex;               // profile-scoped, was per-server
  ports: { rcon: number; query: number | null };  // defaults; instance may override minecraft
  createdAt: string;
  updatedAt: string;
  /** How many instances currently run this profile — drives delete guards. */
  instanceCount: number;
}
```

### New: `Instance` (replaces `ServerSummary`)

```ts
export interface Instance {
  id: string;
  name: string;                 // "survival-1"
  profileId: string;
  worldId: string;
  state: ServerState;
  players: { online: number; max: number };   // max sourced from profile
  address: string;
  ports: { minecraft: number; rcon: number; query: number | null };
  environment: Record<string, string>;
  createdAt: string;
  /** Resolved for display so the sidebar can render without N+1 fetches. */
  profile: { name: string; software: string; version: string };
  world: { name: string; folder: string };
}
```

Keeping `profile`/`world` denormalised on the instance is deliberate: the sidebar and hero need a name and version to render, and this avoids a waterfall.

### `ServerStatus` — strip world data

Remove `disk.worldBytes` and the whole `world: { chunks, entities, blockEntities, day, timeOfDay, difficulty }` block. Those move to a new `WorldStatus`, fetched for the *world*, not the instance. Keep `state`, uptime, memory, cpu, tps, tickMs, network, host, container.

### New: `WorldStatus` (world runtime metrics, independent of any instance)

```ts
export interface WorldStatus {
  worldId: string;
  attached: boolean;            // is some instance running it right now
  diskBytes: number;
  chunks: number;
  entities: number;
  blockEntities: number;
  day: number;
  timeOfDay: string;
  dimensions: { nether: boolean; end: boolean };
}
```

### `Backup` — bind to a world, not a server

```ts
export interface Backup {
  // ...existing id/name/kind/createdAt/sizeBytes/status/note
  worldId: string;              // replaces worlds: string[]
  instanceId: string | null;    // which instance produced it, for provenance
}
```

`BackupIndex` moves under a world (`BackupStorage`/`BackupSchedule` become per-world), so schedules follow the world rather than the container.

### `ServerSettings` → split

Delete `ServerSettings` entirely. Its fields distribute as:
- → `Profile`: `description`, `softwareId`, `version`, `image`, memory fields, `javaArgs`, supervision fields, `environment` (loader-agnostic parts), `ports.rcon`/`ports.query`.
- → `Instance`: `ports.minecraft`, `environment` (port/TZ/EULA specifics).
- → `World`: `worldVolume` is **deleted** — replaced by the `worldId` relation.

### New request types

```ts
export interface CreateWorldRequest { name; levelType; seed?; generateStructures; difficulty; gamemode; hardcore; maxWorldSize; allowNether; ... }
export interface CreateProfileRequest { name; description; softwareId; version; memory...; cpuCores?; javaArgs; autoStart; restartOnCrash; crashThreshold; properties; }
export interface CreateInstanceRequest { name; profileId; worldId; minecraftPort; rconPort?; queryPort?; environment?; }
```

The old `CreateServerRequest` — which mixed a `levelSeed` (world) with `memoryMb` (profile) and `minecraftPort` (instance) — is removed.

---

## 2. Endpoint tree (`src/api/client.ts`, `types.ts` header)

Replace the flat `/servers/:id/*` tree with three families plus instance composition:

```
GET    /worlds                     POST   /worlds
GET    /worlds/:id                 PATCH  /worlds/:id        DELETE /worlds/:id
POST   /worlds/:id/duplicate
GET    /worlds/:id/status                                  # WorldStatus
GET    /worlds/:id/backups         POST   /worlds/:id/backups
DELETE /worlds/:id/backups/:bid    POST   /worlds/:id/backups/:bid/restore
GET    /worlds/:id/backups/schedule  PATCH /worlds/:id/backups/schedule

GET    /profiles                   POST   /profiles
GET    /profiles/:id               PATCH  /profiles/:id      DELETE /profiles/:id
POST   /profiles/:id/duplicate
GET    /profiles/:id/mods          POST   /profiles/:id/mods
DELETE /profiles/:id/mods/:mid     PATCH  /profiles/:id/mods/:mid
POST   /profiles/:id/mods/:mid/update
GET    /profiles/:id/properties    PUT    /profiles/:id/properties
GET    /profiles/software                                        # available software + versions

GET    /instances                  POST   /instances
GET    /instances/:id              PATCH  /instances/:id     DELETE /instances/:id
POST   /instances/:id/actions      { action: start|stop|restart }
GET    /instances/:id/status
GET    /instances/:id/console?tail=  POST /instances/:id/console
GET    /instances/:id/console/stream                            # SSE
GET    /instances/:id/players      POST   /instances/:id/players
POST   /instances/:id/world                                     # hot-swap bound world
```

`GET /profiles/software` is new: the create dialog currently hardcodes software/version lists in the component. Surfacing them from the API lets the backend own image/tag availability.

---

## 3. Hooks (`src/api/queries.ts`)

Re-key everything off three prefixes: `['worlds']`, `['profiles']`, `['instances']`.

```ts
keys = {
  worlds: ['worlds'], world: (id) => ['worlds', id],
  worldStatus: (id) => ['worlds', id, 'status'],
  worldBackups: (id) => ['worlds', id, 'backups'],
  profiles: ['profiles'], profile: (id) => ['profiles', id],
  profileMods: (id) => ['profiles', id, 'mods'],
  profileProperties: (id) => ['profiles', id, 'properties'],
  software: ['profiles', 'software'],
  instances: ['instances'], instance: (id) => ['instances', id],
  instanceStatus: (id) => ['instances', id, 'status'],
  instancePlayers: (id) => ['instances', id, 'players'],
  console: (id) => ['instances', id, 'console'],
}
```

New hooks: `useWorlds`, `useWorld`, `useWorldStatus`, `useCreateWorld`, `useUpdateWorld`, `useDuplicateWorld`, `useDeleteWorld`, `useProfiles`, `useProfile`, `useCreateProfile`, `useUpdateProfile`, `useDuplicateProfile`, `useDeleteProfile`, `useSoftware`, `useInstances`, `useInstance`, `useCreateInstance`, `useUpdateInstance`, `useDeleteInstance`, `useSwitchWorld`.

**Cross-invalidation rules** (new, and important):
- `useDeleteWorld` must invalidate `worlds` **and** `instances` (an instance's `world` display field goes stale).
- `useDeleteProfile` likewise invalidates `profiles` and `instances`.
- `useSwitchWorld` / `useUpdateInstance` invalidates `instances`, `instances/:id`, and both `worlds` and `worlds/:id/status` (attachment flips).
- `useUpdateProfile` invalidates `profiles` and `instances` (profile name/version shown in sidebar and hero).

---

## 4. Navigation & shell (`src/components/server-navigation.tsx`, `src/routes/index.tsx`)

Per your choice, the sidebar becomes **three top-level sections**:

```
LODESTONE
[+ New Instance]

INSTANCES · 2/3 running
  ● Survival-1      Paper 1.21.4 · Survival Season 2
  ● Creative-1      Vanilla 1.21.4 · Flat Test
  ● Modded-1        Fabric 1.21.1 · Create World

PROFILES · 3
  Paper 1.21.4 · Performance
  Vanilla 1.21.4 · Vanilla
  Fabric 1.21.1 · Create

WORLDS · 4
  Survival Season 2      6.3 GB · attached
  Flat Test              120 MB · attached
  Create World           2.1 GB · attached
  Archived Nether Test   1.4 GB · free
```

- `ServerSidebar` → `AppSidebar` rendering three `SidebarGroup`s. Each world/profile row is a link into its own detail page; each shows an `attached`/`free` badge so "can I reuse this?" is answerable at a glance.
- Routing: the single `/` route with `?server=&page=` becomes search params `?instance=`, `?profile=`, `?world=` plus `?page=`. `validateSearch` gains the three ids and a widened page union.
- The instance hero's subtitle shows the composition explicitly — `Paper 1.21.4 · Survival Season 2` — and its badge shows `World`/`Profile` chips that jump to those pages. This is the single most important UI affordance for teaching the new mental model.

---

## 5. Page-by-page adaptation

### New pages

- **`-pages/worlds.tsx`** — inventory: name, folder, size, dimensions, difficulty, attached-to (instance link or "free"), created/updated, actions (open, duplicate, back up, delete). Delete is guarded when `attachedInstanceId !== null`.
- **`-pages/world-detail.tsx`** — World settings (level-type/seed read-only-after-generation with an explicit "regenerate" danger action), difficulty, gamemode, spawn rules, datapacks; the world's **backup** index and schedule; live `WorldStatus` metrics (chunks/entities/day/time). Absorbs what was the dashboard's "World" card and the backups page.
- **`-pages/profiles.tsx`** — inventory: software/version, loader, heap range, mod count, used-by count, actions (open, duplicate, delete).
- **`-pages/profile-detail.tsx`** — Profile settings (software/version/modpack resources, JVM args, supervision), the **mods & plugins** manager, and the **server.properties** editor, restricted to profile-scoped keys. Absorbs the current mods page and most of the settings and properties pages.

### Adapted pages

- **`dashboard.tsx`** → becomes the **instance dashboard**: container/host/runtime metrics only (uptime, players, memory, CPU, tick rate, host disk/swap, network) plus a "composition" card linking to its world and profile. Remove the World card (moves to world detail), the world-volume row, and the world size meter. The recent-console card stays.
- **`console.tsx`** → near-unchanged; only `serverId` → `instanceId` and endpoint strings `/servers/` → `/instances/`.
- **`properties.tsx`** → moves under a profile and **splits its field list**. World-scoped keys (`level-name`, `level-seed`, `level-type`, `generate-structures`, `max-world-size`, `difficulty`, `gamemode`, `hardcore`, `allow-nether`, spawn settings, datapacks) leave this page for world detail and appear as read-only references with a "managed on the world" note. `level-name` in particular is no longer typed by hand — it is the world's `folder`.
- **`players.tsx`** → instance-scoped. Bans/whitelist copy that referenced "the world volume" is corrected: these are instance/profile access policy, but the JSON files live per-world on disk — the copy should state which world is mounted.
- **`backups.tsx`** → removed as a top-level page; folded into world detail. Backup rows stop showing `worlds.join(', ')` and simply belong to one world.
- **`mods.tsx`** → folded into profile detail (`loaderIsPlugin` derivation still works off `profile.softwareId`).
- **`settings.tsx`** → **split into three.** Profile-identity/resources/supervision/ports-defaults go to profile detail; port overrides and env go to an instance "Runtime" section; delete copy changes fundamentally — the current "Deleting removes the container, the world volume and every backup" becomes "Deleting removes the container only; the profile and world are kept" (with separate, explicitly-worded delete flows on the world/profile pages that warn about data loss).

### Create flow

`create-server-dialog.tsx` → **`create-instance-dialog.tsx`**, restructured into a two-mode wizard:

1. **Pick a world** — existing world from a list, *or* create a new one inline (name, level-type, seed, generate-structures, difficulty, gamemode, hardcore). This preserves the current "World" FieldSet but makes it a choice rather than an unconditional create.
2. **Pick a profile** — existing profile, *or* create one inline (software, version, memory, CPU, JVM args) — the current Identity + Resources fieldsets.
3. **Instance specifics** — name, port bindings (prefilled from profile defaults), env. The current dialog already forces ~3 groups; this maps cleanly.

Because hot-swap is allowed, the dialog can also offer "start immediately" and the instance edit view gets a **Change world** action that performs the automatic stop → remount → restart, with a compatibility warning when `world.levelType` was generated differently from the target profile's expectations.

---

## 6. Property key re-scoping

The properties editor's flat `network|gameplay|world|performance|rcon|advanced` grouping is replaced by owner-based scoping, so a key can only be edited where it belongs:

| Owner | Keys |
|---|---|
| **World** | `level-name` (derived from world folder, read-only), `level-seed`, `level-type`, `generate-structures`, `max-world-size`, `allow-nether`, `spawn-protection`, `spawn-monsters`, `spawn-npcs`, `difficulty`, `gamemode`, `force-gamemode`, `hardcore`, `pvp`, `allow-flight`, `enable-command-block`, `initial-enabled-packs`, `initial-disabled-packs` |
| **Profile** | `motd`, `online-mode`, `white-list`, `enforce-whitelist`, `hide-online-players`, `max-players`, `view-distance`, `simulation-distance`, `max-tick-time`, `max-chained-neighbor-updates`, `network-compression-threshold`, `sync-chunk-writes`, `use-native-transport`, `rate-limit`, `enable-query`, `query.port`, `enable-rcon`, `rcon.port`, `rcon.password`, `broadcast-rcon-to-ops`, `broadcast-console-to-ops`, `enforce-secure-profile`, `prevent-proxy-connections`, `honor-proxy-ips`, `log-ips`, `enable-jmx-monitoring`, `track-random-splits`, `text-filtering-config`, `bug-report-link`, `function-permission-level`, `operator-user-permission-level`, `require-resource-pack`, `resource-pack`, `resource-pack-prompt`, `resource-pack-sha1`, `player-idle-timeout`, `entity-broadcast-range-percentage` |
| **Instance** | `server-port`, `server-ip` |

Edge cases the refactor resolves:
- **`difficulty`** currently smeared across a property, `ServerStatus.world.difficulty`, `CreateServerRequest`, and written back by `saveProperties`. It becomes a single `World.difficulty` field with one write path.
- **`max-players`** currently synced into `summary.players.max` and `status.players.max`. It becomes a profile property; the instance's `players.max` is read from it.
- **`gamemode`/`hardcore`/`maxPlayers`** currently collected under the dialog's "World" FieldSet but stored on the server. They now split: gamemode/hardcore → world, maxPlayers → profile.

---

## 7. Demo backend (`src/api/demo.ts`)

The `DemoServer` bundle (`summary`/`status`/`properties`/`players`/`backups`/`mods`/`settings`, lines 346–358) is replaced by three maps (`worlds`, `profiles`, `instances`) plus an attachment index. Seeds are re-cut:

- **Worlds**: `survival` (Paper world, hard, 6.3 GB), `creative` (`minecraft:flat`, peaceful), `modded` (Create, 2.1 GB), plus a **detached** `archived-nether-test` to exercise the "free" state and prove a world can exist without an instance.
- **Profiles**: `paper-1.21.4-perf` (8 plugins), `vanilla-1.21.4`, `fabric-1.21.1-create` (8 mods). Add `vanilla-1.21.4-copy` as a duplicate to exercise profile reuse by two instances.
- **Instances**: three, binding the above — plus one extra instance sharing an existing profile against the archived world, so the UI can be demonstrated reusing a profile.

`createServer` splits into `createWorld`/`createProfile`/`createInstance`; the `worldVolume` string, the "World volume mounted at…" console line, and the property-write-backs into `status.world.difficulty` / `players.max` are all deleted — those mutations are precisely what the decoupling removes.

---

## 8. Sequencing

1. **Types + client + queries** (`types.ts`, `client.ts`, `queries.ts`) — the contract, with demo fallback kept working so the UI never breaks mid-refactor.
2. **Demo backend re-cut** (`demo.ts`) with detached world + shared profile fixtures.
3. **Navigation + shell** (`server-navigation.tsx`, `index.tsx`) — three sidebar sections, new search params.
4. **New asset pages** (worlds, world-detail, profiles, profile-detail) by moving existing sections out of dashboard/settings/backups/mods/properties.
5. **Adapt remaining pages** (dashboard, console, properties, players; retire backups/mods/settings as standalone).
6. **Create-instance wizard** replacing the create-server dialog.
7. **Copy pass** — every string asserting the old coupling ("mounts a fresh world volume", "Deleting removes the container and the world volume", "Writes go back to the world volume", "The world volume at {path} is deleted") must be rewritten, since they now describe the wrong architecture.

The demo backend keeps the whole thing explorable at every step: the `/health` probe falls back to in-memory data, so each layer can be landed and viewed before the next.
