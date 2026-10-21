/**
 * In-memory stand-in for the Rust backend.
 *
 * The client probes `/api/v1/health` once; while that fails (no backend
 * running yet) every request lands here instead. When the Axum service
 * comes up the same calls go over HTTP/SSE with no UI changes.
 *
 * Three stores mirror the three assets:
 *
 *   worlds     persistent regions + world-scoped gameplay policy
 *   profiles   software, mods, resource limits, profile-scoped properties
 *   instances  a profile bound to a world, running in a container
 *
 * The store is deliberately stateful: lifecycle actions move instances
 * through starting/running/stopping, the console streams on a ticker and
 * metrics drift so the dashboard reads like a live server.
 */

import type {
  Backup,
  BackupIndex,
  BanEntry,
  CatalogEntry,
  ConsoleLine,
  CreateInstanceRequest,
  CreateProfileRequest,
  CreateWorldRequest,
  Difficulty,
  GameMode,
  Instance,
  InstanceStatus,
  LevelType,
  LogLevel,
  ModEntry,
  ModIndex,
  Player,
  Profile,
  PropertyField,
  PropertyScope,
  ServerAction,
  ServerPlayers,
  ServerSoftwareId,
  ServerState,
  SoftwareCatalog,
  World,
  WorldStatus,
} from './types';

/* ------------------------------------------------------------------ *
 * helpers
 * ------------------------------------------------------------------ */

let seq = 0;
const uid = (prefix: string) => `${prefix}-${(++seq).toString(36)}${Date.now().toString(36).slice(-4)}`;

const rand = (min: number, max: number) => min + Math.random() * (max - min);
const int = (min: number, max: number) => Math.floor(rand(min, max + 1));
const pick = <T,>(items: readonly T[]): T => items[Math.floor(Math.random() * items.length)];
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));
const jitter = (value: number, amount: number, min: number, max: number) =>
  clamp(value + rand(-amount, amount), min, max);

const HOUR = 3600_000;
const DAY = 24 * HOUR;
const iso = (offsetMs = 0) => new Date(Date.now() + offsetMs).toISOString();

const slugify = (value: string): string => {
  const slug = value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 32);
  return slug || 'world';
};

const SOFTWARE_LABEL: Record<ServerSoftwareId, string> = {
  vanilla: 'Vanilla',
  paper: 'Paper',
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
};

/* ------------------------------------------------------------------ *
 * server.properties — one table, scoped by owner
 *
 * A key belongs to exactly one asset. `level-name` is the world's
 * folder, so it is never typed by hand; `server-port` is published by a
 * container, so it is instance-scoped.
 * ------------------------------------------------------------------ */

type FieldDef = Omit<PropertyField, 'value'>;

const text = (
  key: string,
  label: string,
  scope: PropertyScope,
  group: string,
  description: string,
  defaultValue: string,
  extra: Partial<FieldDef> = {},
): FieldDef => ({ key, label, scope, group, description, defaultValue, control: 'text', ...extra });

const num = (
  key: string,
  label: string,
  scope: PropertyScope,
  group: string,
  description: string,
  defaultValue: string,
  extra: Partial<FieldDef> = {},
): FieldDef => ({ key, label, scope, group, description, defaultValue, control: 'number', ...extra });

const toggle = (
  key: string,
  label: string,
  scope: PropertyScope,
  group: string,
  description: string,
  defaultValue: 'true' | 'false',
  extra: Partial<FieldDef> = {},
): FieldDef => ({ key, label, scope, group, description, defaultValue, control: 'switch', ...extra });

const choice = (
  key: string,
  label: string,
  scope: PropertyScope,
  group: string,
  description: string,
  defaultValue: string,
  options: { value: string; label: string }[],
  extra: Partial<FieldDef> = {},
): FieldDef => ({ key, label, scope, group, description, defaultValue, control: 'select', options, ...extra });

const slider = (
  key: string,
  label: string,
  scope: PropertyScope,
  group: string,
  description: string,
  defaultValue: string,
  min: number,
  max: number,
  unit?: string,
  extra: Partial<FieldDef> = {},
): FieldDef => ({
  key,
  label,
  scope,
  group,
  description,
  defaultValue,
  control: 'slider',
  min,
  max,
  step: 1,
  unit,
  ...extra,
});

const GAME_MODES = [
  { value: 'survival', label: 'Survival' },
  { value: 'creative', label: 'Creative' },
  { value: 'adventure', label: 'Adventure' },
  { value: 'spectator', label: 'Spectator' },
];

const DIFFICULTIES = [
  { value: 'peaceful', label: 'Peaceful' },
  { value: 'easy', label: 'Easy' },
  { value: 'normal', label: 'Normal' },
  { value: 'hard', label: 'Hard' },
];

/**
 * World-scoped keys. Generation keys lock once the world has generated,
 * which the world editor enforces by disabling them.
 */
const WORLD_DEFS: FieldDef[] = [
  text('level-name', 'World folder', 'world', 'identity', 'Folder name on disk. Taken from the world asset — change it by renaming the world.', 'world', {
    readOnly: true,
    derivedFrom: 'world.folder',
  }),
  text('level-seed', 'World seed', 'world', 'generation', 'Used only the first time the world generates.', '', {
    placeholder: 'blank = random',
  }),
  choice('level-type', 'World type', 'world', 'generation', 'Generation preset applied to a brand new world.', 'minecraft:normal', [
    { value: 'minecraft:normal', label: 'Default' },
    { value: 'minecraft:flat', label: 'Flat' },
    { value: 'minecraft:amplified', label: 'Amplified' },
    { value: 'minecraft:large_biomes', label: 'Large biomes' },
  ]),
  toggle('generate-structures', 'Generate structures', 'world', 'generation', 'Villages, strongholds, monuments and bastions.', 'true'),
  num('max-world-size', 'World border', 'world', 'generation', 'Diameter of the world border in blocks.', '29999984', {
    min: 1000,
    max: 29999984,
    unit: 'blocks',
  }),
  toggle('allow-nether', 'Allow Nether', 'world', 'dimensions', 'Create and permit travel to this world’s Nether.', 'true'),
  choice('difficulty', 'Difficulty', 'world', 'gameplay', 'Hostile mob damage, hunger and health regen.', 'normal', DIFFICULTIES, {
    appliesLive: true,
  }),
  choice('gamemode', 'Default gamemode', 'world', 'gameplay', 'Game mode given to players the first time they join.', 'survival', GAME_MODES, {
    appliesLive: true,
  }),
  toggle('hardcore', 'Hardcore', 'world', 'gameplay', 'Deaths ban the player and lock the world in spectator mode.', 'false'),
  toggle('force-gamemode', 'Force gamemode', 'world', 'gameplay', 'Reset every join to the default gamemode instead of restoring the last one.', 'false'),
  toggle('pvp', 'Player vs player', 'world', 'gameplay', 'Players may damage each other.', 'true'),
  toggle('allow-flight', 'Allow flight', 'world', 'gameplay', 'Tolerate clients that fly (Creative, elytra, modded). Off can trigger anticheat kicks.', 'false'),
  toggle('enable-command-block', 'Command blocks', 'world', 'gameplay', 'Allow command blocks to execute commands.', 'true'),
  num('spawn-protection', 'Spawn protection', 'world', 'spawn', 'Radius around spawn where only ops may break blocks. 0 turns it off.', '16', {
    min: 0,
    max: 1000,
    unit: 'blocks',
  }),
  toggle('spawn-monsters', 'Hostile mobs', 'world', 'spawn', 'Hostile mobs spawn in the dark.', 'true'),
  toggle('spawn-npcs', 'NPCs', 'world', 'spawn', 'Villagers and other NPCs spawn naturally.', 'true'),
  text('initial-enabled-packs', 'Enabled data packs', 'world', 'datapacks', 'Data packs enabled the first time the world loads.', 'vanilla'),
  text('initial-disabled-packs', 'Disabled data packs', 'world', 'datapacks', 'Data packs shipped in the world folder but left off.', ''),
];

/** Profile-scoped keys: software behaviour, access policy, tuning. */
const PROFILE_DEFS: FieldDef[] = [
  text('motd', 'Message of the day', 'profile', 'network', 'Shown in the multiplayer list. Supports § colour codes.', 'A Minecraft Server', {
    placeholder: '§aMy Server §7| §fWelcome',
  }),
  num('max-players', 'Max players', 'profile', 'network', 'Player slots advertised in the server list ping. Does not kick anyone already online.', '20', {
    min: 1,
    max: 1000,
  }),
  toggle('online-mode', 'Online mode', 'profile', 'network', 'Authenticate names against Mojang sessions. Disable only when a proxy handles auth.', 'true'),
  toggle('white-list', 'Whitelist', 'profile', 'access', 'Only whitelisted players may join. Manage the list in the Players tab.', 'false', {
    appliesLive: true,
  }),
  toggle('enforce-whitelist', 'Enforce whitelist', 'profile', 'access', 'Kick players who are removed from the whitelist while online.', 'false', {
    appliesLive: true,
  }),
  toggle('hide-online-players', 'Hide player list', 'profile', 'access', 'Do not reveal the online player list to server list pings.', 'false'),
  num('player-idle-timeout', 'Idle timeout', 'profile', 'access', 'Kick players idle for this long. 0 never kicks.', '0', {
    min: 0,
    max: 43200,
    unit: 'min',
    appliesLive: true,
  }),
  toggle('enable-query', 'Query protocol', 'profile', 'network', 'Answer GameSpy4 query pings on the query port (used by server list scrapers).', 'false'),
  num('query.port', 'Query port', 'profile', 'network', 'UDP port for the query protocol.', '25565', { min: 1, max: 65535 }),
  slider('view-distance', 'View distance', 'profile', 'performance', 'Chunks sent to each client. The heaviest lever for CPU and bandwidth.', '10', 2, 32, 'chunks'),
  slider('simulation-distance', 'Simulation distance', 'profile', 'performance', 'Chunks where mobs, crops and redstone actually tick.', '8', 2, 32, 'chunks'),
  num('max-tick-time', 'Tick watchdog', 'profile', 'performance', 'Warn when one tick runs longer than this. -1 disables the watchdog.', '60000', {
    min: -1,
    max: 600000,
    unit: 'ms',
  }),
  num('max-chained-neighbor-updates', 'Chained updates', 'profile', 'performance', 'Cap on block updates per tick, limits piston and observer loops.', '100000', {
    min: 1000,
    max: 1000000,
  }),
  num('network-compression-threshold', 'Compression threshold', 'profile', 'performance', 'Compress packets larger than this. 0 disables compression.', '256', {
    min: 0,
    max: 65536,
    unit: 'bytes',
  }),
  toggle('sync-chunk-writes', 'Sync chunk writes', 'profile', 'performance', 'Flush chunks to disk synchronously. Faster off, safer on.', 'true'),
  toggle('use-native-transport', 'Native transport', 'profile', 'performance', 'Linux only: use epoll for lower latency networking.', 'true'),
  num('rate-limit', 'Rate limit', 'profile', 'performance', 'Drop packets from clients sending faster than this. 0 disables it.', '0', {
    min: 0,
    max: 100000,
    unit: 'pkt/s',
  }),
  num('entity-broadcast-range-percentage', 'Broadcast range', 'profile', 'performance', 'How far entity updates reach clients. 100 matches the vanilla view radius.', '100', {
    min: 10,
    max: 1000,
    unit: '%',
  }),
  toggle('enable-rcon', 'Enable RCON', 'profile', 'rcon', 'Expose RCON so Lodestone can send commands and read responses.', 'true'),
  num('rcon.port', 'RCON port', 'profile', 'rcon', 'Published on loopback only — never reachable from the internet.', '25575', {
    min: 1,
    max: 65535,
  }),
  text('rcon.password', 'RCON password', 'profile', 'rcon', 'Shared with Lodestone and stored in SQLite. Never written to logs.', '', {
    control: 'password',
    sensitive: true,
    placeholder: 'auto-generated',
  }),
  toggle('broadcast-rcon-to-ops', 'Broadcast RCON commands', 'profile', 'rcon', 'Show commands sent over RCON to online ops.', 'true'),
  toggle('broadcast-console-to-ops', 'Broadcast console commands', 'profile', 'advanced', 'Show console commands to online ops.', 'true'),
  toggle('enforce-secure-profile', 'Chat signing', 'profile', 'advanced', 'Enforce Mojang chat signing. Unsigned messages are rejected.', 'true'),
  toggle('prevent-proxy-connections', 'Block proxies', 'profile', 'advanced', 'Reject connections from known proxy IP ranges.', 'false'),
  toggle('honor-proxy-ips', 'Honour proxy IPs', 'profile', 'advanced', 'Trust forwarded IPs from a proxy sitting in front of the server.', 'true'),
  toggle('log-ips', 'Log player IPs', 'profile', 'advanced', 'Write connecting addresses into the log file.', 'true'),
  toggle('enable-jmx-monitoring', 'JMX monitoring', 'profile', 'advanced', 'Expose JVM metrics on localhost:9010.', 'false'),
  toggle('track-random-splits', 'Track random splits', 'profile', 'advanced', 'Log RNG splits so bug reports can be reproduced.', 'false'),
  text('text-filtering-config', 'Text filtering config', 'profile', 'advanced', 'File used to filter chat text. Blank disables filtering.', ''),
  text('bug-report-link', 'Bug report link', 'profile', 'advanced', 'Shown to players when they file a crash report.', ''),
  num('function-permission-level', 'Function permission level', 'profile', 'advanced', 'Permission level for functions and command block functions.', '2', {
    min: 0,
    max: 4,
  }),
  num('operator-user-permission-level', 'Op permission level', 'profile', 'advanced', 'Permission level granted when a player is made op in game.', '4', {
    min: 0,
    max: 4,
  }),
  toggle('require-resource-pack', 'Require resource pack', 'profile', 'advanced', 'Kick players who decline the resource pack.', 'false'),
  text('resource-pack', 'Resource pack URL', 'profile', 'advanced', 'HTTPS URL of the pack pushed to clients on join.', ''),
  text('resource-pack-prompt', 'Resource pack prompt', 'profile', 'advanced', 'Message shown when asking players to download the pack.', ''),
  text('resource-pack-sha1', 'Resource pack SHA-1', 'profile', 'advanced', 'Hash of the pack. Required for modern clients to accept it.', ''),
];

/** Instance-scoped: published by the container, not the world or profile. */
const INSTANCE_DEFS: FieldDef[] = [
  num('server-port', 'Server port', 'instance', 'network', 'TCP port players connect to. Mapped into the container.', '25565', {
    min: 1,
    max: 65535,
  }),
  text('server-ip', 'Bind address', 'instance', 'network', 'Interface the server binds to. Leave blank to bind every interface.', '', {
    placeholder: 'blank = all interfaces',
  }),
];

export const PROPERTY_DEFS: FieldDef[] = [...WORLD_DEFS, ...PROFILE_DEFS, ...INSTANCE_DEFS];

export const TYPE_GROUPS: Record<PropertyScope, { id: string; label: string; hint: string }[]> = {
  world: [
    { id: 'identity', label: 'Identity', hint: 'Folder on disk' },
    { id: 'generation', label: 'Generation', hint: 'Locked once generated' },
    { id: 'dimensions', label: 'Dimensions', hint: 'Nether and beyond' },
    { id: 'gameplay', label: 'Gameplay', hint: 'Rules for this world' },
    { id: 'spawn', label: 'Spawning', hint: 'Where things appear' },
    { id: 'datapacks', label: 'Data packs', hint: 'Per-world packs' },
  ],
  profile: [
    { id: 'network', label: 'Network', hint: 'Ports and motd' },
    { id: 'access', label: 'Access', hint: 'Whitelist and idling' },
    { id: 'performance', label: 'Performance', hint: 'Distance and tick load' },
    { id: 'rcon', label: 'RCON', hint: 'Remote console' },
    { id: 'advanced', label: 'Advanced', hint: 'Everything else' },
  ],
  instance: [{ id: 'network', label: 'Network', hint: 'Published ports' }],
};

const GROUP_LABEL = new Map<string, string>(
  Object.values(TYPE_GROUPS)
    .flat()
    .map((group) => [group.id, group.label]),
);

export const propertyGroupLabel = (group: string): string => GROUP_LABEL.get(group) ?? group;

function buildFields(overrides: Record<string, string> = {}): PropertyField[] {
  return PROPERTY_DEFS.map((def) => ({ ...def, value: overrides[def.key] ?? def.defaultValue }));
}

export function splitFields(fields: PropertyField[], scope: PropertyScope): PropertyField[] {
  return fields.filter((field) => field.scope === scope);
}

/* ------------------------------------------------------------------ *
 * content pools
 * ------------------------------------------------------------------ */

const SURVIVAL_ONLINE = [
  { name: 'Steve_Builds', ping: 24, playtime: 8040, dimension: 'overworld' as const, op: true },
  { name: 'CreeperSlayer', ping: 41, playtime: 6720, dimension: 'nether' as const, op: false },
  { name: 'redstone_rae', ping: 18, playtime: 3900, dimension: 'overworld' as const, op: false },
  { name: 'Alexandrite', ping: 77, playtime: 2880, dimension: 'end' as const, op: false },
  { name: 'mossy_cobble', ping: 33, playtime: 1860, dimension: 'overworld' as const, op: false },
  { name: 'DiamondDan', ping: 112, playtime: 720, dimension: 'overworld' as const, op: false },
  { name: 'pixelpilot', ping: 29, playtime: 240, dimension: 'nether' as const, op: false },
];

const SURVIVAL_OFFLINE = [
  { name: 'quarry_quinn', lastSeen: Date.now() - 2 * DAY, playtime: 411_000 },
  { name: 'birch_and_beans', lastSeen: Date.now() - 5 * HOUR, playtime: 88_200 },
  { name: 'tnt_lily', lastSeen: Date.now() - 26 * HOUR, playtime: 154_000, op: true },
  { name: 'cartographer_kay', lastSeen: Date.now() - 4 * DAY, playtime: 61_000 },
  { name: 'wither_sarah', lastSeen: Date.now() - 9 * DAY, playtime: 203_000 },
];

const MODDED_ONLINE = [
  { name: 'gearshift_gwen', ping: 46, playtime: 5400, dimension: 'overworld' as const, op: true },
  { name: 'copper_cowboy', ping: 61, playtime: 3120, dimension: 'nether' as const, op: false },
  { name: 'tinker_tom', ping: 38, playtime: 1200, dimension: 'overworld' as const, op: false },
];

const MODDED_OFFLINE = [
  { name: 'brass_bandit', lastSeen: Date.now() - 3 * HOUR, playtime: 40_500 },
  { name: 'cogwheel_carl', lastSeen: Date.now() - 3 * DAY, playtime: 122_000 },
];

const CREATIVE_OFFLINE = [
  { name: 'pixel_pete', lastSeen: Date.now() - 6 * HOUR, playtime: 7400 },
  { name: 'buildmaster_bob', lastSeen: Date.now() - 3 * DAY, playtime: 31_000 },
  { name: 'luma_builds', lastSeen: Date.now() - 11 * HOUR, playtime: 9600 },
  { name: 'grid_gremlin', lastSeen: Date.now() - 8 * DAY, playtime: 22_000 },
];

const ADVANCEMENTS = [
  'Diamonds!',
  'A Terrible Fortress',
  'Free the End',
  'Two by Two',
  'A Balanced Diet',
  'Cover Me in Debris',
  'Bees in a Bucket? No, Bees in a Bucket',
];

const DEATHS = [
  '{name} was blown up by Creeper',
  '{name} fell from a high place',
  '{name} tried to swim in lava',
  '{name} was shot by a Skeleton',
  '{name} fell out of the water',
  '{name} was killed by Enderman',
];

const AMBIENT_LINES = [
  'Saving the game (this may take a moment!)',
  'Saved the game',
  'ThreadedAnvilChunkStorage (world): All chunks are saved',
  'Preparing start region for dimension [minecraft:overworld]',
  'Preparing spawn area: 47%',
  'Time elapsed: 1024 ms',
  '[Server thread/WARN]: Can\'t keep up! Is the server overloaded? Running 2312ms behind, skipping 46 ticks',
  '[Server thread/DEBUG]: Failed to load chunk at [-312, 88]: region file r.-10.2.mca is truncated',
];

function playerLine(kind: 'join' | 'quit', name: string): string {
  return kind === 'join' ? `${name} joined the game` : `${name} left the game`;
}

/* ------------------------------------------------------------------ *
 * stores
 * ------------------------------------------------------------------ */

/**
 * An instance bundles its own runtime. Everything about the world and
 * profile it uses is looked up by id, so deleting the instance never
 * touches either asset.
 */
interface DemoInstance {
  instance: Instance;
  status: InstanceStatus;
  players: ServerPlayers;
  console: ConsoleLine[];
  listeners: Set<(line: ConsoleLine) => void>;
  ticker: ReturnType<typeof setInterval> | null;
  timer: ReturnType<typeof setTimeout> | null;
}

interface DemoProfile {
  profile: Profile;
  properties: PropertyField[];
  mods: ModIndex;
}

interface DemoWorld {
  world: World;
  status: WorldStatus;
  backups: BackupIndex;
  /** World-scoped property values, kept beside the world asset. */
  properties: Record<string, string>;
}

const instances = new Map<string, DemoInstance>();
const profiles = new Map<string, DemoProfile>();
const worlds = new Map<string, DemoWorld>();

const uniqueId = (base: string, taken: (id: string) => boolean): string => {
  let id = slugify(base);
  let suffix = 2;
  while (taken(id)) id = `${slugify(base)}-${suffix++}`;
  return id;
};

/* ------------------------------------------------------------------ *
 * builders
 * ------------------------------------------------------------------ */

function makePlayers(
  online: { name: string; ping: number; playtime: number; dimension: Player['dimension']; op?: boolean }[],
  offline: { name: string; lastSeen: number; playtime: number; op?: boolean }[],
  whitelisted: string[],
): ServerPlayers {
  const uuid = (name: string) =>
    [...name].reduce((hash, char) => hash * 31 + char.charCodeAt(0), 7).toString(16).padStart(8, '0') +
    '-0000-4000-8000-' +
    [...name].reduce((hash, char) => hash * 33 + char.charCodeAt(0), 11).toString(16).padStart(12, '0');

  const players: Player[] = [
    ...online.map((entry) => ({
      uuid: uuid(entry.name),
      name: entry.name,
      online: true,
      pingMs: entry.ping,
      playtimeSeconds: entry.playtime,
      dimension: entry.dimension,
      op: entry.op ?? false,
      whitelisted: whitelisted.includes(entry.name),
      firstSeen: iso(-120 * DAY),
      lastSeen: iso(),
    })),
    ...offline.map((entry) => ({
      uuid: uuid(entry.name),
      name: entry.name,
      online: false,
      pingMs: null,
      playtimeSeconds: entry.playtime,
      dimension: null,
      op: entry.op ?? false,
      whitelisted: whitelisted.includes(entry.name),
      firstSeen: iso(-200 * DAY),
      lastSeen: iso(-Date.now() + entry.lastSeen),
    })),
  ];

  return { players, whitelistEnabled: whitelisted.length > 0, whitelistEnforced: false, bans: [] };
}

function makeStatus(partial: Partial<InstanceStatus> & { state: ServerState }): InstanceStatus {
  return {
    startedAt: null,
    uptimeSeconds: null,
    players: { online: 0, max: 20 },
    memory: { usedBytes: 0, limitBytes: 8 * 1024 ** 3 },
    cpuPercent: 0,
    tps: 0,
    tickMs: 0,
    disk: { usedBytes: 41.6 * 1024 ** 3, totalBytes: 80 * 1024 ** 3 },
    network: { rxPerSec: 0, txPerSec: 0, rxTotal: 9.4 * 1024 ** 3, txTotal: 38.1 * 1024 ** 3, dropped: 0.02 },
    host: {
      cpuPercent: 21,
      cpuCores: 4,
      diskUsedBytes: 41.6 * 1024 ** 3,
      diskTotalBytes: 80 * 1024 ** 3,
      swapUsedBytes: 0.2 * 1024 ** 3,
      swapTotalBytes: 2 * 1024 ** 3,
    },
    container: { id: '—', name: '—', image: '—', healthy: false },
    ...partial,
  };
}

function backup(
  name: string,
  worldId: string,
  kind: Backup['kind'],
  ageMs: number,
  sizeGb: number,
  status: Backup['status'] = 'complete',
  instanceId: string | null = null,
): Backup {
  return {
    id: uid('bk'),
    worldId,
    name,
    kind,
    createdAt: iso(-ageMs),
    sizeBytes: Math.round(sizeGb * 1024 ** 3),
    status,
    instanceId,
  };
}

function plugin(
  id: string,
  name: string,
  version: string,
  source: ModEntry['source'],
  author: string,
  description: string,
  sizeKb: number,
  downloads: number,
  extra: Partial<ModEntry> = {},
): ModEntry {
  return {
    id,
    name,
    version,
    kind: 'plugin',
    source,
    author,
    description,
    sizeBytes: sizeKb * 1024,
    downloads,
    enabled: true,
    ...extra,
  };
}

function mod(
  id: string,
  name: string,
  version: string,
  source: ModEntry['source'],
  author: string,
  description: string,
  sizeKb: number,
  downloads: number,
  extra: Partial<ModEntry> = {},
): ModEntry {
  return {
    id,
    name,
    version,
    kind: 'mod',
    source,
    author,
    description,
    sizeBytes: sizeKb * 1024,
    downloads,
    enabled: true,
    ...extra,
  };
}

function catalogEntry(
  id: string,
  name: string,
  author: string,
  description: string,
  kind: ModEntry['kind'],
  source: ModEntry['source'],
  version: string,
  sizeKb: number,
  downloads: number,
  categories: string[],
): CatalogEntry {
  return {
    id,
    name,
    author,
    description,
    kind,
    source,
    latestVersion: version,
    sizeBytes: sizeKb * 1024,
    downloads,
    categories,
  };
}

/* ------------------------------------------------------------------ *
 * seed data — worlds
 * ------------------------------------------------------------------ */

interface WorldSeed {
  id: string;
  name: string;
  folder: string;
  levelType: LevelType;
  seed: string | null;
  generateStructures: boolean;
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
  status: {
    chunks: number;
    entities: number;
    blockEntities: number;
    day: number;
    timeOfDay: string;
    playerCount: number;
    lastPlayedAt: string | null;
  };
  backups: BackupIndex;
}

const WORLD_SEEDS: WorldSeed[] = [
  {
    id: 'survival-season-2',
    name: 'Survival Season 2',
    folder: 'world',
    levelType: 'minecraft:normal',
    seed: '4839201772',
    generateStructures: true,
    generated: true,
    maxWorldSize: 29999984,
    allowNether: true,
    difficulty: 'hard',
    gamemode: 'survival',
    hardcore: false,
    spawnProtection: 16,
    spawnMonsters: true,
    spawnNpcs: true,
    pvp: true,
    allowFlight: false,
    enableCommandBlock: true,
    dataPacks: { enabled: ['vanilla'], disabled: [] },
    sizeBytes: 6.3 * 1024 ** 3,
    createdAt: iso(-63 * DAY),
    updatedAt: iso(-20 * 60_000),
    status: {
      chunks: 1248,
      entities: 3917,
      blockEntities: 642,
      day: 412,
      timeOfDay: '14:20',
      playerCount: 12,
      lastPlayedAt: iso(-4 * 60_000),
    },
    backups: {
      backups: [
        backup('Scheduled', 'survival-season-2', 'scheduled', 42 * 60_000, 1.82, 'running', 'survival-1'),
        backup('Scheduled', 'survival-season-2', 'scheduled', 6 * HOUR, 1.81, 'complete', 'survival-1'),
        backup('Manual', 'survival-season-2', 'manual', 17 * HOUR, 1.79, 'complete', 'survival-1'),
        backup('Scheduled', 'survival-season-2', 'scheduled', 20 * HOUR, 1.78, 'complete', 'survival-1'),
        backup('Scheduled', 'survival-season-2', 'scheduled', 30 * HOUR, 1.77, 'complete', 'survival-1'),
        backup('Before Paper 1.21.4', 'survival-season-2', 'manual', 3 * DAY, 1.71, 'complete', 'survival-1'),
        backup('Scheduled', 'survival-season-2', 'scheduled', 3 * DAY + 6 * HOUR, 1.7, 'complete', 'survival-1'),
        backup('Scheduled', 'survival-season-2', 'scheduled', 4 * DAY, 1.68, 'complete', 'survival-1'),
      ],
      schedule: {
        enabled: true,
        intervalMinutes: 360,
        retention: 28,
        lastRunAt: iso(-6 * HOUR),
        nextRunAt: iso(0),
      },
      storage: {
        path: '/var/lib/lodestone/backups/survival-season-2',
        usedBytes: 12.9 * 1024 ** 3,
        quotaBytes: 50 * 1024 ** 3,
      },
    },
  },
  {
    id: 'flat-test',
    name: 'Flat Test',
    folder: 'flat_test',
    levelType: 'minecraft:flat',
    seed: null,
    generateStructures: false,
    generated: true,
    maxWorldSize: 29999984,
    allowNether: false,
    difficulty: 'peaceful',
    gamemode: 'creative',
    hardcore: false,
    spawnProtection: 0,
    spawnMonsters: false,
    spawnNpcs: false,
    pvp: false,
    allowFlight: true,
    enableCommandBlock: true,
    dataPacks: { enabled: ['vanilla'], disabled: [] },
    sizeBytes: 120 * 1024 ** 2,
    createdAt: iso(-41 * DAY),
    updatedAt: iso(-2 * DAY),
    status: {
      chunks: 289,
      entities: 84,
      blockEntities: 1203,
      day: 58,
      timeOfDay: '06:00',
      playerCount: 4,
      lastPlayedAt: iso(-2 * DAY),
    },
    backups: {
      backups: [
        backup('Manual', 'flat-test', 'manual', 2 * DAY, 0.42, 'complete', 'creative-1'),
        backup('Scheduled', 'flat-test', 'scheduled', 3 * DAY, 0.41, 'complete', 'creative-1'),
        backup('Scheduled', 'flat-test', 'scheduled', 4 * DAY, 0.4, 'complete', 'creative-1'),
      ],
      schedule: {
        enabled: false,
        intervalMinutes: 1440,
        retention: 10,
        lastRunAt: iso(-3 * DAY),
        nextRunAt: null,
      },
      storage: {
        path: '/var/lib/lodestone/backups/flat-test',
        usedBytes: 2.1 * 1024 ** 3,
        quotaBytes: 50 * 1024 ** 3,
      },
    },
  },
  {
    id: 'create-world',
    name: 'Create World',
    folder: 'create_world',
    levelType: 'minecraft:normal',
    seed: '9912043587',
    generateStructures: true,
    generated: true,
    maxWorldSize: 29999984,
    allowNether: true,
    difficulty: 'normal',
    gamemode: 'survival',
    hardcore: false,
    spawnProtection: 0,
    spawnMonsters: true,
    spawnNpcs: true,
    pvp: true,
    allowFlight: true,
    enableCommandBlock: true,
    dataPacks: { enabled: ['vanilla'], disabled: [] },
    sizeBytes: 2.1 * 1024 ** 3,
    createdAt: iso(-19 * DAY),
    updatedAt: iso(-30 * 60_000),
    status: {
      chunks: 2140,
      entities: 6402,
      blockEntities: 1877,
      day: 96,
      timeOfDay: '21:45',
      playerCount: 5,
      lastPlayedAt: iso(-12 * 60_000),
    },
    backups: {
      backups: [
        backup('Scheduled', 'create-world', 'scheduled', 3 * HOUR, 3.4, 'complete', 'modded-1'),
        backup('Scheduled', 'create-world', 'scheduled', 9 * HOUR, 3.38, 'complete', 'modded-1'),
        backup('Manual', 'create-world', 'manual', 22 * HOUR, 3.31, 'complete', 'modded-1'),
        backup('Scheduled', 'create-world', 'scheduled', 27 * HOUR, 3.3, 'complete', 'modded-1'),
        backup('Before Create 6.0', 'create-world', 'manual', 2 * DAY, 2.9, 'failed', 'modded-1'),
      ],
      schedule: {
        enabled: true,
        intervalMinutes: 720,
        retention: 14,
        lastRunAt: iso(-3 * HOUR),
        nextRunAt: iso(6 * HOUR),
      },
      storage: {
        path: '/var/lib/lodestone/backups/create-world',
        usedBytes: 6.4 * 1024 ** 3,
        quotaBytes: 50 * 1024 ** 3,
      },
    },
  },
  {
    // Deliberately detached: proves a world outlives every container
    // that ran it, and exercises the "free" badge in the sidebar.
    id: 'archived-nether-test',
    name: 'Archived Nether Test',
    folder: 'nether_test',
    levelType: 'minecraft:normal',
    seed: '1122334455',
    generateStructures: true,
    generated: true,
    maxWorldSize: 29999984,
    allowNether: true,
    difficulty: 'easy',
    gamemode: 'survival',
    hardcore: false,
    spawnProtection: 0,
    spawnMonsters: true,
    spawnNpcs: true,
    pvp: true,
    allowFlight: false,
    enableCommandBlock: true,
    dataPacks: { enabled: ['vanilla'], disabled: [] },
    sizeBytes: 1.4 * 1024 ** 3,
    createdAt: iso(-120 * DAY),
    updatedAt: iso(-88 * DAY),
    status: {
      chunks: 0,
      entities: 0,
      blockEntities: 0,
      day: 233,
      timeOfDay: '—',
      playerCount: 3,
      lastPlayedAt: iso(-88 * DAY),
    },
    backups: {
      backups: [backup('Final archive', 'archived-nether-test', 'manual', 88 * DAY, 1.38)],
      schedule: {
        enabled: false,
        intervalMinutes: 1440,
        retention: 5,
        lastRunAt: iso(-88 * DAY),
        nextRunAt: null,
      },
      storage: {
        path: '/var/lib/lodestone/backups/archived-nether-test',
        usedBytes: 1.38 * 1024 ** 3,
        quotaBytes: 50 * 1024 ** 3,
      },
    },
  },
];

/* ------------------------------------------------------------------ *
 * seed data — profiles
 * ------------------------------------------------------------------ */

interface ProfileSeed {
  id: string;
  name: string;
  description: string;
  softwareId: ServerSoftwareId;
  version: string;
  memory: { minMb: number; maxMb: number; initialMb: number };
  cpuCores: number | null;
  javaArgs: string;
  autoStart: boolean;
  restartOnCrash: boolean;
  crashThreshold: number;
  ports: { rcon: number; query: number | null };
  createdAt: string;
  updatedAt: string;
  properties: Record<string, string>;
  mods: ModIndex;
}

const PAPER_PLUGIN_CATALOG: CatalogEntry[] = [
  catalogEntry('geyser', 'Geyser', 'GeyserMC', 'Let Bedrock players join this Java server.', 'plugin', 'modrinth', '2.4.3', 4800, 34_000_000, ['proxy', 'crossplay']),
  catalogEntry('floodgate', 'Floodgate', 'GeyserMC', 'Bedrock accounts join without a Java account.', 'plugin', 'modrinth', '2.2.3', 900, 21_000_000, ['proxy', 'crossplay']),
  catalogEntry('plan', 'Plan', 'Plan Player Analytics', 'Player activity dashboards inside the server.', 'plugin', 'spigotmc', '5.6.1', 6400, 4_300_000, ['analytics']),
  catalogEntry('worldguard', 'WorldGuard', 'EngineHub', 'Regions that protect builds from other players.', 'plugin', 'spigotmc', '7.0.10', 1100, 15_000_000, ['protection']),
  catalogEntry('clearlagg', 'ClearLag', 'Mesphito', 'Removes leftover entities on a timer.', 'plugin', 'spigotmc', '3.2.1', 140, 8_900_000, ['performance']),
  catalogEntry('tabtps', 'TabTPS', 'melopello', 'TPS and ping in the tab list.', 'plugin', 'modrinth', '1.5.12', 320, 2_700_000, ['performance', 'ui']),
];

const FABRIC_MOD_CATALOG: CatalogEntry[] = [
  catalogEntry('distanthorizons', 'Distant Horizons', 'Distant Horizons Team', 'Level of detail terrain far past the view distance.', 'mod', 'modrinth', '2.3.2-b', 18_400, 41_000_000, ['performance', 'world']),
  catalogEntry('modernfix', 'ModernFix', 'embeddedt', 'Hundreds of startup and memory fixes.', 'mod', 'modrinth', '5.20.0', 1900, 39_000_000, ['performance']),
  catalogEntry('entityculling', 'Entity Culling', 'tr7zw', 'Skip rendering entities you cannot see.', 'mod', 'modrinth', '1.7.4', 140, 62_000_000, ['performance']),
  catalogEntry('continuity', 'Continuity', 'PepperCode1', 'Connected textures without OptiFine.', 'mod', 'modrinth', '3.0.0', 320, 33_000_000, ['visual']),
  catalogEntry('emi', 'EMI', 'emi', 'Recipe viewer with a searchable index.', 'mod', 'modrinth', '1.1.22', 1400, 51_000_000, ['ui']),
  catalogEntry('xaerosminimap', "Xaero's Minimap", 'xaero96', 'Minimap with waypoints for every player.', 'mod', 'curseforge', '25.0.0', 2100, 120_000_000, ['ui']),
  catalogEntry('appleskin', 'AppleSkin', 'squeek502', 'Shows saturation and food values.', 'mod', 'modrinth', '3.0.6', 90, 110_000_000, ['ui']),
  catalogEntry('servux', 'Servux', 'Bloom', 'Debug helpers used by structure tools.', 'mod', 'modrinth', '0.4.0', 210, 4_600_000, ['utility']),
];

const PROFILE_SEEDS: ProfileSeed[] = [
  {
    id: 'paper-1-21-4-performance',
    name: 'Paper 1.21.4 · Performance',
    description: 'Whitelist SMP tuning. View distance 10, G1GC with a 200 ms pause target, daily RCON access.',
    softwareId: 'paper',
    version: '1.21.4',
    memory: { minMb: 2048, maxMb: 8192, initialMb: 4096 },
    cpuCores: 4,
    javaArgs: '-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC',
    autoStart: true,
    restartOnCrash: true,
    crashThreshold: 3,
    ports: { rcon: 25575, query: null },
    createdAt: iso(-63 * DAY),
    updatedAt: iso(-6 * HOUR),
    properties: {
      motd: '§aSunstead §7Survival §8| §fwhitelist on',
      'max-players': '20',
      'white-list': 'true',
      'enforce-whitelist': 'true',
      'view-distance': '10',
      'simulation-distance': '8',
      'rcon.port': '25575',
      'rcon.password': '7f3c1a9d5e2b4860',
      'entity-broadcast-range-percentage': '100',
    },
    mods: {
      loader: 'Paper 1.21.4',
      installed: [
        plugin('luckperms', 'LuckPerms', '5.4.145', 'modrinth', 'Luck', 'Permissions, groups and prefixes.', 1420, 92_400_000),
        plugin('essentialsx', 'EssentialsX', '2.21.0', 'spigotmc', 'Essentials Team', 'Homes, warps, kits, economy.', 2600, 41_000_000),
        plugin('vault', 'Vault', '1.7.3', 'spigotmc', 'MilkBowl', 'Economy and permissions bridge.', 260, 68_000_000),
        plugin('worldedit', 'WorldEdit', '7.3.9', 'modrinth', 'EngineHub', 'In-game world editing.', 5100, 120_000_000),
        plugin('coreprotect', 'CoreProtect', '22.4', 'spigotmc', 'Intelli', 'Block logging and rollback.', 890, 12_800_000),
        plugin('viaversion', 'ViaVersion', '5.2.1', 'modrinth', 'ViaVersion', 'Lets older clients join 1.21.4.', 1300, 64_000_000),
        plugin('chunky', 'Chunky', '1.4.29', 'modrinth', 'pop4959', 'Pre-generates chunks up front.', 180, 6_100_000, { latestVersion: '1.4.31', updateAvailable: true }),
        plugin('spark', 'spark', '1.10.133', 'modrinth', 'Luck', 'Profiler for tick and memory spikes.', 3200, 34_000_000),
      ],
      catalog: PAPER_PLUGIN_CATALOG,
    },
  },
  {
    id: 'paper-1-21-4-baseline',
    name: 'Paper 1.21.4 · Baseline',
    description: 'Duplicate of the performance profile with no plugins installed. Reusable for test instances.',
    softwareId: 'paper',
    version: '1.21.4',
    memory: { minMb: 1024, maxMb: 4096, initialMb: 2048 },
    cpuCores: 2,
    javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=200',
    autoStart: false,
    restartOnCrash: true,
    crashThreshold: 3,
    ports: { rcon: 25585, query: null },
    createdAt: iso(-12 * DAY),
    updatedAt: iso(-12 * DAY),
    properties: {
      motd: '§7Lodestone §8| §fbaseline',
      'max-players': '20',
      'view-distance': '8',
      'simulation-distance': '6',
      'rcon.port': '25585',
      'rcon.password': 'a2f70c19d4e83b61',
      'white-list': 'false',
    },
    mods: { loader: 'Paper 1.21.4', installed: [], catalog: PAPER_PLUGIN_CATALOG },
  },
  {
    id: 'vanilla-1-21-4',
    name: 'Vanilla 1.21.4',
    description: 'Unmodified server software. No plugins, no mods — used for build and test worlds.',
    softwareId: 'vanilla',
    version: '1.21.4',
    memory: { minMb: 1024, maxMb: 4096, initialMb: 2048 },
    cpuCores: null,
    javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=150',
    autoStart: false,
    restartOnCrash: true,
    crashThreshold: 5,
    ports: { rcon: 25576, query: null },
    createdAt: iso(-41 * DAY),
    updatedAt: iso(-2 * DAY),
    properties: {
      motd: '§bCreative §7| §fflat build world',
      'max-players': '10',
      'view-distance': '12',
      'simulation-distance': '10',
      'rcon.port': '25576',
      'rcon.password': 'c1e9af204b7d3351',
      'player-idle-timeout': '30',
    },
    mods: { loader: 'Vanilla 1.21.4', installed: [], catalog: [] },
  },
  {
    id: 'fabric-1-21-1-create',
    name: 'Fabric 1.21.1 · Create',
    description: 'Create + exploration pack. Lower view distance, 12 GB heap, broad entity broadcast range.',
    softwareId: 'fabric',
    version: '1.21.1',
    memory: { minMb: 4096, maxMb: 12288, initialMb: 8192 },
    cpuCores: 4,
    javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=200 -XX:G1NewSizePercent=20 -XX:G1ReservePercent=20',
    autoStart: true,
    restartOnCrash: true,
    crashThreshold: 2,
    ports: { rcon: 25577, query: null },
    createdAt: iso(-19 * DAY),
    updatedAt: iso(-4 * HOUR),
    properties: {
      motd: '§6Modded §7| §fCreate + Sodium',
      'max-players': '16',
      'view-distance': '8',
      'simulation-distance': '6',
      'rcon.port': '25577',
      'rcon.password': 'b64d0e17ca9f2853',
      'network-compression-threshold': '512',
      'entity-broadcast-range-percentage': '80',
    },
    mods: {
      loader: 'Fabric 1.21.1',
      installed: [
        mod('sodium', 'Sodium', '0.6.13', 'modrinth', 'CaffeineMC', 'Renderer rewrite, big FPS win.', 1200, 148_000_000),
        mod('lithium', 'Lithium', '0.14.8', 'modrinth', 'CaffeineMC', 'General purpose server tick optimisations.', 640, 118_000_000),
        mod('ferritecore', 'FerriteCore', '7.0.2', 'modrinth', 'malte0811', 'Cuts memory used by blocks and chunk data.', 190, 96_000_000),
        mod('iris', 'Iris', '1.8.8', 'modrinth', 'Iris Shaders', 'Shader pack support, works with Sodium.', 3900, 78_000_000),
        mod('create', 'Create', '6.0.4', 'modrinth', 'simibubi', 'Kinetic machines, trains and contraptions.', 24_600, 82_000_000, { latestVersion: '6.1.0', updateAvailable: true }),
        mod('farmersdelight', "Farmer's Delight", '1.2.7', 'curseforge', 'vectorwing', 'Cooking expansion with real recipes.', 4100, 61_000_000, { enabled: false }),
        mod('indium', 'Indium', '1.0.34', 'modrinth', 'comp500', 'Fabric rendering API for Sodium.', 120, 74_000_000),
        mod('voicechat', 'Simple Voice Chat', '2.5.26', 'modrinth', 'henkelmax', 'Proximity voice chat on UDP.', 2400, 44_000_000),
      ],
      catalog: FABRIC_MOD_CATALOG,
    },
  },
];

/* ------------------------------------------------------------------ *
 * seed data — instances
 * ------------------------------------------------------------------ */

interface InstanceSeed {
  id: string;
  name: string;
  profileId: string;
  worldId: string;
  state: ServerState;
  ports: { minecraft: number; rcon: number; query: number | null };
  environment: Record<string, string>;
  createdAt: string;
  status: Partial<InstanceStatus> & { state: ServerState };
  players: ServerPlayers;
  consoleHistory?: { level: LogLevel; message: string; gap?: [number, number] }[];
}

const INSTANCE_SEEDS: InstanceSeed[] = [
  {
    id: 'survival-1',
    name: 'survival-1',
    profileId: 'paper-1-21-4-performance',
    worldId: 'survival-season-2',
    state: 'running',
    ports: { minecraft: 25565, rcon: 25575, query: null },
    environment: { TZ: 'Europe/Berlin', EULA: 'TRUE', PAPER_VELOCITY: 'true' },
    createdAt: iso(-63 * DAY),
    status: {
      state: 'running',
      startedAt: iso(-(3 * DAY + 14 * HOUR + 22 * 60_000)),
      uptimeSeconds: 3 * 86400 + 14 * 3600 + 22 * 60,
      players: { online: 7, max: 20 },
      memory: { usedBytes: 5.2 * 1024 ** 3, limitBytes: 8 * 1024 ** 3 },
      cpuPercent: 38,
      tps: 19.97,
      tickMs: 31,
      network: { rxPerSec: 84 * 1024, txPerSec: 312 * 1024, rxTotal: 9.4 * 1024 ** 3, txTotal: 38.1 * 1024 ** 3, dropped: 0.02 },
      host: {
        cpuPercent: 38,
        cpuCores: 4,
        diskUsedBytes: 41.6 * 1024 ** 3,
        diskTotalBytes: 80 * 1024 ** 3,
        swapUsedBytes: 0.2 * 1024 ** 3,
        swapTotalBytes: 2 * 1024 ** 3,
      },
      container: { id: '8f2c41d09ab7', name: 'lodestone-survival-1', image: 'sunstead/minecraft:paper-1.21.4', healthy: true },
    },
    players: makePlayers(
      SURVIVAL_ONLINE,
      SURVIVAL_OFFLINE,
      SURVIVAL_ONLINE.map((p) => p.name).concat(['quarry_quinn', 'tnt_lily']),
    ),
    consoleHistory: [
      { level: 'INFO', message: '[Lodestone] Container lodestone-survival-1 started', gap: [400_000, 400_000] },
      { level: 'INFO', message: 'Starting minecraft server version 1.21.4' },
      { level: 'INFO', message: 'Loading properties' },
      { level: 'INFO', message: 'Preparing level "world"' },
      { level: 'INFO', message: 'Preparing spawn area: 34%' },
      { level: 'INFO', message: 'Preparing spawn area: 100%' },
      { level: 'INFO', message: 'Done (4.812s)! For help, type "help"' },
      { level: 'INFO', message: 'Steve_Builds joined the game', gap: [90_000, 140_000] },
      { level: 'INFO', message: 'redstone_rae issued server command: /home base' },
      { level: 'INFO', message: 'CreeperSlayer joined the game' },
      { level: 'INFO', message: 'Alexandrite has made the advancement [Diamonds!]' },
      { level: 'INFO', message: 'Saving the game (this may take a moment!)' },
      { level: 'INFO', message: 'Saved the game' },
      { level: 'INFO', message: 'mossy_cobble joined the game' },
      { level: 'WARN', message: "Can't keep up! Is the server overloaded? Running 2312ms behind, skipping 46 ticks" },
      { level: 'INFO', message: 'pixelpilot joined the game' },
      { level: 'INFO', message: 'DiamondDan was blown up by Creeper' },
      { level: 'INFO', message: 'Steve_Builds issued server command: /time set day' },
      { level: 'ERROR', message: 'Failed to load chunk at [-312, 88]: region file r.-10.2.mca is truncated' },
      { level: 'INFO', message: 'ThreadedAnvilChunkStorage (world): All chunks are saved' },
      { level: 'INFO', message: 'Alexandrite has made the advancement [Free the End]' },
      { level: 'DEBUG', message: 'Preparing spawn area: 84%' },
      { level: 'INFO', message: 'CreeperSlayer issued server command: /tp redstone_rae' },
      { level: 'INFO', message: 'pixelpilot left the game' },
      { level: 'INFO', message: 'pixelpilot joined the game' },
    ],
  },
  {
    id: 'creative-1',
    name: 'creative-1',
    profileId: 'vanilla-1-21-4',
    worldId: 'flat-test',
    state: 'stopped',
    ports: { minecraft: 25566, rcon: 25576, query: null },
    environment: { TZ: 'Europe/Berlin', EULA: 'TRUE' },
    createdAt: iso(-41 * DAY),
    status: makeStatus({
      state: 'stopped',
      players: { online: 0, max: 10 },
      memory: { usedBytes: 0, limitBytes: 4 * 1024 ** 3 },
      container: { id: '—', name: 'lodestone-creative-1', image: 'sunstead/minecraft:vanilla-1.21.4', healthy: false },
    }),
    players: makePlayers([], CREATIVE_OFFLINE, ['pixel_pete', 'buildmaster_bob', 'luma_builds']),
    consoleHistory: [
      { level: 'INFO', message: 'Steve_Builds issued server command: /time set day' },
      { level: 'INFO', message: 'luma_builds joined the game' },
      { level: 'INFO', message: 'grid_gremlin left the game' },
      { level: 'INFO', message: 'Stopping the server', gap: [300_000, 300_000] },
      { level: 'INFO', message: 'Saving players' },
      { level: 'INFO', message: 'Saving worlds' },
      { level: 'INFO', message: '[Lodestone] Container stopped, world unmounted from flat_test' },
    ],
  },
  {
    id: 'modded-1',
    name: 'modded-1',
    profileId: 'fabric-1-21-1-create',
    worldId: 'create-world',
    state: 'running',
    ports: { minecraft: 25567, rcon: 25577, query: null },
    environment: { TZ: 'Europe/Berlin', EULA: 'TRUE', FABRIC_LOADER: '0.16.10' },
    createdAt: iso(-19 * DAY),
    status: {
      state: 'running',
      startedAt: iso(-6 * HOUR - 12 * 60_000),
      uptimeSeconds: 6 * 3600 + 12 * 60,
      players: { online: 3, max: 16 },
      memory: { usedBytes: 10.1 * 1024 ** 3, limitBytes: 12 * 1024 ** 3 },
      cpuPercent: 62,
      tps: 19.42,
      tickMs: 44,
      network: { rxPerSec: 46 * 1024, txPerSec: 198 * 1024, rxTotal: 3.1 * 1024 ** 3, txTotal: 11.6 * 1024 ** 3, dropped: 0.05 },
      host: {
        cpuPercent: 62,
        cpuCores: 4,
        diskUsedBytes: 41.6 * 1024 ** 3,
        diskTotalBytes: 80 * 1024 ** 3,
        swapUsedBytes: 0.2 * 1024 ** 3,
        swapTotalBytes: 2 * 1024 ** 3,
      },
      container: { id: 'c09ba31f7742', name: 'lodestone-modded-1', image: 'sunstead/minecraft:fabric-1.21.1', healthy: true },
    },
    players: makePlayers(MODDED_ONLINE, MODDED_OFFLINE, MODDED_ONLINE.map((p) => p.name).concat(['brass_bandit'])),
    consoleHistory: [
      { level: 'INFO', message: '[Lodestone] Container lodestone-modded-1 started', gap: [400_000, 400_000] },
      { level: 'INFO', message: 'Loading 8 mods:' },
      { level: 'INFO', message: '\t- fabric-api 0.115.0+1.21.1' },
      { level: 'INFO', message: '\t- sodium 0.6.13' },
      { level: 'INFO', message: '\t- lithium 0.14.8' },
      { level: 'INFO', message: '\t- create 6.0.4' },
      { level: 'INFO', message: 'Done (11.284s)! For help, type "help"' },
      { level: 'INFO', message: 'gearshift_gwen joined the game', gap: [120_000, 180_000] },
      { level: 'INFO', message: 'copper_cowboy joined the game' },
      { level: 'INFO', message: 'tinker_tom issued server command: /create versions' },
      { level: 'WARN', message: "Can't keep up! Is the server overloaded? Running 1184ms behind, skipping 23 ticks" },
      { level: 'INFO', message: 'gearshift_gwen has made the advancement [Ice and Repair]' },
      { level: 'INFO', message: 'Saving the game (this may take a moment!)' },
      { level: 'INFO', message: 'Saved the game' },
      { level: 'ERROR', message: 'Create contraption at [104, 71, -338] failed to load: missing block state train_signal' },
      { level: 'INFO', message: 'copper_cowboy was pulled into a Create train' },
      { level: 'DEBUG', message: 'Ticking block entity: create:mechanical_pump at [112, 64, -320]' },
      { level: 'INFO', message: 'tinker_tom joined the game' },
    ],
  },
  {
    // Reuses the baseline profile against the detached world, proving a
    // profile can serve several instances and a world can be re-attached.
    id: 'nether-test-1',
    name: 'nether-test-1',
    profileId: 'paper-1-21-4-baseline',
    worldId: 'archived-nether-test',
    state: 'stopped',
    ports: { minecraft: 25568, rcon: 25585, query: null },
    environment: { TZ: 'Europe/Berlin', EULA: 'TRUE' },
    createdAt: iso(-88 * DAY),
    status: makeStatus({
      state: 'stopped',
      players: { online: 0, max: 20 },
      memory: { usedBytes: 0, limitBytes: 4 * 1024 ** 3 },
      container: { id: '—', name: 'lodestone-nether-test-1', image: 'sunstead/minecraft:paper-1.21.4', healthy: false },
    }),
    players: makePlayers([], [], []),
    consoleHistory: [
      { level: 'INFO', message: '[Lodestone] Container lodestone-nether-test-1 was removed', gap: [400_000, 400_000] },
      { level: 'INFO', message: '[Lodestone] World archived-nether-test kept on disk', gap: [2000, 2000] },
    ],
  },
];

/* ------------------------------------------------------------------ *
 * hydrate
 * ------------------------------------------------------------------ */

function hydrateProfile(seed: ProfileSeed): void {
  const instanceCount = INSTANCE_SEEDS.filter((instance) => instance.profileId === seed.id).length;
  const profile: Profile = {
    id: seed.id,
    name: seed.name,
    description: seed.description,
    softwareId: seed.softwareId,
    software: SOFTWARE_LABEL[seed.softwareId],
    version: seed.version,
    image: `sunstead/minecraft:${seed.softwareId}-${seed.version}`,
    memory: seed.memory,
    cpuCores: seed.cpuCores,
    javaArgs: seed.javaArgs,
    autoStart: seed.autoStart,
    restartOnCrash: seed.restartOnCrash,
    crashThreshold: seed.crashThreshold,
    ports: seed.ports,
    createdAt: seed.createdAt,
    updatedAt: seed.updatedAt,
    instanceCount,
  };
  profiles.set(seed.id, { profile, properties: buildFields(seed.properties), mods: seed.mods });
}

function hydrateWorld(seed: WorldSeed): void {
  const attached = INSTANCE_SEEDS.find((instance) => instance.worldId === seed.id)?.id ?? null;
  const world: World = {
    id: seed.id,
    name: seed.name,
    folder: seed.folder,
    levelType: seed.levelType,
    seed: seed.seed,
    generateStructures: seed.generateStructures,
    generated: seed.generated,
    maxWorldSize: seed.maxWorldSize,
    allowNether: seed.allowNether,
    difficulty: seed.difficulty,
    gamemode: seed.gamemode,
    hardcore: seed.hardcore,
    spawnProtection: seed.spawnProtection,
    spawnMonsters: seed.spawnMonsters,
    spawnNpcs: seed.spawnNpcs,
    pvp: seed.pvp,
    allowFlight: seed.allowFlight,
    enableCommandBlock: seed.enableCommandBlock,
    dataPacks: seed.dataPacks,
    sizeBytes: seed.sizeBytes,
    createdAt: seed.createdAt,
    updatedAt: seed.updatedAt,
    attachedInstanceId: attached,
  };
  const status: WorldStatus = {
    worldId: seed.id,
    attached: attached !== null,
    diskBytes: seed.sizeBytes,
    chunks: seed.status.chunks,
    entities: seed.status.entities,
    blockEntities: seed.status.blockEntities,
    day: seed.status.day,
    timeOfDay: seed.status.timeOfDay,
    dimensions: { nether: seed.allowNether, end: false },
    playerCount: seed.status.playerCount,
    lastPlayedAt: seed.status.lastPlayedAt,
  };
  worlds.set(seed.id, {
    world,
    status,
    backups: seed.backups,
    properties: {
      'level-name': seed.folder,
      'level-seed': seed.seed ?? '',
      'level-type': seed.levelType,
      'generate-structures': String(seed.generateStructures),
      'max-world-size': String(seed.maxWorldSize),
      'allow-nether': String(seed.allowNether),
      difficulty: seed.difficulty,
      gamemode: seed.gamemode,
      hardcore: String(seed.hardcore),
      'force-gamemode': 'false',
      pvp: String(seed.pvp),
      'allow-flight': String(seed.allowFlight),
      'enable-command-block': String(seed.enableCommandBlock),
      'spawn-protection': String(seed.spawnProtection),
      'spawn-monsters': String(seed.spawnMonsters),
      'spawn-npcs': String(seed.spawnNpcs),
      'initial-enabled-packs': seed.dataPacks.enabled.join(','),
      'initial-disabled-packs': seed.dataPacks.disabled.join(','),
    },
  });
}

function seedConsole(instance: DemoInstance, history: { level: LogLevel; message: string; gap?: [number, number] }[]): void {
  let ts = Date.now() - 40 * 60_000;
  const lines: ConsoleLine[] = [];
  for (const entry of history) {
    const gapMs = entry.gap ? rand(entry.gap[0], entry.gap[1]) : rand(4000, 22000);
    ts -= gapMs;
    lines.push({
      id: uid('line'),
      ts,
      level: entry.level,
      message: entry.message.replace(/\{name\}/g, pick(SURVIVAL_ONLINE).name),
      source: entry.message.startsWith('[Lodestone') ? 'lodestone' : 'server',
    });
  }
  instance.console = lines.reverse();
}

function hydrateInstance(seed: InstanceSeed): void {
  const profile = profiles.get(seed.profileId);
  const world = worlds.get(seed.worldId);
  if (!profile || !world) throw new Error(`seed ${seed.id} references a missing profile or world`);

  const instance: Instance = {
    id: seed.id,
    name: seed.name,
    profileId: seed.profileId,
    worldId: seed.worldId,
    state: seed.state,
    players: seed.status.players ?? { online: 0, max: 20 },
    address: `play.sunstead.gg:${seed.ports.minecraft}`,
    ports: seed.ports,
    environment: seed.environment,
    createdAt: seed.createdAt,
    profile: { name: profile.profile.name, software: profile.profile.software, version: profile.profile.version },
    world: { name: world.world.name, folder: world.world.folder },
  };

  const demoInstance: DemoInstance = {
    instance,
    status: makeStatus({ ...seed.status, memory: { usedBytes: seed.status.memory?.usedBytes ?? 0, limitBytes: profile.profile.memory.maxMb * 1024 * 1024 } }),
    players: seed.players,
    console: [],
    listeners: new Set(),
    ticker: null,
    timer: null,
  };

  if (seed.consoleHistory) seedConsole(demoInstance, seed.consoleHistory);
  instances.set(seed.id, demoInstance);
}

WORLD_SEEDS.forEach(hydrateWorld);
PROFILE_SEEDS.forEach(hydrateProfile);
INSTANCE_SEEDS.forEach(hydrateInstance);

/* ------------------------------------------------------------------ *
 * metrics drift + console streaming
 * ------------------------------------------------------------------ */

function drift(instance: DemoInstance): void {
  const status = instance.status;
  if (status.state !== 'running') {
    status.memory.usedBytes = 0;
    status.cpuPercent = 0;
    status.tps = 0;
    status.tickMs = 0;
    status.network.rxPerSec = 0;
    status.network.txPerSec = 0;
    status.players.online = 0;
    status.uptimeSeconds = null;
    status.startedAt = null;
    return;
  }

  status.uptimeSeconds =
    status.startedAt !== null ? Math.floor((Date.now() - Date.parse(status.startedAt)) / 1000) : null;
  status.memory.usedBytes = Math.round(
    jitter(status.memory.usedBytes, status.memory.limitBytes * 0.02, status.memory.limitBytes * 0.35, status.memory.limitBytes * 0.95),
  );
  status.cpuPercent = Math.round(jitter(status.cpuPercent, 7, 8, 96));
  status.host.cpuPercent = status.cpuPercent;
  status.tps = Number(jitter(status.tps, 0.06, 17.8, 20).toFixed(2));
  status.tickMs = Math.round(jitter(status.tickMs, 5, 12, 49));
  status.network.rxPerSec = Math.round(jitter(status.network.rxPerSec, 20 * 1024, 4 * 1024, 400 * 1024));
  status.network.txPerSec = Math.round(jitter(status.network.txPerSec, 60 * 1024, 20 * 1024, 900 * 1024));
  status.network.rxTotal += status.network.rxPerSec * 2;
  status.network.txTotal += status.network.txPerSec * 2;
}

/** World metrics only drift while some instance has the world mounted. */
function driftWorld(worldId: string): void {
  const entry = worlds.get(worldId);
  if (!entry) return;
  if (!entry.world.attachedInstanceId) {
    entry.status.attached = false;
    entry.status.chunks = 0;
    entry.status.entities = 0;
    entry.status.blockEntities = 0;
    return;
  }
  entry.status.attached = true;
  entry.status.entities = Math.max(0, Math.round(jitter(entry.status.entities, 24, 0, 9000)));
  entry.status.chunks = Math.max(0, Math.round(jitter(entry.status.chunks, 6, 0, 4000)));
  entry.status.blockEntities = Math.max(0, Math.round(jitter(entry.status.blockEntities, 4, 0, 2000)));
}

const AMBIENT: { level: LogLevel; message: string }[] = [
  { level: 'INFO', message: '{name} joined the game' },
  { level: 'INFO', message: '{name} left the game' },
  { level: 'INFO', message: '{name} issued server command: /list' },
  { level: 'INFO', message: '{name} issued server command: /home base' },
  { level: 'INFO', message: '{name} issued server command: /warp market' },
  { level: 'INFO', message: '{name} has made the advancement [Diamonds!]' },
  { level: 'INFO', message: '{name} has made the advancement [A Balanced Diet]' },
  { level: 'INFO', message: '{name} was blown up by Creeper' },
  { level: 'INFO', message: '{name} fell from a high place' },
  { level: 'INFO', message: 'Saving the game (this may take a moment!)' },
  { level: 'INFO', message: 'Saved the game' },
  { level: 'INFO', message: 'ThreadedAnvilChunkStorage (world): All chunks are saved' },
  { level: 'WARN', message: "Can't keep up! Is the server overloaded? Running 1184ms behind, skipping 23 ticks" },
  { level: 'DEBUG', message: 'Preparing spawn area: 100%' },
  { level: 'INFO', message: '{name} issued server command: /tp redstone_rae' },
  { level: 'INFO', message: '{name} issued server command: /setblock ~ ~ ~ minecraft:redstone_block' },
];

function namesOnline(instance: DemoInstance): string[] {
  const online = instance.players.players.filter((player) => player.online).map((player) => player.name);
  return online.length > 0 ? online : ['Server'];
}

function stopTicker(instance: DemoInstance): void {
  if (instance.ticker) {
    clearInterval(instance.ticker);
    instance.ticker = null;
  }
}

function startTicker(instance: DemoInstance): void {
  if (instance.ticker) return;
  instance.ticker = setInterval(() => {
    if (instance.instance.state !== 'running') return;
    const names = namesOnline(instance);
    const template = pick(AMBIENT);
    push(instance, template.level, template.message.replace(/\{name\}/g, pick(names)));
  }, 2400);
}

function transition(instance: DemoInstance, next: ServerState, delay: number, after?: () => void): void {
  if (instance.timer) clearTimeout(instance.timer);
  instance.timer = setTimeout(() => {
    instance.timer = null;
    instance.instance.state = next;
    instance.status.state = next;
    after?.();
  }, delay);
}

function push(
  instance: DemoInstance,
  level: LogLevel,
  message: string,
  source: ConsoleLine['source'] = 'server',
): void {
  const line: ConsoleLine = { id: uid('line'), ts: Date.now(), level, message, source };
  instance.console.push(line);
  if (instance.console.length > 2000) instance.console.splice(0, instance.console.length - 2000);
  instance.listeners.forEach((listener) => listener(line));
}

function startupLines(version: string, image: string, worldFolder: string): { level: LogLevel; message: string }[] {
  return [
    { level: 'INFO', message: `[Lodestone] Pulling ${image} (already present)` },
    { level: 'INFO', message: `[Lodestone] Mounting world "${worldFolder}" into /server/world` },
    { level: 'INFO', message: 'Starting minecraft server version ' + version },
    { level: 'INFO', message: 'Loading properties' },
    { level: 'INFO', message: 'Default game type: SURVIVAL' },
    { level: 'INFO', message: 'Generating keypair' },
    { level: 'INFO', message: `Preparing level "${worldFolder}"` },
    { level: 'INFO', message: 'Preparing start region for dimension [minecraft:overworld]' },
    { level: 'INFO', message: 'Preparing spawn area: 72%' },
    { level: 'INFO', message: 'Time elapsed: 1842 ms' },
    { level: 'INFO', message: 'RCON running on 0.0.0.0:25575' },
    { level: 'INFO', message: 'Done (4.812s)! For help, type "help"' },
  ];
}

const STOP_LINES: { level: LogLevel; message: string }[] = [
  { level: 'INFO', message: 'Stopping the server' },
  { level: 'INFO', message: 'Saving players' },
  { level: 'INFO', message: 'Saving worlds' },
  { level: 'INFO', message: 'Saving chunks for level ServerLevel[world]/minecraft:overworld' },
  { level: 'INFO', message: 'ThreadedAnvilChunkStorage (world): All chunks are saved' },
  { level: 'INFO', message: '[Lodestone] Container stopped, world left on disk' },
];

/* ------------------------------------------------------------------ *
 * demo API — mirrors client.ts
 * ------------------------------------------------------------------ */

/* -------------------------------- worlds ------------------------------- */

export function getWorlds(): World[] {
  return [...worlds.values()].map((entry) => entry.world);
}

export function getWorld(id: string): World {
  const entry = worlds.get(id);
  if (!entry) throw new Error(`no such world: ${id}`);
  return entry.world;
}

export function getWorldStatus(id: string): WorldStatus {
  const entry = worlds.get(id);
  if (!entry) throw new Error(`no such world: ${id}`);
  driftWorld(id);
  return entry.status;
}

export function createWorld(request: CreateWorldRequest): World {
  const id = uniqueId(request.name, (candidate) => worlds.has(candidate));
  const world: World = {
    id,
    name: request.name,
    folder: id.replace(/-/g, '_'),
    levelType: request.levelType,
    seed: request.seed,
    generateStructures: request.generateStructures,
    // A world that has never been mounted has not generated yet.
    generated: false,
    maxWorldSize: request.maxWorldSize,
    allowNether: request.allowNether,
    difficulty: request.difficulty,
    gamemode: request.gamemode,
    hardcore: request.hardcore,
    spawnProtection: request.hardcore ? 0 : 16,
    spawnMonsters: request.difficulty !== 'peaceful',
    spawnNpcs: request.difficulty !== 'peaceful',
    pvp: true,
    allowFlight: false,
    enableCommandBlock: true,
    dataPacks: { enabled: ['vanilla'], disabled: [] },
    sizeBytes: 0,
    createdAt: iso(),
    updatedAt: iso(),
    attachedInstanceId: null,
  };
  worlds.set(id, {
    world,
    status: {
      worldId: id,
      attached: false,
      diskBytes: 0,
      chunks: 0,
      entities: 0,
      blockEntities: 0,
      day: 0,
      timeOfDay: '06:00',
      dimensions: { nether: request.allowNether, end: false },
      playerCount: 0,
      lastPlayedAt: null,
    },
    backups: {
      backups: [],
      schedule: { enabled: false, intervalMinutes: 1440, retention: 7, lastRunAt: null, nextRunAt: null },
      storage: { path: `/var/lib/lodestone/backups/${id}`, usedBytes: 0, quotaBytes: 50 * 1024 ** 3 },
    },
    properties: {
      'level-name': world.folder,
      'level-seed': request.seed ?? '',
      'level-type': request.levelType,
      'generate-structures': String(request.generateStructures),
      'max-world-size': String(request.maxWorldSize),
      'allow-nether': String(request.allowNether),
      difficulty: request.difficulty,
      gamemode: request.gamemode,
      hardcore: String(request.hardcore),
      'force-gamemode': 'false',
      pvp: 'true',
      'allow-flight': 'false',
      'enable-command-block': 'true',
      'spawn-protection': String(world.spawnProtection),
      'spawn-monsters': String(world.spawnMonsters),
      'spawn-npcs': String(world.spawnNpcs),
      'initial-enabled-packs': 'vanilla',
      'initial-disabled-packs': '',
    },
  });
  return world;
}

export function updateWorld(id: string, body: Partial<CreateWorldRequest>): World {
  const entry = worlds.get(id);
  if (!entry) throw new Error(`no such world: ${id}`);
  const world = entry.world;
  const apply = <K extends keyof World>(key: K, value: World[K] | undefined) => {
    if (value !== undefined) world[key] = value;
  };
  // Generation keys are locked once the world has generated.
  const generationKey = (key: keyof World, value: unknown) => {
    if (value !== undefined && !world.generated) {
      (world[key] as unknown) = value;
    }
  };
  generationKey('levelType', body.levelType);
  generationKey('generateStructures', body.generateStructures);
  if (!world.generated && body.seed !== undefined) world.seed = body.seed;

  apply('difficulty', body.difficulty);
  apply('gamemode', body.gamemode);
  apply('hardcore', body.hardcore);
  apply('maxWorldSize', body.maxWorldSize);
  apply('allowNether', body.allowNether);
  if (body.name !== undefined && body.name.trim()) world.name = body.name.trim();
  world.updatedAt = iso();

  // Keep the mirrored properties in step.
  entry.properties.difficulty = world.difficulty;
  entry.properties.gamemode = world.gamemode;
  entry.properties.hardcore = String(world.hardcore);
  entry.properties['allow-nether'] = String(world.allowNether);
  entry.properties['max-world-size'] = String(world.maxWorldSize);
  entry.status.dimensions.nether = world.allowNether;
  return world;
}

export function duplicateWorld(id: string): World {
  const entry = worlds.get(id);
  if (!entry) throw new Error(`no such world: ${id}`);
  const copy = getWorld(createWorld({
    name: `${entry.world.name} copy`,
    levelType: entry.world.levelType,
    seed: entry.world.seed,
    generateStructures: entry.world.generateStructures,
    difficulty: entry.world.difficulty,
    gamemode: entry.world.gamemode,
    hardcore: entry.world.hardcore,
    maxWorldSize: entry.world.maxWorldSize,
    allowNether: entry.world.allowNether,
  }).id);
  // A duplicate starts as a byte-for-byte copy, so it counts as generated.
  const cloned = worlds.get(copy.id)!;
  cloned.world.generated = entry.world.generated;
  cloned.world.sizeBytes = entry.world.sizeBytes;
  cloned.status.diskBytes = entry.world.sizeBytes;
  cloned.status.day = entry.status.day;
  return cloned.world;
}

export function deleteWorld(id: string): void {
  const entry = worlds.get(id);
  if (!entry) throw new Error(`no such world: ${id}`);
  if (entry.world.attachedInstanceId) {
    throw new Error(`world "${entry.world.name}" is attached to ${entry.world.attachedInstanceId}; detach it first`);
  }
  worlds.delete(id);
}

/* ------------------------------- profiles ------------------------------ */

export function getProfiles(): Profile[] {
  return [...profiles.values()].map((entry) => entry.profile);
}

export function getProfile(id: string): Profile {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  return entry.profile;
}

const SOFTWARE_VERSIONS: Record<ServerSoftwareId, string[]> = {
  vanilla: ['1.21.4', '1.21.1', '1.20.6'],
  paper: ['1.21.4', '1.21.1', '1.20.6'],
  fabric: ['1.21.4', '1.21.1', '1.20.4'],
  forge: ['1.21.1', '1.20.6'],
  neoforge: ['1.21.4', '1.21.1'],
};

const SOFTWARE_BLURB: Record<ServerSoftwareId, string> = {
  vanilla: 'Unmodified. No plugins or mods — the baseline every world falls back to.',
  paper: 'High performance fork with a plugin API. The default for survival worlds.',
  fabric: 'Lightweight mod loader. Best for client-visible mods like Create or Sodium.',
  forge: 'Long-standing mod loader with the widest legacy mod coverage.',
  neoforge: 'Community fork of Forge, tracking the newest Minecraft releases.',
};

export function getSoftware(): SoftwareCatalog {
  return {
    software: (Object.keys(SOFTWARE_LABEL) as ServerSoftwareId[]).map((id) => ({
      id,
      label: SOFTWARE_LABEL[id],
      blurb: SOFTWARE_BLURB[id],
      kind: id === 'vanilla' ? 'vanilla' : id === 'paper' ? 'plugins' : 'mods',
      versions: SOFTWARE_VERSIONS[id].map((version) => ({
        version,
        image: `sunstead/minecraft:${id}-${version}`,
      })),
    })),
  };
}

function countInstancesUsingProfile(profileId: string): number {
  return [...instances.values()].filter((entry) => entry.instance.profileId === profileId).length;
}

export function createProfile(request: CreateProfileRequest): Profile {
  const id = uniqueId(request.name, (candidate) => profiles.has(candidate));
  const profile: Profile = {
    id,
    name: request.name,
    description: request.description,
    softwareId: request.softwareId,
    software: SOFTWARE_LABEL[request.softwareId],
    version: request.version,
    image: `sunstead/minecraft:${request.softwareId}-${request.version}`,
    memory: request.memory,
    cpuCores: request.cpuCores,
    javaArgs: request.javaArgs,
    autoStart: request.autoStart,
    restartOnCrash: request.restartOnCrash,
    crashThreshold: request.crashThreshold,
    ports: { rcon: 25575 + profiles.size * 10, query: null },
    createdAt: iso(),
    updatedAt: iso(),
    instanceCount: 0,
  };
  profiles.set(id, {
    profile,
    properties: buildFields({
      motd: `§a${request.name}`,
      'max-players': '20',
      'rcon.password': Math.random().toString(36).slice(2, 18).padEnd(16, '0'),
      'rcon.port': String(profile.ports.rcon),
    }),
    mods: {
      loader: `${profile.software} ${profile.version}`,
      installed: [],
      catalog: request.softwareId === 'paper' ? PAPER_PLUGIN_CATALOG : request.softwareId === 'fabric' ? FABRIC_MOD_CATALOG : [],
    },
  });
  return profile;
}

export function updateProfile(id: string, body: Partial<CreateProfileRequest>): Profile {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  const profile = entry.profile;
  if (body.name !== undefined && body.name.trim()) profile.name = body.name.trim();
  if (body.description !== undefined) profile.description = body.description;
  if (body.softwareId !== undefined) {
    profile.softwareId = body.softwareId;
    profile.software = SOFTWARE_LABEL[body.softwareId];
    profile.image = `sunstead/minecraft:${body.softwareId}-${profile.version}`;
    entry.mods.loader = `${profile.software} ${profile.version}`;
  }
  if (body.version !== undefined) {
    profile.version = body.version;
    profile.image = `sunstead/minecraft:${profile.softwareId}-${body.version}`;
    entry.mods.loader = `${profile.software} ${profile.version}`;
  }
  if (body.memory !== undefined) profile.memory = body.memory;
  if (body.cpuCores !== undefined) profile.cpuCores = body.cpuCores;
  if (body.javaArgs !== undefined) profile.javaArgs = body.javaArgs;
  if (body.autoStart !== undefined) profile.autoStart = body.autoStart;
  if (body.restartOnCrash !== undefined) profile.restartOnCrash = body.restartOnCrash;
  if (body.crashThreshold !== undefined) profile.crashThreshold = body.crashThreshold;
  profile.updatedAt = iso();
  profile.instanceCount = countInstancesUsingProfile(id);

  // Instances render their profile name and version, so keep them in step.
  for (const running of instances.values()) {
    if (running.instance.profileId !== id) continue;
    running.instance.profile = { name: profile.name, software: profile.software, version: profile.version };
    running.status.container.image = profile.image;
    running.status.memory.limitBytes = profile.memory.maxMb * 1024 * 1024;
  }
  return profile;
}

export function duplicateProfile(id: string): Profile {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  const copy = createProfile({
    name: `${entry.profile.name} copy`,
    description: entry.profile.description,
    softwareId: entry.profile.softwareId,
    version: entry.profile.version,
    memory: { ...entry.profile.memory },
    cpuCores: entry.profile.cpuCores,
    javaArgs: entry.profile.javaArgs,
    autoStart: entry.profile.autoStart,
    restartOnCrash: entry.profile.restartOnCrash,
    crashThreshold: entry.profile.crashThreshold,
  });
  const cloned = profiles.get(copy.id)!;
  cloned.properties = entry.properties.map((field) => ({ ...field }));
  cloned.mods = {
    loader: entry.mods.loader,
    installed: entry.mods.installed.map((m) => ({ ...m })),
    catalog: entry.mods.catalog.map((c) => ({ ...c })),
  };
  return copy;
}

export function deleteProfile(id: string): void {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  const using = countInstancesUsingProfile(id);
  if (using > 0) {
    throw new Error(`profile "${entry.profile.name}" is used by ${using} instance${using === 1 ? '' : 's'}`);
  }
  profiles.delete(id);
}

export function getProfileProperties(id: string): { path: string; updatedAt: string; fields: PropertyField[] } {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  // The editor only offers what a profile owns.
  return {
    path: '/srv/lodestone/profiles/' + id + '/server.properties',
    updatedAt: entry.profile.updatedAt,
    fields: splitFields(entry.properties, 'profile'),
  };
}

export function saveProfileProperties(id: string, changes: Record<string, string>): { path: string; updatedAt: string; fields: PropertyField[] } {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  entry.properties = entry.properties.map((field) =>
    field.scope === 'profile' && field.key in changes && !field.readOnly
      ? { ...field, value: changes[field.key] }
      : field,
  );
  const maxPlayers = entry.properties.find((field) => field.key === 'max-players')?.value;
  if (maxPlayers) {
    entry.profile.updatedAt = iso();
    for (const running of instances.values()) {
      if (running.instance.profileId !== id) continue;
      running.instance.players.max = Number(maxPlayers);
      running.status.players.max = Number(maxPlayers);
    }
  }
  return getProfileProperties(id);
}

export function getProfileMods(id: string): ModIndex {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  return entry.mods;
}

export function patchMod(id: string, modId: string, enabled: boolean): ModIndex {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  entry.mods.installed = entry.mods.installed.map((mod) => (mod.id === modId ? { ...mod, enabled } : mod));
  return entry.mods;
}

export function installMod(id: string, catalogId: string): ModIndex {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  const found = entry.mods.catalog.find((item) => item.id === catalogId);
  if (!found) throw new Error(`no such mod: ${catalogId}`);
  if (!entry.mods.installed.some((item) => item.id === found.id)) {
    entry.mods.installed.push({
      id: found.id,
      name: found.name,
      version: found.latestVersion,
      kind: found.kind,
      source: found.source,
      author: found.author,
      description: found.description,
      sizeBytes: found.sizeBytes,
      downloads: found.downloads,
      enabled: true,
    });
    entry.mods.catalog = entry.mods.catalog.filter((item) => item.id !== catalogId);
  }
  return entry.mods;
}

export function uninstallMod(id: string, modId: string): ModIndex {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  const found = entry.mods.installed.find((item) => item.id === modId);
  entry.mods.installed = entry.mods.installed.filter((item) => item.id !== modId);
  if (found) {
    entry.mods.catalog.unshift({
      id: found.id,
      name: found.name,
      author: found.author,
      description: found.description,
      kind: found.kind,
      source: found.source,
      latestVersion: found.latestVersion ?? found.version,
      sizeBytes: found.sizeBytes,
      downloads: found.downloads,
      categories: ['installed before'],
    });
  }
  return entry.mods;
}

export function updateMod(id: string, modId: string): ModIndex {
  const entry = profiles.get(id);
  if (!entry) throw new Error(`no such profile: ${id}`);
  entry.mods.installed = entry.mods.installed.map((mod) =>
    mod.id === modId && mod.latestVersion ? { ...mod, version: mod.latestVersion, updateAvailable: false } : mod,
  );
  return entry.mods;
}

/* ------------------------------ instances ------------------------------ */

export function getInstances(): Instance[] {
  return [...instances.values()].map((entry) => entry.instance);
}

export function getInstance(id: string): Instance {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  return entry.instance;
}

export function createInstance(request: CreateInstanceRequest): Instance {
  const profile = profiles.get(request.profileId);
  const world = worlds.get(request.worldId);
  if (!profile) throw new Error(`no such profile: ${request.profileId}`);
  if (!world) throw new Error(`no such world: ${request.worldId}`);
  if (world.world.attachedInstanceId) {
    throw new Error(`world "${world.world.name}" is already attached to ${world.world.attachedInstanceId}`);
  }

  const id = uniqueId(request.name, (candidate) => instances.has(candidate));
  const instance: Instance = {
    id,
    name: request.name.trim() || id,
    profileId: request.profileId,
    worldId: request.worldId,
    state: 'stopped',
    players: { online: 0, max: Number(profile.properties.find((f) => f.key === 'max-players')?.value ?? 20) },
    address: `play.sunstead.gg:${request.minecraftPort}`,
    ports: { minecraft: request.minecraftPort, rcon: request.rconPort, query: request.queryPort },
    environment: { TZ: 'UTC', EULA: 'TRUE' },
    createdAt: iso(),
    profile: { name: profile.profile.name, software: profile.profile.software, version: profile.profile.version },
    world: { name: world.world.name, folder: world.world.folder },
  };

  const demoInstance: DemoInstance = {
    instance,
    status: makeStatus({
      state: 'stopped',
      players: { online: 0, max: instance.players.max },
      memory: { usedBytes: 0, limitBytes: profile.profile.memory.maxMb * 1024 * 1024 },
      container: {
        id: '—',
        name: `lodestone-${id}`,
        image: profile.profile.image,
        healthy: false,
      },
    }),
    players: makePlayers([], [], []),
    console: [],
    listeners: new Set(),
    ticker: null,
    timer: null,
  };

  instances.set(id, demoInstance);
  world.world.attachedInstanceId = id;
  world.status.attached = true;
  profile.profile.instanceCount = countInstancesUsingProfile(request.profileId);

  push(demoInstance, 'INFO', `[Lodestone] Instance "${instance.name}" created from ${profile.profile.name}`, 'lodestone');
  push(demoInstance, 'INFO', `[Lodestone] World "${world.world.name}" attached — nothing is generated until first start`, 'lodestone');
  return instance;
}

export function updateInstance(id: string, body: Partial<CreateInstanceRequest>): Instance {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const instance = entry.instance;
  if (body.name !== undefined && body.name.trim()) instance.name = body.name.trim();
  if (body.minecraftPort !== undefined) {
    instance.ports.minecraft = body.minecraftPort;
    instance.address = `play.sunstead.gg:${body.minecraftPort}`;
  }
  if (body.rconPort !== undefined) instance.ports.rcon = body.rconPort;
  if (body.queryPort !== undefined) instance.ports.query = body.queryPort;
  if (body.profileId !== undefined && body.profileId !== instance.profileId) {
    const next = profiles.get(body.profileId);
    if (!next) throw new Error(`no such profile: ${body.profileId}`);
    instance.profileId = body.profileId;
    instance.profile = { name: next.profile.name, software: next.profile.software, version: next.profile.version };
    entry.status.container.image = next.profile.image;
    entry.status.memory.limitBytes = next.profile.memory.maxMb * 1024 * 1024;
  }
  return instance;
}

export function deleteInstance(id: string): void {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  stopTicker(entry);
  if (entry.timer) clearTimeout(entry.timer);

  // Detach the world, but keep it (and its backups) on disk.
  const world = worlds.get(entry.instance.worldId);
  if (world && world.world.attachedInstanceId === id) {
    world.world.attachedInstanceId = null;
    world.status.attached = false;
    world.world.updatedAt = iso();
  }
  const profile = profiles.get(entry.instance.profileId);
  instances.delete(id);
  if (profile) profile.profile.instanceCount = countInstancesUsingProfile(entry.instance.profileId);
}

/**
 * Rebinds an instance to another world. A running instance is stopped,
 * the mount is swapped and the server is started again on the new world.
 */
export function switchInstanceWorld(id: string, worldId: string): Instance {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const target = worlds.get(worldId);
  if (!target) throw new Error(`no such world: ${worldId}`);
  if (target.world.attachedInstanceId && target.world.attachedInstanceId !== id) {
    throw new Error(`world "${target.world.name}" is already attached to ${target.world.attachedInstanceId}`);
  }

  const previous = worlds.get(entry.instance.worldId);
  if (previous && previous.world.attachedInstanceId === id) {
    previous.world.attachedInstanceId = null;
    previous.status.attached = false;
  }
  target.world.attachedInstanceId = id;
  target.status.attached = true;
  target.world.updatedAt = iso();

  entry.instance.worldId = worldId;
  entry.instance.world = { name: target.world.name, folder: target.world.folder };

  const wasRunning = entry.instance.state === 'running' || entry.instance.state === 'starting';
  push(entry, 'INFO', `[Lodestone] Switching world to "${target.world.name}"`, 'lodestone');
  if (wasRunning) {
    push(entry, 'INFO', '[Lodestone] Restarting to mount the new world', 'lodestone');
  }
  // The world generates on first mount.
  if (!target.world.generated) {
    target.world.generated = true;
    push(entry, 'INFO', `[Lodestone] "${target.world.name}" generated for the first time`, 'lodestone');
  }
  return entry.instance;
}

export function postAction(id: string, action: ServerAction): Instance {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const profile = profiles.get(entry.instance.profileId);
  const world = worlds.get(entry.instance.worldId);
  const image = profile?.profile.image ?? entry.status.container.image;
  const version = profile?.profile.version ?? '1.21.4';
  const folder = world?.world.folder ?? 'world';

  if (action === 'start') {
    if (entry.instance.state === 'running') return entry.instance;
    entry.instance.state = 'starting';
    entry.status.state = 'starting';
    push(entry, 'INFO', `[Lodestone] Starting container lodestone-${id}`, 'lodestone');
    startupLines(version, image, folder).forEach((line, index) => {
      setTimeout(() => push(entry, line.level, line.message), 250 * (index + 1));
    });
    transition(entry, 'running', 250 * (startupLines(version, image, folder).length + 2), () => {
      entry.status.startedAt = iso();
      entry.status.uptimeSeconds = 0;
      entry.status.players = { ...entry.status.players, online: 0 };
      entry.status.memory.usedBytes = Math.round(entry.status.memory.limitBytes * 0.42);
      entry.status.tps = 20;
      entry.status.tickMs = 18;
      entry.status.cpuPercent = 44;
      entry.status.container.healthy = true;
      if (world) {
        world.world.generated = true;
        world.status.chunks = 32;
        world.status.entities = 12;
        world.status.blockEntities = 0;
        world.status.day = Math.max(1, world.status.day);
        world.status.timeOfDay = '06:00';
        world.status.lastPlayedAt = iso();
      }
      startTicker(entry);
    });
  }

  if (action === 'stop') {
    if (entry.instance.state === 'stopped') return entry.instance;
    entry.instance.state = 'stopping';
    entry.status.state = 'stopping';
    STOP_LINES.forEach((line, index) => setTimeout(() => push(entry, line.level, line.message), 300 * (index + 1)));
    transition(entry, 'stopped', 300 * (STOP_LINES.length + 2), () => {
      entry.status.container.healthy = false;
      stopTicker(entry);
      drift(entry);
    });
  }

  if (action === 'restart') {
    entry.instance.state = 'stopping';
    entry.status.state = 'stopping';
    STOP_LINES.forEach((line, index) => setTimeout(() => push(entry, line.level, line.message), 250 * (index + 1)));
    transition(entry, 'starting', 250 * (STOP_LINES.length + 1), () => {
      push(entry, 'INFO', '[Lodestone] Restarting container', 'lodestone');
      startupLines(version, image, folder).forEach((line, index) =>
        setTimeout(() => push(entry, line.level, line.message), 250 * (index + 1)),
      );
      transition(entry, 'running', 250 * (startupLines(version, image, folder).length + 2), () => {
        entry.status.startedAt = iso();
        entry.status.uptimeSeconds = 0;
        entry.status.tps = 20;
        entry.status.tickMs = 18;
        entry.status.container.healthy = true;
        startTicker(entry);
      });
    });
  }

  return entry.instance;
}

/* ------------------------------- console ------------------------------- */

export function getConsole(id: string, tail = 500): ConsoleLine[] {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  return entry.console.slice(-tail);
}

export function subscribeConsole(id: string, listener: (line: ConsoleLine) => void): () => void {
  const entry = instances.get(id);
  if (!entry) return () => undefined;
  entry.listeners.add(listener);
  if (entry.instance.state === 'running') startTicker(entry);
  return () => {
    entry.listeners.delete(listener);
    if (entry.listeners.size === 0) stopTicker(entry);
  };
}

function commandResponse(entry: DemoInstance, command: string): { level: LogLevel; message: string }[] {
  const trimmed = command.trim();
  const online = namesOnline(entry);
  const base = trimmed.startsWith('/') ? trimmed.slice(1) : trimmed;
  const [name, ...args] = base.split(/\s+/);
  const world = worlds.get(entry.instance.worldId);

  if (entry.instance.state !== 'running') {
    return [
      { level: 'ERROR', message: `[Lodestone] Cannot run "/${base}": ${entry.instance.name} is ${entry.instance.state}` },
    ];
  }

  switch (name) {
    case 'list':
      return [
        {
          level: 'INFO',
          message: `There are ${online.length} of a maximum of ${entry.instance.players.max} players online: ${online.join(', ')}`,
        },
      ];
    case 'help':
      return [
        { level: 'INFO', message: '/help — show this help' },
        { level: 'INFO', message: '/list — list online players' },
        { level: 'INFO', message: '/save-all — save chunks, players and the world' },
        { level: 'INFO', message: '/time set <day|night|noon> — change the world time' },
        { level: 'INFO', message: '/weather <clear|rain|thunder> — change the weather' },
        { level: 'INFO', message: '/whitelist <add|remove|list> — manage the whitelist' },
        { level: 'INFO', message: '/tps — Paper only: tick rate over 1m, 5m, 15m' },
      ];
    case 'save-all':
      return [
        { level: 'INFO', message: 'Saving the game (this may take a moment!)' },
        { level: 'INFO', message: 'Saved the game' },
      ];
    case 'save-off':
      return [{ level: 'INFO', message: 'Automatic saving is now disabled' }];
    case 'save-on':
      return [{ level: 'INFO', message: 'Automatic saving is now enabled' }];
    case 'time':
      return [{ level: 'INFO', message: `Set the time to ${args[1] === 'night' ? 13000 : args[1] === 'noon' ? 6000 : 1000}` }];
    case 'weather':
      return [{ level: 'INFO', message: `Set the weather to ${args[0] ?? 'clear'}` }];
    case 'difficulty':
      return [{ level: 'INFO', message: `Set the difficulty to ${args[0] ?? 'normal'} — this world only` }];
    case 'tps':
      return [
        {
          level: 'INFO',
          message: `TPS from last 1m, 5m, 15m: ${entry.status.tps.toFixed(2)}, ${Math.max(19.1, entry.status.tps - 0.2).toFixed(2)}, 19.98`,
        },
      ];
    case 'seed':
      return [{ level: 'INFO', message: `Seed: [${world?.world.seed ?? 'random'}]` }];
    case 'whitelist': {
      const sub = args[0];
      if (sub === 'list') {
        const listed = entry.players.players.filter((player) => player.whitelisted).map((player) => player.name);
        return [
          { level: 'INFO', message: `There are ${listed.length} whitelisted players: ${listed.join(', ') || '(none)'}` },
        ];
      }
      if (sub === 'on') {
        entry.players.whitelistEnabled = true;
        return [{ level: 'INFO', message: 'Whitelist is now turned on' }];
      }
      if (sub === 'off') {
        entry.players.whitelistEnabled = false;
        return [{ level: 'INFO', message: 'Whitelist is now turned off' }];
      }
      if (sub === 'add' || sub === 'remove') {
        const target = args[1];
        if (!target) return [{ level: 'ERROR', message: 'Usage: /whitelist <add|remove> <name>' }];
        const player = entry.players.players.find((item) => item.name.toLowerCase() === target.toLowerCase());
        if (player) player.whitelisted = sub === 'add';
        return [
          {
            level: 'INFO',
            message: sub === 'add' ? `Added ${target} to the whitelist` : `Removed ${target} from the whitelist`,
          },
        ];
      }
      return [{ level: 'ERROR', message: 'Usage: /whitelist <add|remove|on|off|list>' }];
    }
    case 'op':
    case 'deop': {
      const target = args[0];
      const player = entry.players.players.find((item) => item.name.toLowerCase() === (target ?? '').toLowerCase());
      if (player) player.op = name === 'op';
      return [
        {
          level: 'INFO',
          message: target
            ? name === 'op'
              ? `Made ${target} a server operator`
              : `Made ${target} no longer a server operator`
            : `Usage: /${name} <name>`,
        },
      ];
    }
    case 'kick': {
      const target = args[0];
      if (!target) return [{ level: 'ERROR', message: 'Usage: /kick <name> [reason]' }];
      const player = entry.players.players.find((item) => item.name.toLowerCase() === target.toLowerCase());
      if (player?.online) player.online = false;
      entry.status.players.online = entry.players.players.filter((item) => item.online).length;
      return [
        { level: 'INFO', message: `${target} was kicked by the server: ${args.slice(1).join(' ') || 'Kicked by an operator'}` },
        { level: 'INFO', message: `${target} left the game` },
      ];
    }
    case 'ban':
    case 'ban-ip': {
      const target = args[0];
      if (!target) return [{ level: 'ERROR', message: `Usage: /${name} <target> [reason]` }];
      entry.players.bans.unshift({
        uuid: null,
        name: target,
        reason: args.slice(1).join(' ') || 'Banned by an operator',
        bannedBy: 'Lodestone',
        createdAt: iso(),
        expiresAt: null,
      });
      const player = entry.players.players.find((item) => item.name.toLowerCase() === target.toLowerCase());
      if (player) player.online = false;
      entry.status.players.online = entry.players.players.filter((item) => item.online).length;
      return [{ level: 'INFO', message: `Banned ${target}: ${args.slice(1).join(' ') || 'Banned by an operator'}` }];
    }
    case 'pardon': {
      const target = args[0];
      entry.players.bans = entry.players.bans.filter((item) => item.name.toLowerCase() !== (target ?? '').toLowerCase());
      return [{ level: 'INFO', message: target ? `Pardoned ${target}` : 'Usage: /pardon <name>' }];
    }
    case 'stop':
      return [{ level: 'INFO', message: '[Lodestone] Sending stop, waiting for the container to exit' }];
    case 'say':
      return [{ level: 'INFO', message: `[Server] ${args.join(' ') || '...'}` }];
    default:
      return [
        { level: 'ERROR', message: 'Unknown or incomplete command, see below for error' },
        { level: 'ERROR', message: `${base.substring(Math.max(0, base.length - 8))}<--[PREVIOUS_HERE]` },
      ];
  }
}

export function sendCommand(id: string, command: string): ConsoleLine[] {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const trimmed = command.trim();
  if (!trimmed) return [];

  const echo: ConsoleLine = {
    id: uid('line'),
    ts: Date.now(),
    level: 'INFO',
    message: `Console issued server command: ${trimmed}`,
    source: 'rcon',
  };
  entry.console.push(echo);
  entry.listeners.forEach((listener) => listener(echo));

  const responses = commandResponse(entry, trimmed).map((response) => {
    const line: ConsoleLine = {
      id: uid('line'),
      ts: Date.now() + 1,
      level: response.level,
      message: response.message,
      source: 'server',
    };
    entry.console.push(line);
    entry.listeners.forEach((listener) => listener(line));
    return line;
  });

  if (entry.console.length > 2000) entry.console.splice(0, entry.console.length - 2000);
  if (trimmed.replace('/', '') === 'stop') setTimeout(() => postAction(id, 'stop'), 400);

  return [echo, ...responses];
}

/* ------------------------------- players ------------------------------- */

export function getPlayers(id: string): ServerPlayers {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const bans: BanEntry[] = entry.players.bans;
  return { ...entry.players, bans };
}

export function postPlayerAction(
  id: string,
  request: { action: string; name: string; reason?: string },
): ServerPlayers {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  const player = entry.players.players.find((item) => item.name.toLowerCase() === request.name.toLowerCase());

  switch (request.action) {
    case 'kick':
      if (player) player.online = false;
      push(entry, 'INFO', `${request.name} was kicked by the server: ${request.reason || 'Kicked by an operator'}`);
      push(entry, 'INFO', `${request.name} left the game`);
      break;
    case 'op':
      if (player) player.op = true;
      push(entry, 'INFO', `Made ${request.name} a server operator`);
      break;
    case 'deop':
      if (player) player.op = false;
      push(entry, 'INFO', `Made ${request.name} no longer a server operator`);
      break;
    case 'whitelist-add':
      if (player) player.whitelisted = true;
      else
        entry.players.players.push({
          uuid: uid('uuid'),
          name: request.name,
          online: false,
          pingMs: null,
          playtimeSeconds: 0,
          dimension: null,
          op: false,
          whitelisted: true,
          firstSeen: iso(),
          lastSeen: iso(),
        });
      entry.players.whitelistEnabled = true;
      push(entry, 'INFO', `Added ${request.name} to the whitelist`);
      break;
    case 'whitelist-remove':
      if (player) player.whitelisted = false;
      push(entry, 'INFO', `Removed ${request.name} from the whitelist`);
      break;
    case 'ban':
      entry.players.bans.unshift({
        uuid: player?.uuid ?? null,
        name: request.name,
        reason: request.reason || 'Banned by an operator',
        bannedBy: 'Lodestone',
        createdAt: iso(),
        expiresAt: null,
      });
      if (player) player.online = false;
      push(entry, 'INFO', `Banned ${request.name}: ${request.reason || 'Banned by an operator'}`);
      break;
    case 'pardon':
      entry.players.bans = entry.players.bans.filter((item) => item.name !== request.name);
      push(entry, 'INFO', `Pardoned ${request.name}`);
      break;
    default:
      break;
  }

  entry.status.players.online = entry.players.players.filter((item) => item.online).length;
  return getPlayers(id);
}

/* ------------------------------- backups ------------------------------- */

export function getBackups(worldId: string): BackupIndex {
  const entry = worlds.get(worldId);
  if (!entry) throw new Error(`no such world: ${worldId}`);
  const { schedule } = entry.backups;
  if (schedule.enabled && schedule.nextRunAt && Date.parse(schedule.nextRunAt) < Date.now()) {
    schedule.lastRunAt = schedule.nextRunAt;
    schedule.nextRunAt = iso(schedule.intervalMinutes * 60_000);
  }
  return entry.backups;
}

export function createBackup(worldId: string): Backup {
  const entry = worlds.get(worldId);
  if (!entry) throw new Error(`no such world: ${worldId}`);
  const backupEntry: Backup = {
    id: uid('bk'),
    worldId,
    name: 'Manual',
    kind: 'manual',
    createdAt: iso(),
    sizeBytes: Math.round((0.4 + Math.random() * 3) * 1024 ** 3),
    status: 'running',
    instanceId: entry.world.attachedInstanceId,
  };
  entry.backups.backups.unshift(backupEntry);
  setTimeout(() => {
    backupEntry.status = 'complete';
    entry.backups.storage.usedBytes += backupEntry.sizeBytes;
  }, 3500);
  return backupEntry;
}

export function deleteBackup(worldId: string, backupId: string): void {
  const entry = worlds.get(worldId);
  if (!entry) throw new Error(`no such world: ${worldId}`);
  const found = entry.backups.backups.find((item) => item.id === backupId);
  entry.backups.backups = entry.backups.backups.filter((item) => item.id !== backupId);
  if (found) entry.backups.storage.usedBytes = Math.max(0, entry.backups.storage.usedBytes - found.sizeBytes);
}

export function restoreBackup(worldId: string, backupId: string): void {
  const entry = worlds.get(worldId);
  if (!entry) throw new Error(`no such world: ${worldId}`);
  const attached = entry.world.attachedInstanceId;
  if (attached) {
    const instance = instances.get(attached);
    if (instance) push(instance, 'WARN', `[Lodestone] Restoring backup ${backupId}, the server will restart`, 'lodestone');
    postAction(attached, 'restart');
  }
}

export function patchSchedule(worldId: string, patchBody: Partial<BackupIndex['schedule']>): BackupIndex {
  const entry = worlds.get(worldId);
  if (!entry) throw new Error(`no such world: ${worldId}`);
  entry.backups.schedule = {
    ...entry.backups.schedule,
    ...patchBody,
    nextRunAt:
      patchBody.enabled === true || (patchBody.intervalMinutes && entry.backups.schedule.enabled)
        ? iso((patchBody.intervalMinutes ?? entry.backups.schedule.intervalMinutes) * 60_000)
        : entry.backups.schedule.nextRunAt,
  };
  return entry.backups;
}

/* --------------------------- drift snapshots --------------------------- */

/** Current metrics for an instance, with the demo drift applied first. */
export function snapshotStatus(id: string): InstanceStatus {
  const entry = instances.get(id);
  if (!entry) throw new Error(`no such instance: ${id}`);
  drift(entry);
  entry.instance.players = entry.status.players;
  return entry.status;
}

export { drift, driftWorld };
export { ADVANCEMENTS, DEATHS, AMBIENT_LINES, playerLine, int, pick };


