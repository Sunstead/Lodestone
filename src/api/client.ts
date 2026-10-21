/**
 * Lodestone API client.
 *
 * Speaks to the Rust/Axum backend at `/api/v1`. If the service is not
 * reachable (the common case while only the frontend exists) every call
 * is served by the in-memory demo backend in `demo.ts`, so the UI is
 * fully explorable without Docker.
 *
 * The probe runs once at startup and the `mode` is exposed through
 * `getMode()`/`onModeChange()` so the shell can show a "Demo data" badge.
 */

import * as demo from './demo';
import type {
  ApiMode,
  Backup,
  BackupIndex,
  ConsoleLine,
  CreateInstanceRequest,
  CreateProfileRequest,
  CreateWorldRequest,
  Instance,
  InstanceStatus,
  ModIndex,
  Profile,
  PropertyFile,
  ServerAction,
  ServerPlayers,
  SoftwareCatalog,
  World,
  WorldStatus,
} from './types';

const BASE = import.meta.env.VITE_API_BASE ?? '/api/v1';

let mode: ApiMode = 'demo';
let probe: Promise<ApiMode> | null = null;
const listeners = new Set<(mode: ApiMode) => void>();

export function getMode(): ApiMode {
  return mode;
}

export function onModeChange(listener: (mode: ApiMode) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function setMode(next: ApiMode): void {
  if (mode === next) return;
  mode = next;
  listeners.forEach((listener) => listener(next));
}

/** Resolves to 'live' when the Axum service answers `/health`, else 'demo'. */
export function detectMode(): Promise<ApiMode> {
  if (probe) return probe;
  probe = fetch(`${BASE}/health`, { headers: { accept: 'application/json' } })
    .then((response): ApiMode => (response.ok ? 'live' : 'demo'))
    .catch((): ApiMode => 'demo')
    .then((next) => {
      setMode(next);
      return next;
    });
  return probe;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    ...init,
  });
  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new ApiError(body || `${response.status} ${response.statusText}`, response.status);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

const json = (body: unknown): RequestInit => ({ method: 'POST', body: JSON.stringify(body) });
const patch = (body: unknown): RequestInit => ({ method: 'PATCH', body: JSON.stringify(body) });

/* -------------------------------- worlds ------------------------------- */

export function listWorlds(): Promise<World[]> {
  return mode === 'live' ? request<World[]>('/worlds') : Promise.resolve(demo.getWorlds());
}

export function getWorld(id: string): Promise<World> {
  return mode === 'live' ? request<World>(`/worlds/${id}`) : Promise.resolve(demo.getWorld(id));
}

export function createWorld(body: CreateWorldRequest): Promise<World> {
  return mode === 'live' ? request<World>('/worlds', json(body)) : Promise.resolve(demo.createWorld(body));
}

export function updateWorld(id: string, body: Partial<CreateWorldRequest>): Promise<World> {
  return mode === 'live' ? request<World>(`/worlds/${id}`, patch(body)) : Promise.resolve(demo.updateWorld(id, body));
}

export function duplicateWorld(id: string): Promise<World> {
  return mode === 'live'
    ? request<World>(`/worlds/${id}/duplicate`, { method: 'POST' })
    : Promise.resolve(demo.duplicateWorld(id));
}

export function deleteWorld(id: string): Promise<void> {
  return mode === 'live' ? request<void>(`/worlds/${id}`, { method: 'DELETE' }) : Promise.resolve(demo.deleteWorld(id));
}

export function getWorldStatus(id: string): Promise<WorldStatus> {
  return mode === 'live'
    ? request<WorldStatus>(`/worlds/${id}/status`)
    : Promise.resolve(demo.getWorldStatus(id));
}

/* ------------------------------- profiles ------------------------------ */

export function listProfiles(): Promise<Profile[]> {
  return mode === 'live' ? request<Profile[]>('/profiles') : Promise.resolve(demo.getProfiles());
}

export function getProfile(id: string): Promise<Profile> {
  return mode === 'live' ? request<Profile>(`/profiles/${id}`) : Promise.resolve(demo.getProfile(id));
}

export function getSoftware(): Promise<SoftwareCatalog> {
  return mode === 'live' ? request<SoftwareCatalog>('/profiles/software') : Promise.resolve(demo.getSoftware());
}

export function createProfile(body: CreateProfileRequest): Promise<Profile> {
  return mode === 'live'
    ? request<Profile>('/profiles', json(body))
    : Promise.resolve(demo.createProfile(body));
}

export function updateProfile(id: string, body: Partial<CreateProfileRequest>): Promise<Profile> {
  return mode === 'live'
    ? request<Profile>(`/profiles/${id}`, patch(body))
    : Promise.resolve(demo.updateProfile(id, body));
}

export function duplicateProfile(id: string): Promise<Profile> {
  return mode === 'live'
    ? request<Profile>(`/profiles/${id}/duplicate`, { method: 'POST' })
    : Promise.resolve(demo.duplicateProfile(id));
}

export function deleteProfile(id: string): Promise<void> {
  return mode === 'live'
    ? request<void>(`/profiles/${id}`, { method: 'DELETE' })
    : Promise.resolve(demo.deleteProfile(id));
}

export function getProfileProperties(id: string): Promise<PropertyFile> {
  return mode === 'live'
    ? request<PropertyFile>(`/profiles/${id}/properties`)
    : Promise.resolve(demo.getProfileProperties(id));
}

export function saveProfileProperties(id: string, changes: Record<string, string>): Promise<PropertyFile> {
  return mode === 'live'
    ? request<PropertyFile>(`/profiles/${id}/properties`, { method: 'PUT', body: JSON.stringify({ changes }) })
    : Promise.resolve(demo.saveProfileProperties(id, changes));
}

export function getProfileMods(id: string): Promise<ModIndex> {
  return mode === 'live'
    ? request<ModIndex>(`/profiles/${id}/mods`)
    : Promise.resolve(demo.getProfileMods(id));
}

export function installMod(id: string, modId: string): Promise<ModIndex> {
  return mode === 'live'
    ? request<ModIndex>(`/profiles/${id}/mods`, json({ modId }))
    : Promise.resolve(demo.installMod(id, modId));
}

export function uninstallMod(id: string, modId: string): Promise<ModIndex> {
  return mode === 'live'
    ? request<ModIndex>(`/profiles/${id}/mods/${modId}`, { method: 'DELETE' })
    : Promise.resolve(demo.uninstallMod(id, modId));
}

export function patchMod(id: string, modId: string, enabled: boolean): Promise<ModIndex> {
  return mode === 'live'
    ? request<ModIndex>(`/profiles/${id}/mods/${modId}`, patch({ enabled }))
    : Promise.resolve(demo.patchMod(id, modId, enabled));
}

export function updateMod(id: string, modId: string): Promise<ModIndex> {
  return mode === 'live'
    ? request<ModIndex>(`/profiles/${id}/mods/${modId}/update`, { method: 'POST' })
    : Promise.resolve(demo.updateMod(id, modId));
}

/* ------------------------------ instances ------------------------------ */

export function listInstances(): Promise<Instance[]> {
  return mode === 'live' ? request<Instance[]>('/instances') : Promise.resolve(demo.getInstances());
}

export function getInstance(id: string): Promise<Instance> {
  return mode === 'live' ? request<Instance>(`/instances/${id}`) : Promise.resolve(demo.getInstance(id));
}

export function createInstance(body: CreateInstanceRequest): Promise<Instance> {
  return mode === 'live'
    ? request<Instance>('/instances', json(body))
    : Promise.resolve(demo.createInstance(body));
}

export function updateInstance(id: string, body: Partial<CreateInstanceRequest>): Promise<Instance> {
  return mode === 'live'
    ? request<Instance>(`/instances/${id}`, patch(body))
    : Promise.resolve(demo.updateInstance(id, body));
}

export function deleteInstance(id: string): Promise<void> {
  return mode === 'live'
    ? request<void>(`/instances/${id}`, { method: 'DELETE' })
    : Promise.resolve(demo.deleteInstance(id));
}

/** Rebinds an instance to another world, restarting it if it was running. */
export function switchInstanceWorld(id: string, worldId: string): Promise<Instance> {
  return mode === 'live'
    ? request<Instance>(`/instances/${id}/world`, json({ worldId }))
    : Promise.resolve(demo.switchInstanceWorld(id, worldId));
}

export function getInstanceStatus(id: string): Promise<InstanceStatus> {
  if (mode === 'live') return request<InstanceStatus>(`/instances/${id}/status`);
  return statusFallback(id);
}

/** Demo instances keep metrics inside the demo store; this keeps polling cheap. */
async function statusFallback(id: string): Promise<InstanceStatus> {
  const { getStatusSnapshot } = await import('./demo-status');
  return getStatusSnapshot(id);
}

export function runAction(id: string, action: ServerAction): Promise<Instance> {
  return mode === 'live'
    ? request<Instance>(`/instances/${id}/actions`, json({ action }))
    : Promise.resolve(demo.postAction(id, action));
}

/* ------------------------------ console -------------------------------- */

export function getConsole(id: string, tail = 500): Promise<ConsoleLine[]> {
  return mode === 'live'
    ? request<ConsoleLine[]>(`/instances/${id}/console?tail=${tail}`)
    : Promise.resolve(demo.getConsole(id, tail));
}

export function sendCommand(id: string, command: string): Promise<ConsoleLine[]> {
  return mode === 'live'
    ? request<ConsoleLine[]>(`/instances/${id}/console`, json({ command }))
    : Promise.resolve(demo.sendCommand(id, command));
}

/**
 * Live console stream. Falls back to the demo store's emitter.
 * Returns an unsubscribe function.
 */
export function subscribeConsole(
  id: string,
  handlers: {
    onLine: (line: ConsoleLine) => void;
    onStatus?: (status: InstanceStatus) => void;
    onError?: (error: Event) => void;
  },
): () => void {
  if (mode === 'live') {
    const source = new EventSource(`${BASE}/instances/${id}/console/stream`);
    source.addEventListener('console', (event) => {
      try {
        handlers.onLine(JSON.parse((event as MessageEvent).data) as ConsoleLine);
      } catch {
        /* ignore malformed frames */
      }
    });
    if (handlers.onStatus) {
      source.addEventListener('status', (event) => {
        try {
          handlers.onStatus?.(JSON.parse((event as MessageEvent).data) as InstanceStatus);
        } catch {
          /* ignore malformed frames */
        }
      });
    }
    source.onerror = (error) => handlers.onError?.(error);
    return () => source.close();
  }

  const unlisten = demo.subscribeConsole(id, handlers.onLine);
  const ticker = setInterval(async () => {
    handlers.onStatus?.(await statusFallback(id));
  }, 3000);
  return () => {
    unlisten();
    clearInterval(ticker);
  };
}

/* ------------------------------- players ------------------------------- */

export function getPlayers(id: string): Promise<ServerPlayers> {
  return mode === 'live'
    ? request<ServerPlayers>(`/instances/${id}/players`)
    : Promise.resolve(demo.getPlayers(id));
}

export function postPlayerAction(
  id: string,
  body: { action: string; name: string; reason?: string },
): Promise<ServerPlayers> {
  return mode === 'live'
    ? request<ServerPlayers>(`/instances/${id}/players`, json(body))
    : Promise.resolve(demo.postPlayerAction(id, body));
}

/* ------------------------------- backups ------------------------------- */

export function getBackups(worldId: string): Promise<BackupIndex> {
  return mode === 'live'
    ? request<BackupIndex>(`/worlds/${worldId}/backups`)
    : Promise.resolve(demo.getBackups(worldId));
}

export function createBackup(worldId: string): Promise<Backup> {
  return mode === 'live'
    ? request<Backup>(`/worlds/${worldId}/backups`, { method: 'POST' })
    : Promise.resolve(demo.createBackup(worldId));
}

export function deleteBackup(worldId: string, backupId: string): Promise<void> {
  return mode === 'live'
    ? request<void>(`/worlds/${worldId}/backups/${backupId}`, { method: 'DELETE' })
    : Promise.resolve(demo.deleteBackup(worldId, backupId));
}

export function restoreBackup(worldId: string, backupId: string): Promise<void> {
  return mode === 'live'
    ? request<void>(`/worlds/${worldId}/backups/${backupId}/restore`, { method: 'POST' })
    : Promise.resolve(demo.restoreBackup(worldId, backupId));
}

export function patchSchedule(worldId: string, patchBody: Partial<BackupIndex['schedule']>): Promise<BackupIndex> {
  return mode === 'live'
    ? request<BackupIndex>(`/worlds/${worldId}/backups/schedule`, patch(patchBody))
    : Promise.resolve(demo.patchSchedule(worldId, patchBody));
}
