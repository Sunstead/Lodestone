/**
 * Wire types for the Lodestone backend (Rust + Axum).
 *
 * Every backend-owned shape is generated from the Rust DTOs in
 * `backend/crates/lodestone/src/model.rs` by ts-rs (`just gen-types`, or
 * any `cargo test`) into `./generated/`. Do not edit those files; change
 * the Rust type and regenerate. CI fails when they drift.
 *
 * Lodestone keeps three concepts apart:
 *
 *   World    persistent, independent asset (region/level data on disk)
 *   Profile  software, version, mods, settings, resource limits
 *   Instance a profile + a world running in one isolated Docker container
 *
 * Deleting an instance never loses a world or a profile.
 *
 * Endpoint map (all under `/api/v1`, errors are `ApiErrorBody`):
 *
 * Everything but `/health` needs a signed-in user (session cookie from
 * `/auth/login`, or a bearer token). Cookie requests that change things
 * send `X-Lodestone-Request: 1`. Lists show the caller's own assets.
 *
 *   GET    /health                                 -> HealthResponse (how to sign in)
 *   GET    /me  PATCH /me                          -> Me ({ eulaAccepted })
 *   GET    /users                                  -> UserSummary[] (admins)
 *   GET    /events                                 -> SSE `lodestone`: LodestoneEvent
 *   GET    /jobs                                   -> Job[]
 *   GET    /jobs/:id                               -> Job
 *   GET    /settings  PATCH /settings              -> AppSettings
 *   GET    /system                                 -> SystemInfo
 *   GET    /backups                                -> Backup[] (every world)
 *
 *   GET    /worlds                                 -> World[]
 *   POST   /worlds                                 -> World
 *   GET    /worlds/:id                             -> World
 *   PATCH  /worlds/:id                             -> World
 *   DELETE /worlds/:id                             -> 204
 *   POST   /worlds/:id/duplicate                   -> World
 *   GET    /worlds/:id/status                      -> WorldStatus
 *   GET    /worlds/:id/backups                     -> BackupIndex
 *   POST   /worlds/:id/backups                     -> Backup (202, runs in background)
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
 *   PUT    /profiles/:id/properties                -> { changes } -> PropertyFile
 *   POST   /profiles/import                        -> multipart .mrpack -> Job (targetId = new profile)
 *   GET    /profiles/:id/content                   -> ContentIndex
 *   GET    /profiles/:id/content/search?q&provider&sort&offset -> ContentSearchResult
 *   GET    /profiles/:id/content/projects/:provider/:project/versions -> ContentVersion[]
 *   POST   /profiles/:id/content/plan              -> ContentPlanRequest -> InstallPlan
 *   POST   /profiles/:id/content                   -> ApplyContentRequest -> ContentIndex
 *   PATCH  /profiles/:id/content/:entry            -> { enabled } -> ContentIndex
 *   DELETE /profiles/:id/content/:entry            -> ContentIndex
 *   POST   /profiles/:id/content/:entry/update     -> ContentIndex
 *   POST   /profiles/:id/content/check-updates     -> ContentIndex
 *   POST   /profiles/:id/content/upload            -> multipart .jar -> ContentIndex
 *
 *   GET    /instances                              -> Instance[]
 *   POST   /instances                              -> CreateInstanceRequest -> Instance
 *   GET    /instances/:id                          -> Instance
 *   PATCH  /instances/:id                          -> Instance
 *   DELETE /instances/:id                          -> 204 (world and profile are kept)
 *   POST   /instances/:id/world                    -> { worldId } -> Instance
 *   POST   /instances/:id/actions                  -> { action: start|stop|restart } -> Instance
 *   GET    /instances/:id/status                   -> InstanceStatus
 *   GET    /instances/:id/console?tail=500         -> ConsoleLine[]
 *   GET    /instances/:id/console/stream           -> SSE: `console`, `status`
 *   POST   /instances/:id/console                  -> { command } -> ConsoleLine[]
 *   GET    /instances/:id/players                  -> ServerPlayers
 *   POST   /instances/:id/players                  -> PlayerAction -> ServerPlayers
 */

export type { ActionRequest } from './generated/ActionRequest';
export type { ApiErrorBody } from './generated/ApiErrorBody';
export type { ApiErrorDetail } from './generated/ApiErrorDetail';
export type { AppSettings } from './generated/AppSettings';
export type { ApplyContentRequest } from './generated/ApplyContentRequest';
export type { AuthInfo } from './generated/AuthInfo';
export type { AuthMode } from './generated/AuthMode';
export type { AvailableUpdate } from './generated/AvailableUpdate';
export type { Backup } from './generated/Backup';
export type { BackupIndex } from './generated/BackupIndex';
export type { BackupKind } from './generated/BackupKind';
export type { BackupSchedule } from './generated/BackupSchedule';
export type { BackupStatus } from './generated/BackupStatus';
export type { BackupStorage } from './generated/BackupStorage';
export type { BanEntry } from './generated/BanEntry';
export type { CommandRequest } from './generated/CommandRequest';
export type { ConsoleLine } from './generated/ConsoleLine';
export type { ConsoleSource } from './generated/ConsoleSource';
export type { ContainerInfo } from './generated/ContainerInfo';
export type { ContentDependency } from './generated/ContentDependency';
export type { ContentEntry } from './generated/ContentEntry';
export type { ContentIndex } from './generated/ContentIndex';
export type { ContentKind } from './generated/ContentKind';
export type { ContentPlanRequest } from './generated/ContentPlanRequest';
export type { ContentProject } from './generated/ContentProject';
export type { ContentProviderInfo } from './generated/ContentProviderInfo';
export type { ContentRef } from './generated/ContentRef';
export type { ContentSearchResult } from './generated/ContentSearchResult';
export type { ContentSource } from './generated/ContentSource';
export type { ContentVersion } from './generated/ContentVersion';
export type { CreateInstanceRequest } from './generated/CreateInstanceRequest';
export type { CreateProfileRequest } from './generated/CreateProfileRequest';
export type { CreateWorldRequest } from './generated/CreateWorldRequest';
export type { DataPacks } from './generated/DataPacks';
export type { DependencyKind } from './generated/DependencyKind';
export type { Difficulty } from './generated/Difficulty';
export type { Dimension } from './generated/Dimension';
export type { Dimensions } from './generated/Dimensions';
export type { DiskUsage } from './generated/DiskUsage';
export type { GameMode } from './generated/GameMode';
export type { HealthResponse } from './generated/HealthResponse';
export type { HostUsage } from './generated/HostUsage';
export type { InstallPlan } from './generated/InstallPlan';
export type { InstalledVersion } from './generated/InstalledVersion';
export type { DnsStatus } from './generated/DnsStatus';
export type { Instance } from './generated/Instance';
export type { InstanceDns } from './generated/InstanceDns';
export type { InstancePorts } from './generated/InstancePorts';
export type { InstanceProfileRef } from './generated/InstanceProfileRef';
export type { InstanceStatus } from './generated/InstanceStatus';
export type { InstanceWorldRef } from './generated/InstanceWorldRef';
export type { Job } from './generated/Job';
export type { JobKind } from './generated/JobKind';
export type { JobState } from './generated/JobState';
export type { LevelType } from './generated/LevelType';
export type { LodestoneEvent } from './generated/LodestoneEvent';
export type { LogLevel } from './generated/LogLevel';
export type { Me } from './generated/Me';
export type { MemoryUsage } from './generated/MemoryUsage';
export type { ModKind } from './generated/ModKind';
export type { NetworkUsage } from './generated/NetworkUsage';
export type { PlanConflict } from './generated/PlanConflict';
export type { PlanItem } from './generated/PlanItem';
export type { PlanSuggestion } from './generated/PlanSuggestion';
export type { Player } from './generated/Player';
export type { PlayerActionKind } from './generated/PlayerActionKind';
export type { PlayerActionRequest } from './generated/PlayerActionRequest';
export type { PlayerCount } from './generated/PlayerCount';
export type { Profile } from './generated/Profile';
export type { ProfileMemory } from './generated/ProfileMemory';
export type { ProfilePorts } from './generated/ProfilePorts';
export type { PropertyControl } from './generated/PropertyControl';
export type { PropertyField } from './generated/PropertyField';
export type { PropertyFile } from './generated/PropertyFile';
export type { PropertyOption } from './generated/PropertyOption';
export type { PropertyScope } from './generated/PropertyScope';
export type { ResourceKind } from './generated/ResourceKind';
export type { RuntimeInfo } from './generated/RuntimeInfo';
export type { SavePropertiesRequest } from './generated/SavePropertiesRequest';
export type { ServerAction } from './generated/ServerAction';
export type { ServerPlayers } from './generated/ServerPlayers';
export type { ServerSoftwareId } from './generated/ServerSoftwareId';
export type { ServerState } from './generated/ServerState';
export type { SoftwareCatalog } from './generated/SoftwareCatalog';
export type { SoftwareOption } from './generated/SoftwareOption';
export type { SoftwareVersion } from './generated/SoftwareVersion';
export type { SwitchWorldRequest } from './generated/SwitchWorldRequest';
export type { SystemInfo } from './generated/SystemInfo';
export type { UpdateContentRequest } from './generated/UpdateContentRequest';
export type { UpdateInstanceRequest } from './generated/UpdateInstanceRequest';
export type { UpdateMeRequest } from './generated/UpdateMeRequest';
export type { UpdateProfileRequest } from './generated/UpdateProfileRequest';
export type { UpdateScheduleRequest } from './generated/UpdateScheduleRequest';
export type { UpdateSettingsRequest } from './generated/UpdateSettingsRequest';
export type { UpdateWorldRequest } from './generated/UpdateWorldRequest';
export type { UserInfo } from './generated/UserInfo';
export type { UserSummary } from './generated/UserSummary';
export type { VersionChannel } from './generated/VersionChannel';
export type { World } from './generated/World';
export type { WorldLayout } from './generated/WorldLayout';
export type { WorldStatus } from './generated/WorldStatus';

/* Frontend-only types ---------------------------------------------------- */

export type ApiMode = 'live' | 'demo';

/** A player action, narrowed per action (the wire type is `PlayerActionRequest`). */
export type PlayerAction =
  | { action: 'kick'; name: string; reason?: string }
  | { action: 'op' | 'deop'; name: string }
  | { action: 'whitelist-add' | 'whitelist-remove'; name: string }
  | { action: 'ban' | 'pardon'; name: string; reason?: string };
