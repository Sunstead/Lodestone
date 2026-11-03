/**
 * TanStack Query bindings for the Lodestone API.
 *
 * Three key prefixes mirror the three assets: `worlds`, `profiles` and
 * `instances`. Because an instance renders its profile and world names,
 * mutations on a world or profile also invalidate `instances` so those
 * denormalised labels stay honest.
 */

import { useEffect, useState } from 'react';
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import * as api from './client';
import type {
  Backup,
  BackupIndex,
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
  AppSettings,
  Job,
  LodestoneEvent,
  PlayerAction,
  SystemInfo,
  UpdateInstanceRequest,
  UpdateProfileRequest,
  UpdateScheduleRequest,
  UpdateSettingsRequest,
  UpdateWorldRequest,
  World,
  WorldStatus,
} from './types';

export const keys = {
  worlds: ['worlds'] as const,
  world: (id: string) => ['worlds', id] as const,
  worldStatus: (id: string) => ['worlds', id, 'status'] as const,
  worldBackups: (id: string) => ['worlds', id, 'backups'] as const,

  profiles: ['profiles'] as const,
  profile: (id: string) => ['profiles', id] as const,
  profileProperties: (id: string) => ['profiles', id, 'properties'] as const,
  profileMods: (id: string) => ['profiles', id, 'mods'] as const,
  software: ['profiles', 'software'] as const,

  instances: ['instances'] as const,
  instance: (id: string) => ['instances', id] as const,
  instanceStatus: (id: string) => ['instances', id, 'status'] as const,
  instancePlayers: (id: string) => ['instances', id, 'players'] as const,
  console: (id: string) => ['instances', id, 'console'] as const,

  backups: ['backups'] as const,
  jobs: ['jobs'] as const,
  settings: ['settings'] as const,
  system: ['system'] as const,
};

/* -------------------------------- events ------------------------------- */

/** Which cached queries an event makes stale. */
export function keysForEvent(event: LodestoneEvent): readonly (readonly string[])[] {
  switch (event.type) {
    case 'changed':
    case 'deleted':
      switch (event.resource) {
        case 'worlds':
          return [keys.worlds, keys.instances, keys.backups];
        case 'profiles':
          return [keys.profiles, keys.instances];
        case 'instances':
          return [keys.instances, keys.worlds];
        case 'backups':
          return event.type === 'changed' && event.id ? [keys.worldBackups(event.id), keys.backups] : [keys.backups, keys.worlds];
        case 'settings':
          return [keys.settings, keys.system];
      }
      return [];
    case 'instanceState':
      return [keys.instance(event.id), keys.instanceStatus(event.id), keys.instances];
    case 'job':
      return [keys.jobs];
  }
}

/**
 * Subscribes to the backend's change feed and invalidates the matching
 * queries, so lists and detail pages update without polling. Mount once.
 */
export function useEventStream(): void {
  const queryClient = useQueryClient();
  // The mode settles after the first `/health` probe; subscribe once it is live.
  const [mode, setMode] = useState(api.getMode());
  useEffect(() => {
    const unsubscribe = api.onModeChange(setMode);
    return () => void unsubscribe();
  }, []);
  useEffect(() => {
    if (mode !== 'live') return;
    return api.subscribeEvents(
      (event) => {
        if (event.type === 'job') {
          queryClient.setQueryData<Job[]>(keys.jobs, (jobs) => [
            event.job,
            ...(jobs ?? []).filter((job) => job.id !== event.job.id),
          ]);
        }
        for (const key of keysForEvent(event)) void queryClient.invalidateQueries({ queryKey: key });
      },
      // On (re)connect anything may have changed, including demo → live.
      () => void queryClient.invalidateQueries(),
    );
  }, [queryClient, mode]);
}

/* ---------------------------- settings / system ------------------------ */

export function useSettings(): UseQueryResult<AppSettings> {
  return useQuery({ queryKey: keys.settings, queryFn: api.getSettings });
}

export function useUpdateSettings(): UseMutationResult<AppSettings, Error, UpdateSettingsRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.updateSettings,
    onSuccess: (data) => {
      queryClient.setQueryData(keys.settings, data);
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useSystem(): UseQueryResult<SystemInfo> {
  return useQuery({ queryKey: keys.system, queryFn: api.getSystem, refetchInterval: 10_000 });
}

export function useJobs(): UseQueryResult<Job[]> {
  return useQuery({ queryKey: keys.jobs, queryFn: api.listJobs });
}

/** The job currently working on an asset, if any. */
export function useActiveJob(targetId: string | null | undefined): Job | undefined {
  const jobs = useJobs();
  return jobs.data?.find((job) => job.targetId === targetId && (job.state === 'running' || job.state === 'queued'));
}

export function useAllBackups(): UseQueryResult<Backup[]> {
  return useQuery({ queryKey: keys.backups, queryFn: api.listAllBackups, refetchInterval: 30_000 });
}

/* -------------------------------- worlds ------------------------------- */

export function useWorlds(): UseQueryResult<World[]> {
  return useQuery({ queryKey: keys.worlds, queryFn: api.listWorlds, refetchInterval: 30_000 });
}

export function useWorld(worldId: string): UseQueryResult<World> {
  return useQuery({ queryKey: keys.world(worldId), queryFn: () => api.getWorld(worldId), enabled: !!worldId });
}

export function useWorldStatus(worldId: string): UseQueryResult<WorldStatus> {
  return useQuery({
    queryKey: keys.worldStatus(worldId),
    queryFn: () => api.getWorldStatus(worldId),
    enabled: !!worldId,
    refetchInterval: 10_000,
  });
}

export function useCreateWorld(): UseMutationResult<World, Error, CreateWorldRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.createWorld,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.worlds }),
  });
}

export function useUpdateWorld(worldId: string): UseMutationResult<World, Error, UpdateWorldRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateWorldRequest) => api.updateWorld(worldId, body),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.world(worldId), data);
      void queryClient.invalidateQueries({ queryKey: keys.worlds });
      // An instance renders its world name, so refresh those too.
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useDuplicateWorld(): UseMutationResult<World, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (worldId: string) => api.duplicateWorld(worldId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.worlds }),
  });
}

export function useDeleteWorld(): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (worldId: string) => api.deleteWorld(worldId),
    onSuccess: (_data, worldId) => {
      queryClient.removeQueries({ queryKey: keys.world(worldId) });
      void queryClient.invalidateQueries({ queryKey: keys.worlds });
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

/* ------------------------------- profiles ------------------------------ */

export function useProfiles(): UseQueryResult<Profile[]> {
  return useQuery({ queryKey: keys.profiles, queryFn: api.listProfiles, refetchInterval: 30_000 });
}

export function useProfile(profileId: string): UseQueryResult<Profile> {
  return useQuery({ queryKey: keys.profile(profileId), queryFn: () => api.getProfile(profileId), enabled: !!profileId });
}

export function useSoftware(): UseQueryResult<SoftwareCatalog> {
  return useQuery({ queryKey: keys.software, queryFn: api.getSoftware, staleTime: 5 * 60_000 });
}

export function useCreateProfile(): UseMutationResult<Profile, Error, CreateProfileRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.createProfile,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.profiles }),
  });
}

export function useUpdateProfile(profileId: string): UseMutationResult<Profile, Error, UpdateProfileRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateProfileRequest) => api.updateProfile(profileId, body),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.profile(profileId), data);
      void queryClient.invalidateQueries({ queryKey: keys.profiles });
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useDuplicateProfile(): UseMutationResult<Profile, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (profileId: string) => api.duplicateProfile(profileId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.profiles }),
  });
}

export function useDeleteProfile(): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (profileId: string) => api.deleteProfile(profileId),
    onSuccess: (_data, profileId) => {
      queryClient.removeQueries({ queryKey: keys.profile(profileId) });
      void queryClient.invalidateQueries({ queryKey: keys.profiles });
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useProfileProperties(profileId: string): UseQueryResult<PropertyFile> {
  return useQuery({
    queryKey: keys.profileProperties(profileId),
    queryFn: () => api.getProfileProperties(profileId),
    enabled: !!profileId,
  });
}

export function useSaveProfileProperties(
  profileId: string,
): UseMutationResult<PropertyFile, Error, Record<string, string>> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (changes: Record<string, string>) => api.saveProfileProperties(profileId, changes),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.profileProperties(profileId), data);
      // Whitelist flags and max players show on instances.
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

export function useProfileMods(profileId: string): UseQueryResult<ModIndex> {
  return useQuery({
    queryKey: keys.profileMods(profileId),
    queryFn: () => api.getProfileMods(profileId),
    enabled: !!profileId,
  });
}

export function useInstallMod(profileId: string): UseMutationResult<ModIndex, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (modId: string) => api.installMod(profileId, modId),
    onSuccess: (data) => queryClient.setQueryData(keys.profileMods(profileId), data),
  });
}

export function useUninstallMod(profileId: string): UseMutationResult<ModIndex, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (modId: string) => api.uninstallMod(profileId, modId),
    onSuccess: (data) => queryClient.setQueryData(keys.profileMods(profileId), data),
  });
}

export function useToggleMod(profileId: string): UseMutationResult<ModIndex, Error, { modId: string; enabled: boolean }> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ modId, enabled }: { modId: string; enabled: boolean }) => api.patchMod(profileId, modId, enabled),
    onSuccess: (data) => queryClient.setQueryData(keys.profileMods(profileId), data),
  });
}

export function useUpdateMod(profileId: string): UseMutationResult<ModIndex, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (modId: string) => api.updateMod(profileId, modId),
    onSuccess: (data) => queryClient.setQueryData(keys.profileMods(profileId), data),
  });
}

/* ------------------------------ instances ------------------------------ */

export function useInstances(): UseQueryResult<Instance[]> {
  return useQuery({ queryKey: keys.instances, queryFn: api.listInstances, refetchInterval: 15_000 });
}

export function useInstance(instanceId: string): UseQueryResult<Instance> {
  return useQuery({
    queryKey: keys.instance(instanceId),
    queryFn: () => api.getInstance(instanceId),
    enabled: !!instanceId,
  });
}

export function useInstanceStatus(instanceId: string): UseQueryResult<InstanceStatus> {
  return useQuery({
    queryKey: keys.instanceStatus(instanceId),
    queryFn: () => api.getInstanceStatus(instanceId),
    enabled: !!instanceId,
    refetchInterval: (query) => (query.state.data?.state === 'running' ? 3000 : 1500),
  });
}

export function useCreateInstance(): UseMutationResult<Instance, Error, CreateInstanceRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.createInstance,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.instances });
      void queryClient.invalidateQueries({ queryKey: keys.worlds });
      void queryClient.invalidateQueries({ queryKey: keys.profiles });
    },
  });
}

export function useUpdateInstance(instanceId: string): UseMutationResult<Instance, Error, UpdateInstanceRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateInstanceRequest) => api.updateInstance(instanceId, body),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.instance(instanceId), data);
      void queryClient.invalidateQueries({ queryKey: keys.instances });
      void queryClient.invalidateQueries({ queryKey: keys.instanceStatus(instanceId) });
    },
  });
}

export function useDeleteInstance(): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (instanceId: string) => api.deleteInstance(instanceId),
    onSuccess: (_data, instanceId) => {
      queryClient.removeQueries({ queryKey: keys.instance(instanceId) });
      void queryClient.invalidateQueries({ queryKey: keys.instances });
      // Detaching frees the world, so its attachment badge changes.
      void queryClient.invalidateQueries({ queryKey: keys.worlds });
    },
  });
}

/** Rebinds an instance to another world. Invalidates both sides of the attachment. */
export function useSwitchWorld(instanceId: string): UseMutationResult<Instance, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (worldId: string) => api.switchInstanceWorld(instanceId, worldId),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.instance(instanceId), data);
      void queryClient.invalidateQueries({ queryKey: keys.instances });
      void queryClient.invalidateQueries({ queryKey: keys.worlds });
      void queryClient.invalidateQueries({ queryKey: keys.instanceStatus(instanceId) });
    },
  });
}

export function useInstanceAction(instanceId: string): UseMutationResult<Instance, Error, ServerAction> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (action: ServerAction) => api.runAction(instanceId, action),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.instanceStatus(instanceId) });
      void queryClient.invalidateQueries({ queryKey: keys.instances });
    },
  });
}

/* ------------------------------- console ------------------------------- */

export function useConsoleTail(instanceId: string): UseQueryResult<Awaited<ReturnType<typeof api.getConsole>>> {
  return useQuery({
    queryKey: keys.console(instanceId),
    queryFn: () => api.getConsole(instanceId, 500),
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });
}

export function useSendCommand(
  instanceId: string,
): UseMutationResult<Awaited<ReturnType<typeof api.sendCommand>>, Error, string> {
  return useMutation({ mutationFn: (command: string) => api.sendCommand(instanceId, command) });
}

/* ------------------------------- players ------------------------------- */

export function usePlayers(instanceId: string): UseQueryResult<ServerPlayers> {
  return useQuery({
    queryKey: keys.instancePlayers(instanceId),
    queryFn: () => api.getPlayers(instanceId),
    enabled: !!instanceId,
    refetchInterval: 10_000,
  });
}

export function usePlayerAction(
  instanceId: string,
): UseMutationResult<ServerPlayers, Error, PlayerAction> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: PlayerAction) => api.postPlayerAction(instanceId, body),
    onSuccess: (data) => {
      queryClient.setQueryData(keys.instancePlayers(instanceId), data);
      void queryClient.invalidateQueries({ queryKey: keys.instanceStatus(instanceId) });
    },
  });
}

/* ------------------------------- backups ------------------------------- */

export function useBackups(worldId: string): UseQueryResult<BackupIndex> {
  return useQuery({
    queryKey: keys.worldBackups(worldId),
    queryFn: () => api.getBackups(worldId),
    enabled: !!worldId,
    refetchInterval: 5000,
  });
}

export function useCreateBackup(worldId: string): UseMutationResult<Backup, Error, void> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => api.createBackup(worldId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.worldBackups(worldId) }),
  });
}

export function useDeleteBackup(worldId: string): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (backupId: string) => api.deleteBackup(worldId, backupId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keys.worldBackups(worldId) }),
  });
}

export function useRestoreBackup(worldId: string): UseMutationResult<void, Error, string> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (backupId: string) => api.restoreBackup(worldId, backupId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: keys.worldBackups(worldId) });
      void queryClient.invalidateQueries({ queryKey: keys.worldStatus(worldId) });
    },
  });
}

export function usePatchSchedule(
  worldId: string,
): UseMutationResult<BackupIndex, Error, UpdateScheduleRequest> {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: UpdateScheduleRequest) => api.patchSchedule(worldId, body),
    onSuccess: (data) => queryClient.setQueryData(keys.worldBackups(worldId), data),
  });
}
