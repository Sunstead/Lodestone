/**
 * Wire types for the Lodestone backend (Rust + Axum).
 *
 * Every shape in here mirrors a JSON body or SSE payload the API serves.
 * The frontend never talks to Docker directly: it talks to these endpoints,
 * and the Rust side owns the containers, volumes and SQLite metadata.
 *
 * Lodestone keeps three concepts apart:
 *
 *   World    persistent, independent asset (region/level data on disk)
 *   Profile  software, version, mods, settings, resource limits
 *   Instance a profile + a world running in one isolated Docker container
 *
 * Deleting an instance never loses a world or a profile. A world can be
 * re-attached to any compatible profile, and a profile can be reused by
 * many instances.
 *
 * Endpoint map (all under `/api/v1`):
 *
 *   GET    /health
 *
 *   GET    /worlds                                 -> World[]
 *   POST   /worlds                                 -> World
 *   GET    /worlds/:id                             -> World
 *   PATCH  /worlds/:id                             -> World
 *   DELETE /worlds/:id                             -> 204
 *   POST   /worlds/:id/duplicate                   -> World
 *   GET    /worlds/:id/status                      -> WorldStatus          (polled)
 *   GET    /worlds/:id/backups                     -> BackupIndex
 *   POST   /worlds/:id/backups                     -> Backup
 *   POST   /worlds/:id/backups/:backupId/restore   -> 202
 *   DELETE /worlds/:id/backups/:backupId           -> 204
 *   PATCH  /worlds/:id/backups/schedule            -> BackupIndex
 *
 *   GET    /profiles                               -> Profile[]
 *   POST   /profiles                               -> Profile
 *   GET    /profiles/software                      -> SoftwareCatalog
 *   GET    /profiles/:id                           -> Profile
 *   PATCH  /profiles/:id                           -> Profile
 *   DELETE /profiles/:id                           -> 204
 *   POST   /profiles/:id/duplicate                 -> Profile
 *   GET    /profiles/:id/properties                -> PropertyFile
 *   PUT    /profiles/:id/properties                -> PropertyFile
 *   GET    /profiles/:id/mods                      -> ModIndex
 *   POST   /profiles/:id/mods                      -> { modId }
 *   PATCH  /profiles/:id/mods/:modId               -> { enabled }
 *   POST   /profiles/:id/mods/:modId/update        -> ModIndex
 *   DELETE /profiles/:id/mods/:modId               -> 204
 *
 *   GET    /instances                              -> Instance[]
 *   POST   /instances                              -> Instance
 *   GET    /instances/:id                          -> Instance
 *   PATCH  /instances/:id                          -> Instance
 *   DELETE /instances/:id                          -> 204
 *   POST   /instances/:id/world                    -> { worldId } -> Instance
 *   POST   /instances/:id/actions                  -> { action: start|stop|restart }
 *   GET    /instances/:id/status                   -> InstanceStatus       (polled)
 *   GET    /instances/:id/console?tail=500         -> ConsoleLine[]
 *   GET    /instances/:id/console/stream           -> SSE: `console`, `status`
 *   POST   /instances/:id/console                  -> { command } -> ConsoleLine[]
 *   GET    /instances/:id/players                  -> ServerPlayers
 *   POST   /instances/:id/players                  -> { action, name, reason? }
 */

export type ApiMode = 'live' | 'demo';

export type ServerState = 'starting' | 'running' | 'stopping' | 'stopped' | 'error';

export type ServerSoftwareId = 'vanilla' | 'paper' | 'fabric' | 'forge' | 'neoforge';

export type ServerAction = 'start' | 'stop' | 'restart';

export type GameMode = 'survival' | 'creative' | 'adventure' | 'spectator';
export type Difficulty = 'peaceful' | 'easy' | 'normal' | 'hard';
export type LevelType = 'minecraft:normal' | 'minecraft:flat' | 'minecraft:large_biomes' | 'minecraft:amplified' | 'minecraft:single_biome_surface';

/* -------------------------------------------------------------------------- */
/* Worlds                                                                     */
/* -------------------------------------------------------------------------- */

/**
 * A persistent world. It owns the region/level data on disk plus the
 * generation and gameplay policy that belongs to that world alone.
 * `attachedInstanceId` is the 1:1 attachment: a world is mounted by at
 * most one running instance at a time.
 */
export interface World {
  id: string;
  /** User-facing label, e.g. `Survival Season 2`. */
  name: string;
  /** Resolved `level-name` on disk, e.g. `world`. */
  folder: string;
  /** Generation-only: fixed once the world has generated. */
  levelType: LevelType;
  /** Generation-only: `null` means random. */
  seed: string | null;
  /** Generation-only. */
  generateStructures: boolean;
  /** Whether the world has ever generated; locks the fields above. */
  generated: boolean;
  maxWorldSize: number;
  allowNether: boolean;
  difficulty: Difficulty;
  gamemode: GameMode;
  hardcore: boolean;
  spawnProtection: number;
  spawnMonsters: boolean;
  spawnNpcs: boolean;
  pvp: boolean;
  allowFlight: boolean;
  enableCommandBlock: boolean;
  dataPacks: { enabled: string[]; disabled: string[] };
  sizeBytes: number;
  createdAt: string;
  updatedAt: string;
  /** Instance currently mounting this world, if any. */
  attachedInstanceId: string | null;
}

/** World metrics, read for the world itself rather than any container. */
export interface WorldStatus {
  worldId: string;
  /** True while an instance has this world mounted. */
  attached: boolean;
  diskBytes: number;
  chunks: number;
  entities: number;
  blockEntities: number;
  day: number;
  timeOfDay: string;
  dimensions: { nether: boolean; end: boolean };
  /** People who have played this world, across every instance that ran it. */
  playerCount: number;
  lastPlayedAt: string | null;
}

/* -------------------------------------------------------------------------- */
/* Profiles                                                                   */
/* -------------------------------------------------------------------------- */

export interface ProfileMemory {
  minMb: number;
  maxMb: number;
  /** `-Xms`: the initial heap. */
  initialMb: number;
}

/**
 * A reusable server configuration. Profiles carry the software, version,
 * mods, `server.properties` that belong to the software, and the resource
 * limits. Many instances may run the same profile.
 */
export interface Profile {
  id: string;
  name: string;
  description: string;
  softwareId: ServerSoftwareId;
  /** Human label, e.g. `Paper`. */
  software: string;
  /** Minecraft version, e.g. `1.21.4`. */
  version: string;
  /** Derived image tag, e.g. `sunstead/minecraft:paper-1.21.4`. */
  image: string;
  memory: ProfileMemory;
  /** Container CPU limit; `null` means unbounded. */
  cpuCores: number | null;
  javaArgs: string;
  autoStart: boolean;
  restartOnCrash: boolean;
  crashThreshold: number;
  /** Default RCON/query ports; an instance may override the Minecraft port. */
  ports: { rcon: number; query: number | null };
  createdAt: string;
  updatedAt: string;
  /** Instances currently using this profile, so the UI can guard deletion. */
  instanceCount: number;
}

export interface SoftwareVersion {
  version: string;
  image: string;
}

export interface SoftwareOption {
  id: ServerSoftwareId;
  /** Human label, e.g. `Paper`. */
  label: string;
  /** Short note about what the loader is for. */
  blurb: string;
  /** Whether this software takes plugins, mods, or neither. */
  kind: 'plugins' | 'mods' | 'vanilla';
  versions: SoftwareVersion[];
}

export interface SoftwareCatalog {
  software: SoftwareOption[];
}

/* -------------------------------------------------------------------------- */
/* Instances                                                                  */
/* -------------------------------------------------------------------------- */

/**
 * A profile bound to a world, running in one container. The `profile` and
 * `world` fields are denormalised so the sidebar and hero can render a name
 * and version without a waterfall of extra fetches.
 */
export interface Instance {
  id: string;
  name: string;
  profileId: string;
  worldId: string;
  state: ServerState;
  players: { online: number; max: number };
  address: string;
  ports: { minecraft: number; rcon: number; query: number | null };
  environment: Record<string, string>;
  createdAt: string;
  profile: { name: string; software: string; version: string };
  world: { name: string; folder: string };
}

/** Container and host metrics for a running instance. World data lives on WorldStatus. */
export interface InstanceStatus {
  state: ServerState;
  startedAt: string | null;
  uptimeSeconds: number | null;
  players: { online: number; max: number };
  memory: { usedBytes: number; limitBytes: number };
  cpuPercent: number;
  /** Ticks per second, 20.0 is healthy. */
  tps: number;
  /** Average time spent inside a tick, budget is 50 ms. */
  tickMs: number;
  disk: { usedBytes: number; totalBytes: number };
  network: { rxPerSec: number; txPerSec: number; rxTotal: number; txTotal: number; dropped: number };
  host: {
    cpuPercent: number;
    cpuCores: number;
    diskUsedBytes: number;
    diskTotalBytes: number;
    swapUsedBytes: number;
    swapTotalBytes: number;
  };
  container: { id: string; name: string; image: string; healthy: boolean };
}

export type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

export interface ConsoleLine {
  /** Monotonic id so the client can de-duplicate replays. */
  id: string;
  /** Unix milliseconds. */
  ts: number;
  level: LogLevel;
  message: string;
  /** Where the line came from: the server itself or Lodestone's own events. */
  source: 'server' | 'lodestone' | 'rcon';
}

/* -------------------------------------------------------------------------- */
/* server.properties                                                          */
/* -------------------------------------------------------------------------- */

/**
 * Which asset owns a property key. The editor only offers a key on the
 * asset that owns it, so a world key can never be edited on a profile.
 */
export type PropertyScope = 'world' | 'profile' | 'instance';

export type PropertyControl =
  | 'text'
  | 'number'
  | 'switch'
  | 'select'
  | 'slider'
  | 'password'
  | 'textarea';

export interface PropertyField {
  /** Key exactly as written in `server.properties`. */
  key: string;
  label: string;
  description: string;
  scope: PropertyScope;
  /** Fieldset within a scope, e.g. `network` on a profile. */
  group: string;
  control: PropertyControl;
  /** Always the raw string form, matching the file on disk. */
  value: string;
  defaultValue: string;
  options?: { value: string; label: string }[];
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
  placeholder?: string;
  /** Value is a secret (rcon.password) and must not be echoed to logs. */
  sensitive?: boolean;
  /** Saving applies immediately (the key is honoured at runtime). */
  appliesLive?: boolean;
  /**
   * Set when the key is derived from another asset and is not editable
   * here, e.g. `level-name` mirrors a world's `folder`.
   */
  derivedFrom?: string;
  readOnly?: boolean;
}

export interface PropertyFile {
  /** Path on the host, shown so admins know what is being written. */
  path: string;
  updatedAt: string;
  fields: PropertyField[];
}

/* -------------------------------------------------------------------------- */
/* Players                                                                    */
/* -------------------------------------------------------------------------- */

export interface Player {
  uuid: string;
  name: string;
  online: boolean;
  pingMs: number | null;
  playtimeSeconds: number;
  dimension: 'overworld' | 'nether' | 'end' | null;
  op: boolean;
  whitelisted: boolean;
  firstSeen: string;
  lastSeen: string;
}

export interface BanEntry {
  uuid: string | null;
  name: string;
  reason: string;
  bannedBy: string;
  createdAt: string;
  expiresAt: string | null;
}

export interface ServerPlayers {
  players: Player[];
  whitelistEnabled: boolean;
  whitelistEnforced: boolean;
  bans: BanEntry[];
}

export type PlayerAction =
  | { action: 'kick'; name: string; reason?: string }
  | { action: 'op' | 'deop'; name: string }
  | { action: 'whitelist-add' | 'whitelist-remove'; name: string }
  | { action: 'ban' | 'pardon'; name: string; reason?: string };

/* -------------------------------------------------------------------------- */
/* Backups (owned by a world)                                                 */
/* -------------------------------------------------------------------------- */

export type BackupKind = 'manual' | 'scheduled';
export type BackupStatus = 'complete' | 'running' | 'failed';

export interface Backup {
  id: string;
  /** The world this archive belongs to. */
  worldId: string;
  name: string;
  kind: BackupKind;
  createdAt: string;
  sizeBytes: number;
  status: BackupStatus;
  /** The instance that produced it, for provenance. */
  instanceId: string | null;
  note?: string;
}

export interface BackupSchedule {
  enabled: boolean;
  /** Minutes between runs. */
  intervalMinutes: number;
  retention: number;
  lastRunAt: string | null;
  nextRunAt: string | null;
}

export interface BackupStorage {
  path: string;
  usedBytes: number;
  quotaBytes: number;
}

export interface BackupIndex {
  backups: Backup[];
  schedule: BackupSchedule;
  storage: BackupStorage;
}

/* -------------------------------------------------------------------------- */
/* Mods (owned by a profile)                                                  */
/* -------------------------------------------------------------------------- */

export type ModKind = 'plugin' | 'mod';
export type ModSource = 'modrinth' | 'spigotmc' | 'curseforge' | 'hangar' | 'built-in';

export interface ModEntry {
  id: string;
  name: string;
  version: string;
  latestVersion?: string;
  kind: ModKind;
  source: ModSource;
  author: string;
  description: string;
  sizeBytes: number;
  downloads: number;
  enabled: boolean;
  updateAvailable?: boolean;
}

export interface CatalogEntry {
  id: string;
  name: string;
  author: string;
  description: string;
  kind: ModKind;
  source: ModSource;
  latestVersion: string;
  sizeBytes: number;
  downloads: number;
  categories: string[];
}

export interface ModIndex {
  installed: ModEntry[];
  catalog: CatalogEntry[];
  /** Loader the profile runs, catalog is filtered to it. */
  loader: string;
}

/* -------------------------------------------------------------------------- */
/* Create requests                                                            */
/* -------------------------------------------------------------------------- */

export interface CreateWorldRequest {
  name: string;
  levelType: LevelType;
  seed: string | null;
  generateStructures: boolean;
  difficulty: Difficulty;
  gamemode: GameMode;
  hardcore: boolean;
  maxWorldSize: number;
  allowNether: boolean;
}

export interface CreateProfileRequest {
  name: string;
  description: string;
  softwareId: ServerSoftwareId;
  version: string;
  memory: ProfileMemory;
  cpuCores: number | null;
  javaArgs: string;
  autoStart: boolean;
  restartOnCrash: boolean;
  crashThreshold: number;
}

export interface CreateInstanceRequest {
  name: string;
  profileId: string;
  worldId: string;
  minecraftPort: number;
  rconPort: number;
  queryPort: number | null;
}
