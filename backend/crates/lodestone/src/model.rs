//! Wire model: every JSON body and SSE payload the API serves.
//!
//! These types are the contract with the frontend. `cargo test` exports
//! each one to `frontend/src/api/generated/` through ts-rs, and CI fails
//! when the generated files drift from what is committed.
//!
//! Lodestone keeps three concepts apart:
//!
//! - **World**: persistent level data plus world-scoped gameplay policy.
//! - **Profile**: software, version, mods, properties, resource limits.
//! - **Instance**: one profile running one world in one container.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A JSON object of string values, `Record<string, string>` in TypeScript.
pub type StringMap = BTreeMap<String, String>;

/* ------------------------------------------------------------------------ */
/* Enums                                                                    */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ServerState {
    /// Downloading software, mods or a Java runtime before the first start.
    Preparing,
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

impl ServerState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Preparing | Self::Starting | Self::Running | Self::Stopping)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ServerSoftwareId {
    Vanilla,
    Paper,
    Fabric,
    Forge,
    Neoforge,
}

impl ServerSoftwareId {
    pub const ALL: [Self; 5] = [Self::Vanilla, Self::Paper, Self::Fabric, Self::Forge, Self::Neoforge];

    pub fn label(self) -> &'static str {
        match self {
            Self::Vanilla => "Vanilla",
            Self::Paper => "Paper",
            Self::Fabric => "Fabric",
            Self::Forge => "Forge",
            Self::Neoforge => "NeoForge",
        }
    }

    pub fn content_kind(self) -> ContentKind {
        match self {
            Self::Vanilla => ContentKind::Vanilla,
            Self::Paper => ContentKind::Plugins,
            Self::Fabric | Self::Forge | Self::Neoforge => ContentKind::Mods,
        }
    }

    /// Paper/Bukkit keep the Nether and End in sibling folders.
    pub fn world_layout(self) -> WorldLayout {
        match self {
            Self::Paper => WorldLayout::Bukkit,
            _ => WorldLayout::Vanilla,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ServerAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum GameMode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
            Self::Adventure => "adventure",
            Self::Spectator => "spectator",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peaceful => "peaceful",
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum LevelType {
    #[serde(rename = "minecraft:normal")]
    Normal,
    #[serde(rename = "minecraft:flat")]
    Flat,
    #[serde(rename = "minecraft:large_biomes")]
    LargeBiomes,
    #[serde(rename = "minecraft:amplified")]
    Amplified,
    #[serde(rename = "minecraft:single_biome_surface")]
    SingleBiomeSurface,
}

impl LevelType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "minecraft:normal",
            Self::Flat => "minecraft:flat",
            Self::LargeBiomes => "minecraft:large_biomes",
            Self::Amplified => "minecraft:amplified",
            Self::SingleBiomeSurface => "minecraft:single_biome_surface",
        }
    }
}

/// How dimensions are laid out on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum WorldLayout {
    /// One folder; the Nether and End live in `DIM-1` / `DIM1` inside it.
    Vanilla,
    /// Paper/Bukkit: `<folder>`, `<folder>_nether`, `<folder>_the_end`.
    Bukkit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ContentKind {
    Plugins,
    Mods,
    Vanilla,
}

/* ------------------------------------------------------------------------ */
/* Worlds                                                                   */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub struct DataPacks {
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

/// A persistent world. `attachedInstanceId` is the 1:1 attachment: a world
/// is bound to at most one instance at a time.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct World {
    pub id: String,
    pub name: String,
    /// `level-name` on disk, e.g. `world`.
    pub folder: String,
    /// Generation-only: fixed once the world has generated.
    pub level_type: LevelType,
    /// Generation-only: `null` means random.
    pub seed: Option<String>,
    /// Generation-only.
    pub generate_structures: bool,
    /// Whether the world has ever generated; locks the fields above.
    pub generated: bool,
    pub layout: WorldLayout,
    /// `DataVersion` from level.dat once generated.
    pub data_version: Option<i32>,
    /// The Minecraft version that last saved it (level.dat). Opening it with
    /// an older one is refused, since that can corrupt chunks.
    #[serde(default)]
    pub saved_with: Option<String>,
    pub max_world_size: u32,
    pub allow_nether: bool,
    pub difficulty: Difficulty,
    pub gamemode: GameMode,
    pub hardcore: bool,
    pub spawn_protection: u32,
    pub spawn_monsters: bool,
    pub spawn_npcs: bool,
    pub pvp: bool,
    pub allow_flight: bool,
    pub enable_command_block: bool,
    pub data_packs: DataPacks,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
    pub attached_instance_id: Option<String>,
}

/// World facts that do not depend on any container.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorldStatus {
    pub world_id: String,
    pub attached: bool,
    #[ts(type = "number")]
    pub disk_bytes: u64,
    /// In-game day, read from level.dat. `null` before first generation.
    #[ts(type = "number | null")]
    pub day: Option<i64>,
    pub time_of_day: Option<String>,
    pub dimensions: Dimensions,
    /// People who have played this world, across every instance that ran it.
    pub player_count: u32,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Dimensions {
    pub nether: bool,
    pub end: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreateWorldRequest {
    pub name: String,
    pub level_type: LevelType,
    pub seed: Option<String>,
    pub generate_structures: bool,
    pub difficulty: Difficulty,
    pub gamemode: GameMode,
    pub hardcore: bool,
    pub max_world_size: u32,
    pub allow_nether: bool,
}

/// Every editable world field. Generation-only fields are refused once
/// the world has generated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateWorldRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub level_type: Option<LevelType>,
    #[ts(optional)]
    pub seed: Option<String>,
    #[ts(optional)]
    pub generate_structures: Option<bool>,
    #[ts(optional)]
    pub difficulty: Option<Difficulty>,
    #[ts(optional)]
    pub gamemode: Option<GameMode>,
    #[ts(optional)]
    pub hardcore: Option<bool>,
    #[ts(optional)]
    pub max_world_size: Option<u32>,
    #[ts(optional)]
    pub allow_nether: Option<bool>,
    #[ts(optional)]
    pub spawn_protection: Option<u32>,
    #[ts(optional)]
    pub spawn_monsters: Option<bool>,
    #[ts(optional)]
    pub spawn_npcs: Option<bool>,
    #[ts(optional)]
    pub pvp: Option<bool>,
    #[ts(optional)]
    pub allow_flight: Option<bool>,
    #[ts(optional)]
    pub enable_command_block: Option<bool>,
    #[ts(optional)]
    pub data_packs: Option<DataPacks>,
}

/* ------------------------------------------------------------------------ */
/* Profiles                                                                 */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileMemory {
    pub min_mb: u32,
    pub max_mb: u32,
    /// `-Xms`: the initial heap.
    pub initial_mb: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProfilePorts {
    pub rcon: u16,
    pub query: Option<u16>,
}

/// A reusable server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub software_id: ServerSoftwareId,
    /// Human label, e.g. `Paper`.
    pub software: String,
    /// Minecraft version, e.g. `1.21.4`.
    pub version: String,
    /// Java major version this Minecraft version needs.
    pub java_major: u32,
    /// The stock JRE image the server runs in, e.g. `eclipse-temurin:21-jre`.
    pub image: String,
    pub memory: ProfileMemory,
    /// Container CPU limit; `null` means unbounded.
    pub cpu_cores: Option<f64>,
    pub java_args: String,
    pub auto_start: bool,
    pub restart_on_crash: bool,
    pub crash_threshold: u32,
    pub ports: ProfilePorts,
    pub created_at: String,
    pub updated_at: String,
    /// Instances currently using this profile, so the UI can guard deletion.
    pub instance_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreateProfileRequest {
    pub name: String,
    pub description: String,
    pub software_id: ServerSoftwareId,
    pub version: String,
    pub memory: ProfileMemory,
    pub cpu_cores: Option<f64>,
    pub java_args: String,
    pub auto_start: bool,
    pub restart_on_crash: bool,
    pub crash_threshold: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateProfileRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub description: Option<String>,
    #[ts(optional)]
    pub software_id: Option<ServerSoftwareId>,
    #[ts(optional)]
    pub version: Option<String>,
    #[ts(optional)]
    pub memory: Option<ProfileMemory>,
    /// Send `null` to clear the limit.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(type = "number | null")]
    #[ts(optional)]
    pub cpu_cores: Option<Option<f64>>,
    #[ts(optional)]
    pub java_args: Option<String>,
    #[ts(optional)]
    pub auto_start: Option<bool>,
    #[ts(optional)]
    pub restart_on_crash: Option<bool>,
    #[ts(optional)]
    pub crash_threshold: Option<u32>,
}

fn double_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SoftwareVersion {
    pub version: String,
    pub java_major: u32,
    /// `false` for snapshots, pre-releases and experimental loader builds.
    pub stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SoftwareOption {
    pub id: ServerSoftwareId,
    pub label: String,
    pub blurb: String,
    pub kind: ContentKind,
    /// Newest first.
    pub versions: Vec<SoftwareVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SoftwareCatalog {
    pub software: Vec<SoftwareOption>,
}

/* ------------------------------------------------------------------------ */
/* Instances                                                                */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstancePorts {
    pub minecraft: u16,
    pub rcon: u16,
    pub query: Option<u16>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlayerCount {
    pub online: u32,
    pub max: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstanceProfileRef {
    pub name: String,
    pub software: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstanceWorldRef {
    pub name: String,
    pub folder: String,
}

/// A profile bound to a world, running in one container. `profile` and
/// `world` are denormalised so lists render without extra fetches.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub profile_id: String,
    pub world_id: String,
    pub state: ServerState,
    pub players: PlayerCount,
    pub address: String,
    pub ports: InstancePorts,
    #[ts(type = "Record<string, string>")]
    pub environment: StringMap,
    pub created_at: String,
    /// Why the last start or run failed, when `state` is `error`.
    pub last_error: Option<String>,
    /// The job preparing or changing this instance right now, if any.
    pub job_id: Option<String>,
    pub profile: InstanceProfileRef,
    pub world: InstanceWorldRef,
}

/// Creates an instance, and optionally its world and profile, in one
/// transaction. Give exactly one of `worldId`/`newWorld` and one of
/// `profileId`/`newProfile`. Ports left out are allocated by the server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreateInstanceRequest {
    pub name: String,
    #[ts(optional)]
    pub world_id: Option<String>,
    #[ts(optional)]
    pub new_world: Option<CreateWorldRequest>,
    #[ts(optional)]
    pub profile_id: Option<String>,
    #[ts(optional)]
    pub new_profile: Option<CreateProfileRequest>,
    #[ts(optional)]
    pub minecraft_port: Option<u16>,
    #[ts(optional)]
    pub rcon_port: Option<u16>,
    #[ts(optional)]
    pub query_port: Option<u16>,
    #[ts(type = "Record<string, string>")]
    #[ts(optional)]
    pub environment: Option<StringMap>,
    /// Start the instance as soon as it exists.
    #[ts(optional)]
    pub start: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateInstanceRequest {
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub minecraft_port: Option<u16>,
    #[ts(optional)]
    pub rcon_port: Option<u16>,
    #[serde(default, deserialize_with = "double_option")]
    #[ts(type = "number | null")]
    #[ts(optional)]
    pub query_port: Option<Option<u16>>,
    #[ts(type = "Record<string, string>")]
    #[ts(optional)]
    pub environment: Option<StringMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SwitchWorldRequest {
    pub world_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ActionRequest {
    pub action: ServerAction,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryUsage {
    #[ts(type = "number")]
    pub used_bytes: u64,
    #[ts(type = "number")]
    pub limit_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskUsage {
    #[ts(type = "number")]
    pub used_bytes: u64,
    #[ts(type = "number")]
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetworkUsage {
    pub rx_per_sec: f64,
    pub tx_per_sec: f64,
    #[ts(type = "number")]
    pub rx_total: u64,
    #[ts(type = "number")]
    pub tx_total: u64,
    #[ts(type = "number")]
    pub dropped: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostUsage {
    pub cpu_percent: f64,
    pub cpu_cores: u32,
    #[ts(type = "number")]
    pub disk_used_bytes: u64,
    #[ts(type = "number")]
    pub disk_total_bytes: u64,
    #[ts(type = "number")]
    pub swap_used_bytes: u64,
    #[ts(type = "number")]
    pub swap_total_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub healthy: bool,
}

/// Container and host metrics for an instance. World facts live on WorldStatus.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstanceStatus {
    pub state: ServerState,
    pub started_at: Option<String>,
    #[ts(type = "number | null")]
    pub uptime_seconds: Option<u64>,
    pub players: PlayerCount,
    pub memory: MemoryUsage,
    pub cpu_percent: f64,
    /// Ticks per second (20 is healthy). `null` when the software cannot report it.
    pub tps: Option<f64>,
    /// Average milliseconds per tick (budget 50). `null` when unavailable.
    pub tick_ms: Option<f64>,
    pub disk: DiskUsage,
    pub network: NetworkUsage,
    pub host: HostUsage,
    pub container: ContainerInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
#[ts(export)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ConsoleSource {
    Server,
    Lodestone,
    Rcon,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConsoleLine {
    /// Monotonic id so the client can de-duplicate replays.
    pub id: String,
    /// Unix milliseconds.
    #[ts(type = "number")]
    pub ts: i64,
    pub level: LogLevel,
    pub message: String,
    pub source: ConsoleSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CommandRequest {
    pub command: String,
}

/* ------------------------------------------------------------------------ */
/* server.properties                                                        */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum PropertyScope {
    World,
    Profile,
    Instance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum PropertyControl {
    Text,
    Number,
    Switch,
    Select,
    Slider,
    Password,
    Textarea,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PropertyOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertyField {
    pub key: String,
    pub label: String,
    pub description: String,
    pub scope: PropertyScope,
    pub group: String,
    pub control: PropertyControl,
    #[serde(default)]
    pub value: String,
    pub default_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub options: Option<Vec<PropertyOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub step: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sensitive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub applies_live: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub derived_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PropertyFile {
    pub path: String,
    pub updated_at: String,
    pub fields: Vec<PropertyField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SavePropertiesRequest {
    #[ts(type = "Record<string, string>")]
    pub changes: StringMap,
}

/* ------------------------------------------------------------------------ */
/* Players                                                                  */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Player {
    pub uuid: String,
    pub name: String,
    pub online: bool,
    pub ping_ms: Option<u32>,
    #[ts(type = "number")]
    pub playtime_seconds: u64,
    pub dimension: Option<Dimension>,
    pub op: bool,
    pub whitelisted: bool,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BanEntry {
    pub uuid: Option<String>,
    pub name: String,
    pub reason: String,
    pub banned_by: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerPlayers {
    pub players: Vec<Player>,
    pub whitelist_enabled: bool,
    pub whitelist_enforced: bool,
    pub bans: Vec<BanEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum PlayerActionKind {
    Kick,
    Op,
    Deop,
    WhitelistAdd,
    WhitelistRemove,
    Ban,
    Pardon,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlayerActionRequest {
    pub action: PlayerActionKind,
    pub name: String,
    #[ts(optional)]
    pub reason: Option<String>,
}

/* ------------------------------------------------------------------------ */
/* Backups                                                                  */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BackupKind {
    Manual,
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BackupStatus {
    Complete,
    Running,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Backup {
    pub id: String,
    pub world_id: String,
    pub name: String,
    pub kind: BackupKind,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub status: BackupStatus,
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupSchedule {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub retention: u32,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateScheduleRequest {
    #[ts(optional)]
    pub enabled: Option<bool>,
    #[ts(optional)]
    pub interval_minutes: Option<u32>,
    #[ts(optional)]
    pub retention: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupStorage {
    pub path: String,
    #[ts(type = "number")]
    pub used_bytes: u64,
    #[ts(type = "number")]
    pub quota_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupIndex {
    pub backups: Vec<Backup>,
    pub schedule: BackupSchedule,
    pub storage: BackupStorage,
}

/* ------------------------------------------------------------------------ */
/* Content: mods & plugins (owned by a profile)                             */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ModKind {
    Plugin,
    Mod,
}

/// Where a mod or plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ContentSource {
    Modrinth,
    Hangar,
    Curseforge,
    /// A jar the user uploaded.
    Upload,
}

impl ContentSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::Hangar => "Hangar",
            Self::Curseforge => "CurseForge",
            Self::Upload => "Upload",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::Hangar => "hangar",
            Self::Curseforge => "curseforge",
            Self::Upload => "upload",
        }
    }
}

/// A searchable source of mods and plugins, with what it can offer the
/// profile being browsed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentProviderInfo {
    pub id: ContentSource,
    pub label: String,
    /// False when it can't serve this profile (e.g. Hangar has no mods) or
    /// needs setup (CurseForge without an API key); `reason` says why.
    pub available: bool,
    pub reason: Option<String>,
}

/// A project (mod or plugin) in search results.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentProject {
    pub provider: ContentSource,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub page_url: String,
    #[ts(type = "number")]
    pub downloads: u64,
    pub categories: Vec<String>,
    pub kind: ModKind,
    /// Already in the profile being browsed.
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentSearchResult {
    pub hits: Vec<ContentProject>,
    #[ts(type = "number")]
    pub total: u64,
    #[ts(type = "number")]
    pub offset: u64,
    #[ts(type = "number")]
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum VersionChannel {
    Release,
    Beta,
    Alpha,
}

/// One published version of a project, with its primary file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentVersion {
    pub provider: ContentSource,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub channel: VersionChannel,
    pub published_at: String,
    #[ts(type = "number")]
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub filename: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub dependencies: Vec<ContentDependency>,
    /// Runs on the profile's loader and Minecraft version.
    pub compatible: bool,
}

/// Something to install: a project, optionally pinned to a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentRef {
    pub provider: ContentSource,
    pub project_id: String,
    #[serde(default)]
    #[ts(optional)]
    pub version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentPlanRequest {
    pub items: Vec<ContentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstalledVersion {
    pub entry_id: String,
    pub version_name: String,
}

/// One file the plan will install.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanItem {
    pub provider: ContentSource,
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    pub filename: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub icon_url: Option<String>,
    /// `None` when asked for; the project that needs it otherwise.
    pub required_by: Option<String>,
    /// The installed version this replaces.
    pub replaces: Option<InstalledVersion>,
}

/// An optional dependency the user may add.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanSuggestion {
    pub provider: ContentSource,
    pub project_id: String,
    pub name: String,
    pub suggested_by: String,
}

/// Something that stops the plan from being installed as is.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanConflict {
    pub name: String,
    pub message: String,
}

/// What installing the requested projects takes: the files (requested
/// plus required dependencies), optional extras, and blocking conflicts.
/// Nothing is installed until the plan's items are applied.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallPlan {
    pub install: Vec<PlanItem>,
    pub optional: Vec<PlanSuggestion>,
    pub conflicts: Vec<PlanConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApplyContentRequest {
    /// Exact versions, normally a plan's `install` list.
    pub items: Vec<ContentRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AvailableUpdate {
    pub version_id: String,
    pub version_name: String,
}

/// An installed mod or plugin: one line of the profile's lockfile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentEntry {
    pub id: String,
    pub provider: ContentSource,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub name: String,
    pub version_name: String,
    pub filename: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub kind: ModKind,
    pub author: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub page_url: Option<String>,
    pub enabled: bool,
    /// Installed because another entry needs it.
    pub dependency: bool,
    /// Names of installed entries that need this one.
    pub required_by: Vec<String>,
    /// Built for the profile's loader and Minecraft version. Uploads are
    /// assumed to be.
    pub compatible: bool,
    pub update: Option<AvailableUpdate>,
    pub installed_at: String,
}

/// A profile's mods or plugins.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContentIndex {
    /// `vanilla` means the software can't load mods or plugins.
    pub kind: ContentKind,
    /// Loader names the providers know (`paper`, `fabric`, ...).
    pub loaders: Vec<String>,
    pub game_version: String,
    pub entries: Vec<ContentEntry>,
    /// When updates were last checked.
    pub checked_at: Option<String>,
    /// Where mods or plugins for this profile can be found.
    pub providers: Vec<ContentProviderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateContentRequest {
    #[serde(default)]
    #[ts(optional)]
    pub enabled: Option<bool>,
}

/* ------------------------------------------------------------------------ */
/* Jobs, events, settings, system                                           */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum JobKind {
    Prepare,
    Backup,
    Restore,
    SwitchWorld,
    /// Downloading mods or plugins into a profile.
    Install,
    /// Importing a modpack (`.mrpack`) as a new profile.
    Import,
}

/// Long-running work, e.g. downloading a server jar before first start.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    /// The asset this job works on (an instance or world id).
    pub target_id: String,
    pub state: JobState,
    /// 0..1, or `null` when the amount of work is unknown.
    pub progress: Option<f64>,
    /// Short human status, e.g. `Downloading Paper 1.21.4`.
    pub message: String,
    pub error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ResourceKind {
    Worlds,
    Profiles,
    Instances,
    Backups,
    Settings,
}

/// Payloads on the global `GET /events` SSE stream (event name `lodestone`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum LodestoneEvent {
    /// Something in a collection changed. `id` names the asset when known.
    #[serde(rename_all = "camelCase")]
    Changed { resource: ResourceKind, id: Option<String> },
    /// An asset was removed.
    #[serde(rename_all = "camelCase")]
    Deleted { resource: ResourceKind, id: String },
    #[serde(rename_all = "camelCase")]
    InstanceState { id: String, state: ServerState },
    #[serde(rename_all = "camelCase")]
    Job { job: Job },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppSettings {
    /// Host players type to connect, e.g. `play.example.com`. Blank = `localhost`.
    pub public_address: String,
    pub default_java_args: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateSettingsRequest {
    #[ts(optional)]
    pub public_address: Option<String>,
    #[ts(optional)]
    pub default_java_args: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AuthMode {
    /// Sign-in through the identity provider (Authentik).
    Oidc,
    /// `--dev-user`: everyone is one admin. Development only.
    Dev,
}

/// How to sign in. Public: nothing here is secret.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuthInfo {
    pub mode: AuthMode,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserInfo {
    pub id: String,
    /// The provider's username.
    pub username: String,
    pub display_name: String,
    /// Manages Lodestone's settings and can open anyone's assets.
    pub admin: bool,
}

/// The signed-in user.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Me {
    pub user: UserInfo,
    /// This user has accepted the Minecraft EULA (https://aka.ms/MinecraftEULA),
    /// which their servers need to start.
    pub eula_accepted: bool,
    pub auth: AuthInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateMeRequest {
    #[ts(optional)]
    pub eula_accepted: Option<bool>,
}

/// A user, for admins.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserSummary {
    pub user: UserInfo,
    pub eula_accepted: bool,
    #[ts(type = "number")]
    pub worlds: u64,
    #[ts(type = "number")]
    pub profiles: u64,
    #[ts(type = "number")]
    pub instances: u64,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RuntimeInfo {
    /// `docker`, or `fake` in dev/test.
    pub kind: String,
    pub available: bool,
    pub version: Option<String>,
    /// Why the runtime is unavailable, phrased for the admin.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemInfo {
    pub version: String,
    pub runtime: RuntimeInfo,
    /// `real` downloads from Mojang/PaperMC/Fabric/...; `fake` serves stand-ins.
    pub providers: String,
    pub data_dir: String,
    #[ts(type = "number")]
    pub data_dir_bytes: u64,
    pub host: HostUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub auth: AuthInfo,
}

/// Every error response: `{ "error": { "code", "message" } }`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiErrorDetail {
    /// Stable machine-readable code, e.g. `not_found`, `eula_required`.
    pub code: String,
    /// Sentence written for the person using the UI.
    pub message: String,
}
