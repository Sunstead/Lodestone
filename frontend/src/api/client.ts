/**
 * Lodestone API client.
 *
 * Speaks to the Rust/Axum backend at `/api/v1`. The mode decides what
 * happens when it is not reachable:
 *
 *   VITE_LODESTONE_MODE=demo  always use the in-memory demo backend
 *   VITE_LODESTONE_MODE=live  always use the backend; if it is down the UI
 *                             says so instead of showing pretend servers
 *   VITE_LODESTONE_MODE=auto  probe `/health` once and fall back to demo
 *                             (the dev default)
 *
 * Production builds default to `live`, so nobody "creates" a demo server
 * believing it is real.
 */

import * as demo from './demo';
import type {
  ApiErrorBody,
  ApiMode,
  AppSettings,
  Backup,
  BackupIndex,
  ConsoleLine,
  CreateInstanceRequest,
  CreateProfileRequest,
  CreateWorldRequest,
  Instance,
  InstanceStatus,
  Job,
  LodestoneEvent,
  ContentIndex,
  ContentPlanRequest,
  ContentRef,
  ContentSearchResult,
  ContentSource,
  ContentVersion,
  InstallPlan,
  Me,
  UpdateMeRequest,
  UserSummary,
  PlayerAction,
  Profile,
  PropertyFile,
  ServerAction,
  ServerPlayers,
  SoftwareCatalog,
  SystemInfo,
  UpdateInstanceRequest,
  UpdateProfileRequest,
  UpdateScheduleRequest,
  UpdateSettingsRequest,
  UpdateWorldRequest,
  World,
  WorldStatus,
} from './types';

const BASE = import.meta.env.VITE_API_BASE ?? '/api/v1';

export type ModeSetting = 'demo' | 'live' | 'auto';

export const modeSetting: ModeSetting = (() => {
  const raw = import.meta.env.VITE_LODESTONE_MODE;
  if (raw === 'demo' || raw === 'live' || raw === 'auto') return raw;
  return import.meta.env.PROD ? 'live' : 'auto';
})();

let mode: ApiMode = modeSetting === 'live' ? 'live' : 'demo';
let probe: Promise<ApiMode> | null = null;
let backendReachable: boolean | null = null;
const listeners = new Set<(mode: ApiMode) => void>();

export function getMode(): ApiMode {
  return mode;
}

/** `false` when in live mode and the backend did not answer `/health`. */
export function isBackendReachable(): boolean | null {
  return backendReachable;
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

/** For tests: force a mode without probing. */
export function forceMode(next: ApiMode): void {
  probe = Promise.resolve(next);
  backendReachable = next === 'live';
  setMode(next);
}

/** Probes `/health` once (again when `retry` is set) and settles the mode. */
export function detectMode(retry = false): Promise<ApiMode> {
  if (modeSetting === 'demo') {
    setMode('demo');
    return Promise.resolve('demo');
  }
  if (probe && !retry) return probe;
  probe = fetch(`${BASE}/health`, { headers: { accept: 'application/json' } })
    .then((response) => response.ok)
    .catch(() => false)
    .then((ok): ApiMode => {
      backendReachable = ok;
      const next: ApiMode = ok || modeSetting === 'live' ? 'live' : 'demo';
      // Notify even when the mode is unchanged: reachability may have flipped.
      mode = next;
      listeners.forEach((listener) => listener(next));
      // Live mode keeps knocking until the backend is up (e.g. still compiling).
      if (!ok && modeSetting === 'live' && typeof window !== 'undefined') {
        setTimeout(() => void detectMode(true), 3000);
      }
      return next;
    });
  return probe;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** Stable code from the backend, e.g. `eula_required`, `world_attached`. */
    readonly code: string = 'unknown',
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/**
 * Sends the browser to sign in, then back here. Only the backend knows the
 * identity provider, so this is a plain navigation to `/auth/login`.
 */
export function signIn(): void {
  if (typeof window === 'undefined') return;
  const back = `${window.location.pathname}${window.location.search}`;
  window.location.assign(`/auth/login?return_to=${encodeURIComponent(back)}`);
}

/** Ends the session, then follows the provider's sign-out page if it has one. */
export async function signOut(): Promise<void> {
  const response = await fetch('/auth/logout', { method: 'POST', headers: { 'x-lodestone-request': '1' } });
  const body = (await response.json().catch(() => ({}))) as { redirect?: string };
  if (typeof window !== 'undefined') window.location.assign(body.redirect ?? '/');
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers: Record<string, string> = {
    accept: 'application/json',
    // Required on cookie-authenticated changes (cross-site request guard).
    'x-lodestone-request': '1',
  };
  if (!(init?.body instanceof FormData)) headers['content-type'] = 'application/json';
  const response = await fetch(`${BASE}${path}`, { ...init, headers });
  if (response.status === 401) {
    // The session ended: sign in again and come back.
    signIn();
  }
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    try {
      const body = JSON.parse(text) as ApiErrorBody;
      throw new ApiError(body.error.message, response.status, body.error.code);
    } catch (error) {
      if (error instanceof ApiError) throw error;
      throw new ApiError(text || `${response.status} ${response.statusText}`, response.status);
    }
  }
  if (response.status === 204 || response.status === 202) {
    const text = await response.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }
  return (await response.json()) as T;
}

/** Runs a demo call so a thrown error becomes a rejected promise, like HTTP. */
function local<T>(fn: () => T): Promise<T> {
  return new Promise<T>((resolve) => resolve(fn()));
}

const live = (): boolean => mode === 'live';
const post = (body?: unknown): RequestInit => ({ method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) });
const patch = (body: unknown): RequestInit => ({ method: 'PATCH', body: JSON.stringify(body) });
const del: RequestInit = { method: 'DELETE' };

/* -------------------------------- worlds ------------------------------- */

export const listWorlds = (): Promise<World[]> => (live() ? request('/worlds') : local(demo.getWorlds));
export const getWorld = (id: string): Promise<World> => (live() ? request(`/worlds/${id}`) : local(() => demo.getWorld(id)));
export const createWorld = (body: CreateWorldRequest): Promise<World> =>
  live() ? request('/worlds', post(body)) : local(() => demo.createWorld(body));
export const updateWorld = (id: string, body: UpdateWorldRequest): Promise<World> =>
  live() ? request(`/worlds/${id}`, patch(body)) : local(() => demo.updateWorld(id, body));
export const duplicateWorld = (id: string): Promise<World> =>
  live() ? request(`/worlds/${id}/duplicate`, post()) : local(() => demo.duplicateWorld(id));
export const deleteWorld = (id: string): Promise<void> =>
  live() ? request(`/worlds/${id}`, del) : local(() => demo.deleteWorld(id));
export const getWorldStatus = (id: string): Promise<WorldStatus> =>
  live() ? request(`/worlds/${id}/status`) : local(() => demo.getWorldStatus(id));

/* ------------------------------- profiles ------------------------------ */

export const listProfiles = (): Promise<Profile[]> => (live() ? request('/profiles') : local(demo.getProfiles));
export const getProfile = (id: string): Promise<Profile> =>
  live() ? request(`/profiles/${id}`) : local(() => demo.getProfile(id));
export const getSoftware = (): Promise<SoftwareCatalog> =>
  live() ? request('/profiles/software') : local(demo.getSoftware);
export const createProfile = (body: CreateProfileRequest): Promise<Profile> =>
  live() ? request('/profiles', post(body)) : local(() => demo.createProfile(body));
export const updateProfile = (id: string, body: UpdateProfileRequest): Promise<Profile> =>
  live() ? request(`/profiles/${id}`, patch(body)) : local(() => demo.updateProfile(id, body));
export const duplicateProfile = (id: string): Promise<Profile> =>
  live() ? request(`/profiles/${id}/duplicate`, post()) : local(() => demo.duplicateProfile(id));
export const deleteProfile = (id: string): Promise<void> =>
  live() ? request(`/profiles/${id}`, del) : local(() => demo.deleteProfile(id));
export const getProfileProperties = (id: string): Promise<PropertyFile> =>
  live() ? request(`/profiles/${id}/properties`) : local(() => demo.getProfileProperties(id));
export const saveProfileProperties = (id: string, changes: Record<string, string>): Promise<PropertyFile> =>
  live()
    ? request(`/profiles/${id}/properties`, { method: 'PUT', body: JSON.stringify({ changes }) })
    : local(() => demo.saveProfileProperties(id, changes));

/* ------------------------- mods and plugins ---------------------------- */

const content = (id: string, rest = ''): string => `/profiles/${id}/content${rest}`;

export const getContent = (id: string): Promise<ContentIndex> =>
  live() ? request(content(id)) : local(() => demo.getContent(id));

export type ContentSearchParams = {
  q: string;
  provider?: ContentSource;
  sort?: 'relevance' | 'downloads' | 'updated' | 'newest';
  offset?: number;
  limit?: number;
};

export function searchContent(id: string, params: ContentSearchParams): Promise<ContentSearchResult> {
  if (!live()) return local(() => demo.searchContent(id, params));
  const query = new URLSearchParams({ q: params.q });
  if (params.provider) query.set('provider', params.provider);
  if (params.sort) query.set('sort', params.sort);
  if (params.offset) query.set('offset', String(params.offset));
  if (params.limit) query.set('limit', String(params.limit));
  return request(content(id, `/search?${query}`));
}

export const getContentVersions = (id: string, provider: ContentSource, project: string): Promise<ContentVersion[]> =>
  live()
    ? request(content(id, `/projects/${provider}/${encodeURIComponent(project)}/versions`))
    : local(() => demo.getContentVersions(id, provider, project));
export const planContent = (id: string, body: ContentPlanRequest): Promise<InstallPlan> =>
  live() ? request(content(id, '/plan'), post(body)) : local(() => demo.planContent(id, body));
export const applyContent = (id: string, items: ContentRef[]): Promise<ContentIndex> =>
  live() ? request(content(id), post({ items })) : local(() => demo.applyContent(id, items));
export const setContentEnabled = (id: string, entry: string, enabled: boolean): Promise<ContentIndex> =>
  live() ? request(content(id, `/${entry}`), patch({ enabled })) : local(() => demo.setContentEnabled(id, entry, enabled));
export const removeContent = (id: string, entry: string): Promise<ContentIndex> =>
  live() ? request(content(id, `/${entry}`), del) : local(() => demo.removeContent(id, entry));
export const updateContent = (id: string, entry: string): Promise<ContentIndex> =>
  live() ? request(content(id, `/${entry}/update`), post()) : local(() => demo.updateContent(id, entry));
export const checkContentUpdates = (id: string): Promise<ContentIndex> =>
  live() ? request(content(id, '/check-updates'), post()) : local(() => demo.checkContentUpdates(id));

function fileBody(file: File): FormData {
  const form = new FormData();
  form.append('file', file, file.name);
  return form;
}

export const uploadContent = (id: string, file: File): Promise<ContentIndex> =>
  live()
    ? request(content(id, '/upload'), { method: 'POST', body: fileBody(file) })
    : local(() => demo.uploadContent(id, file.name, file.size));

/** Imports a `.mrpack` as a new configuration; the job's `targetId` is it. */
export const importModpack = (file: File): Promise<Job> =>
  live()
    ? request('/profiles/import', { method: 'POST', body: fileBody(file) })
    : local(() => demo.importModpack(file.name));

/* ------------------------------ instances ------------------------------ */

export const listInstances = (): Promise<Instance[]> => (live() ? request('/instances') : local(demo.getInstances));
export const getInstance = (id: string): Promise<Instance> =>
  live() ? request(`/instances/${id}`) : local(() => demo.getInstance(id));

/** Creates an instance, plus a new world and/or profile in the same step. */
export const createInstance = (body: CreateInstanceRequest): Promise<Instance> =>
  live() ? request('/instances', post(body)) : local(() => demo.createInstance(body));
export const updateInstance = (id: string, body: UpdateInstanceRequest): Promise<Instance> =>
  live() ? request(`/instances/${id}`, patch(body)) : local(() => demo.updateInstance(id, body));
export const deleteInstance = (id: string): Promise<void> =>
  live() ? request(`/instances/${id}`, del) : local(() => demo.deleteInstance(id));

/** Rebinds an instance to another world, restarting it if it was running. */
export const switchInstanceWorld = (id: string, worldId: string): Promise<Instance> =>
  live() ? request(`/instances/${id}/world`, post({ worldId })) : local(() => demo.switchInstanceWorld(id, worldId));

export function getInstanceStatus(id: string): Promise<InstanceStatus> {
  if (live()) return request(`/instances/${id}/status`);
  return statusFallback(id);
}

/** Demo instances keep metrics inside the demo store; this keeps polling cheap. */
async function statusFallback(id: string): Promise<InstanceStatus> {
  const { getStatusSnapshot } = await import('./demo-status');
  return getStatusSnapshot(id);
}

export const runAction = (id: string, action: ServerAction): Promise<Instance> =>
  live() ? request(`/instances/${id}/actions`, post({ action })) : local(() => demo.postAction(id, action));

/* ------------------------------ console -------------------------------- */

export const getConsole = (id: string, tail = 500): Promise<ConsoleLine[]> =>
  live() ? request(`/instances/${id}/console?tail=${tail}`) : local(() => demo.getConsole(id, tail));
export const sendCommand = (id: string, command: string): Promise<ConsoleLine[]> =>
  live() ? request(`/instances/${id}/console`, post({ command })) : local(() => demo.sendCommand(id, command));

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
  if (live()) {
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

/**
 * Global change feed (`GET /events`). The demo backend has none: its
 * queries poll instead. Returns an unsubscribe function.
 */
export function subscribeEvents(onEvent: (event: LodestoneEvent) => void, onOpen?: () => void): () => void {
  if (!live()) return () => {};
  const source = new EventSource(`${BASE}/events`);
  source.addEventListener('lodestone', (event) => {
    try {
      onEvent(JSON.parse((event as MessageEvent).data) as LodestoneEvent);
    } catch {
      /* ignore malformed frames */
    }
  });
  if (onOpen) source.onopen = onOpen;
  return () => source.close();
}

/* ------------------------------- players ------------------------------- */

export const getPlayers = (id: string): Promise<ServerPlayers> =>
  live() ? request(`/instances/${id}/players`) : local(() => demo.getPlayers(id));
export const postPlayerAction = (id: string, body: PlayerAction): Promise<ServerPlayers> =>
  live() ? request(`/instances/${id}/players`, post(body)) : local(() => demo.postPlayerAction(id, body));

/* ------------------------------- backups ------------------------------- */

export const listAllBackups = (): Promise<Backup[]> => (live() ? request('/backups') : local(demo.getAllBackups));
export const getBackups = (worldId: string): Promise<BackupIndex> =>
  live() ? request(`/worlds/${worldId}/backups`) : local(() => demo.getBackups(worldId));
export const createBackup = (worldId: string): Promise<Backup> =>
  live() ? request(`/worlds/${worldId}/backups`, post()) : local(() => demo.createBackup(worldId));
export const deleteBackup = (worldId: string, backupId: string): Promise<void> =>
  live() ? request(`/worlds/${worldId}/backups/${backupId}`, del) : local(() => demo.deleteBackup(worldId, backupId));
export const restoreBackup = (worldId: string, backupId: string): Promise<void> =>
  live()
    ? request(`/worlds/${worldId}/backups/${backupId}/restore`, post())
    : local(() => demo.restoreBackup(worldId, backupId));
export const patchSchedule = (worldId: string, body: UpdateScheduleRequest): Promise<BackupIndex> =>
  live() ? request(`/worlds/${worldId}/backups/schedule`, patch(body)) : local(() => demo.patchSchedule(worldId, body));

/* ---------------------------- settings / system ------------------------ */

export const getMe = (): Promise<Me> => (live() ? request('/me') : local(demo.getMe));
export const updateMe = (body: UpdateMeRequest): Promise<Me> =>
  live() ? request('/me', patch(body)) : local(() => demo.updateMe(body));
export const listUsers = (): Promise<UserSummary[]> => (live() ? request('/users') : local(demo.getUsers));

export const getSettings = (): Promise<AppSettings> => (live() ? request('/settings') : local(demo.getSettings));
export const updateSettings = (body: UpdateSettingsRequest): Promise<AppSettings> =>
  live() ? request('/settings', patch(body)) : local(() => demo.updateSettings(body));
export const getSystem = (): Promise<SystemInfo> => (live() ? request('/system') : local(demo.getSystem));
export const listJobs = (): Promise<Job[]> => (live() ? request('/jobs') : local(demo.getJobs));
