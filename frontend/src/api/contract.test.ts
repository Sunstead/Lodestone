/**
 * Contract parity: the same scenarios run against the in-memory demo
 * backend and, when `LODESTONE_CONTRACT_API` points at one, a real
 * Lodestone backend (usually `--runtime fake --providers fake`).
 *
 * If the demo and the backend disagree about a shape or a rule, this
 * fails, so demo mode can never drift into showing something the real
 * product does not do.
 *
 *   npm test                                      # demo only
 *   just test-contract                            # demo + live backend
 */

import { beforeAll, describe, expect, it, vi } from 'vitest';
import type * as Client from './client';
import type { CreateProfileRequest, CreateWorldRequest, Instance, ServerState } from './types';

const LIVE_API = process.env.LODESTONE_CONTRACT_API;

const targets: { name: string; mode: 'demo' | 'live'; base?: string }[] = [
  { name: 'demo backend', mode: 'demo' },
  ...(LIVE_API ? [{ name: `live backend (${LIVE_API})`, mode: 'live' as const, base: LIVE_API }] : []),
];

const unique = () => Math.random().toString(36).slice(2, 8);

const worldSpec = (name: string): CreateWorldRequest => ({
  name,
  levelType: 'minecraft:normal',
  seed: null,
  generateStructures: true,
  difficulty: 'normal',
  gamemode: 'survival',
  hardcore: false,
  maxWorldSize: 29_999_984,
  allowNether: true,
});

const profileSpec = (name: string): CreateProfileRequest => ({
  name,
  description: '',
  softwareId: 'paper',
  version: '1.21.4',
  memory: { minMb: 1024, maxMb: 2048, initialMb: 1024 },
  cpuCores: null,
  javaArgs: '',
  autoStart: false,
  restartOnCrash: false,
  crashThreshold: 3,
});

async function until<T>(what: string, read: () => Promise<T>, done: (value: T) => boolean, timeoutMs = 20_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  let last: T | undefined;
  while (Date.now() < deadline) {
    last = await read();
    if (done(last)) return last;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`timed out waiting for ${what}: ${JSON.stringify(last)}`);
}

describe.each(targets)('contract: $name', ({ mode, base }) => {
  let api: typeof Client;

  beforeAll(async () => {
    vi.resetModules();
    if (base) vi.stubEnv('VITE_API_BASE', base);
    api = await import('./client');
    api.forceMode(mode);
    if (mode === 'live') {
      // The dev user accepts the EULA for their own servers.
      await api.updateMe({ eulaAccepted: true });
    }
  });

  const waitState = (id: string, state: ServerState) =>
    until(`instance ${state}`, () => api.getInstance(id), (instance: Instance) => instance.state === state);

  it('serves a software catalog with Java versions', async () => {
    const catalog = await api.getSoftware();
    const paper = catalog.software.find((option) => option.id === 'paper');
    expect(paper?.kind).toBe('plugins');
    expect(paper?.versions[0]).toMatchObject({ version: expect.any(String), javaMajor: expect.any(Number), stable: true });
  });

  it('creates an instance with a new world and configuration in one step', async () => {
    const tag = unique();
    const instance = await api.createInstance({
      name: `contract-${tag}`,
      newWorld: worldSpec(`Contract World ${tag}`),
      newProfile: profileSpec(`Contract Profile ${tag}`),
    });
    expect(instance).toMatchObject({
      name: `contract-${tag}`,
      state: 'stopped',
      lastError: null,
      profile: { software: 'Paper', version: '1.21.4' },
      world: { name: `Contract World ${tag}` },
    });
    expect(instance.ports.minecraft).toBeGreaterThanOrEqual(25565);

    const world = await api.getWorld(instance.worldId);
    expect(world.attachedInstanceId).toBe(instance.id);
    expect(world.generated).toBe(false);
    const profile = await api.getProfile(instance.profileId);
    expect(profile.instanceCount).toBe(1);
    expect(profile.image).toMatch(/^eclipse-temurin:\d+-jre$/);

    // A world belongs to one instance at a time.
    await expect(
      api.createInstance({ name: `dup-${tag}`, worldId: instance.worldId, profileId: instance.profileId }),
    ).rejects.toThrow(/already attached/);

    // Deleting the instance keeps both assets.
    await api.deleteInstance(instance.id);
    expect((await api.getWorld(instance.worldId)).attachedInstanceId).toBeNull();
    expect((await api.getProfile(instance.profileId)).instanceCount).toBe(0);
  });

  it('rejects asking for both an existing and a new world', async () => {
    const world = await api.createWorld(worldSpec(`Either ${unique()}`));
    await expect(
      api.createInstance({ name: 'x', worldId: world.id, newWorld: worldSpec('Or'), newProfile: profileSpec('p') }),
    ).rejects.toThrow(/not both/);
  });

  it('keeps server.properties keys on their owner', async () => {
    const profile = await api.createProfile(profileSpec(`Props ${unique()}`));
    const file = await api.getProfileProperties(profile.id);
    expect(file.fields.every((field) => field.scope === 'profile')).toBe(true);

    const saved = await api.saveProfileProperties(profile.id, { motd: 'Hello from the contract' });
    expect(saved.fields.find((field) => field.key === 'motd')?.value).toBe('Hello from the contract');

    await expect(api.saveProfileProperties(profile.id, { difficulty: 'hard' })).rejects.toThrow(/belongs to the world/);
    await expect(api.saveProfileProperties(profile.id, { 'rcon.port': '1' })).rejects.toThrow(/managed by Lodestone/);
  });

  it('updates world gameplay settings', async () => {
    const world = await api.createWorld(worldSpec(`Rules ${unique()}`));
    const updated = await api.updateWorld(world.id, { difficulty: 'hard', pvp: false });
    expect(updated).toMatchObject({ difficulty: 'hard', pvp: false });
  });

  it('starts, runs commands and stops a server', async () => {
    const tag = unique();
    const instance = await api.createInstance({
      name: `run-${tag}`,
      newWorld: worldSpec(`Run ${tag}`),
      newProfile: profileSpec(`Run ${tag}`),
    });
    await api.runAction(instance.id, 'start');
    await waitState(instance.id, 'running');

    const lines = await api.sendCommand(instance.id, 'list');
    expect(lines.length).toBeGreaterThan(0);
    expect(lines.some((line) => /players online/i.test(line.message))).toBe(true);

    const status = await api.getInstanceStatus(instance.id);
    expect(status.state).toBe('running');

    expect((await api.getWorld(instance.worldId)).generated).toBe(true);

    await api.runAction(instance.id, 'stop');
    await waitState(instance.id, 'stopped');
  });

  it('backs up a world', async () => {
    const world = await api.createWorld(worldSpec(`Backup ${unique()}`));
    const backup = await api.createBackup(world.id);
    expect(backup.worldId).toBe(world.id);
    const index = await until(
      'backup to complete',
      () => api.getBackups(world.id),
      (value) => value.backups.some((entry) => entry.id === backup.id && entry.status === 'complete'),
    );
    expect(index.schedule).toMatchObject({ enabled: expect.any(Boolean), intervalMinutes: expect.any(Number) });
    expect((await api.listAllBackups()).some((entry) => entry.id === backup.id)).toBe(true);
  });

  it('reads and writes app settings', async () => {
    const settings = await api.updateSettings({ publicAddress: 'play.example.com' });
    expect(settings.publicAddress).toBe('play.example.com');
    expect((await api.getMe()).eulaAccepted).toBe(true);
    const system = await api.getSystem();
    expect(system.runtime.available).toBe(true);
    await api.updateSettings({ publicAddress: '' });
  });

  it('says who is signed in', async () => {
    const me = await api.getMe();
    expect(me.user).toMatchObject({ id: expect.any(String), username: expect.any(String), admin: true });
    expect(me.auth.mode).toBe('dev');
    const users = await api.listUsers();
    expect(users.some((row) => row.user.id === me.user.id)).toBe(true);
  });

  it('searches, plans and installs plugins with their dependencies', async () => {
    const profile = await api.createProfile(profileSpec(`Plugins ${unique()}`));
    const empty = await api.getContent(profile.id);
    expect(empty).toMatchObject({ kind: 'plugins', loaders: ['paper', 'spigot', 'bukkit'], gameVersion: '1.21.4', entries: [] });

    const results = await api.searchContent(profile.id, { q: 'guard' });
    expect(results.hits[0]).toMatchObject({ name: 'WorldGuard', kind: 'plugin', installed: false });
    expect((await api.searchContent(profile.id, { q: 'lithium' })).total).toBe(0);

    const versions = await api.getContentVersions(profile.id, 'modrinth', results.hits[0].id);
    expect(versions.every((version) => version.compatible)).toBe(true);

    const plan = await api.planContent(profile.id, { items: [{ provider: 'modrinth', projectId: results.hits[0].id }] });
    expect(plan.install.map((item) => [item.name, item.requiredBy])).toEqual([
      ['WorldGuard', null],
      ['WorldEdit', 'WorldGuard'],
    ]);
    expect(plan.conflicts).toEqual([]);

    const index = await api.applyContent(
      profile.id,
      plan.install.map((item) => ({ provider: item.provider, projectId: item.projectId, versionId: item.versionId })),
    );
    const worldedit = index.entries.find((entry) => entry.name === 'WorldEdit');
    expect(worldedit).toMatchObject({ dependency: true, requiredBy: ['WorldGuard'], compatible: true, enabled: true });

    const conflict = await api.planContent(profile.id, { items: [{ provider: 'modrinth', projectId: 'fk-coreprotect' }] });
    expect(conflict.conflicts[0].message).toBe('CoreProtect has no version for Paper 1.21.4.');
  });

  it('checks for updates, updates, toggles and removes', async () => {
    const profile = await api.createProfile(profileSpec(`Updates ${unique()}`));
    let index = await api.applyContent(profile.id, [
      { provider: 'modrinth', projectId: 'fk-worldedit', versionId: 'fk-worldedit-7.3.9' },
    ]);
    const entry = index.entries[0].id;
    index = await api.checkContentUpdates(profile.id);
    expect(index.entries[0].update).toMatchObject({ versionName: '7.3.10' });
    expect(index.checkedAt).toEqual(expect.any(String));

    index = await api.updateContent(profile.id, entry);
    expect(index.entries[0]).toMatchObject({ id: entry, versionName: '7.3.10', update: null });

    index = await api.setContentEnabled(profile.id, entry, false);
    expect(index.entries[0].enabled).toBe(false);
    index = await api.removeContent(profile.id, entry);
    expect(index.entries).toEqual([]);
  });

  it('offers Hangar for Paper, and CurseForge once it has a key', async () => {
    const paper = await api.createProfile(profileSpec(`Sources ${unique()}`));
    const sources = (await api.getContent(paper.id)).providers;
    expect(sources.map((p) => [p.id, p.available])).toEqual([
      ['modrinth', true],
      ['hangar', true],
      ['curseforge', false],
    ]);
    expect(sources[2].reason).toContain('API key');
    const fabric = await api.createProfile({ ...profileSpec(`Fabric ${unique()}`), softwareId: 'fabric' });
    expect((await api.getContent(fabric.id)).providers.find((p) => p.id === 'hangar')?.available).toBe(false);

    const hangar = await api.searchContent(paper.id, { q: 'guard', provider: 'hangar' });
    expect(hangar.hits[0]).toMatchObject({ provider: 'hangar', name: 'WorldGuard' });
    await expect(api.searchContent(paper.id, { q: 'x', provider: 'curseforge' })).rejects.toThrow(/API key/);

    const settings = await api.updateSettings({ curseforgeApiKey: 'contract-key' });
    expect(settings.curseforgeKeySet).toBe(true);
    expect(JSON.stringify(settings)).not.toContain('contract-key');
    expect((await api.getContent(paper.id)).providers.find((p) => p.id === 'curseforge')?.available).toBe(true);
    expect((await api.updateSettings({ curseforgeApiKey: '' })).curseforgeKeySet).toBe(false);
  });

  it('records restricted files as manual downloads and checks the upload', async () => {
    await api.updateSettings({ curseforgeApiKey: 'contract-key' });
    try {
      const profile = await api.createProfile(profileSpec(`Manual ${unique()}`));
      const plan = await api.planContent(profile.id, { items: [{ provider: 'curseforge', projectId: 'fk-essentialsx' }] });
      const item = plan.install[0];
      expect(item.manualDownload).toEqual(expect.any(String));
      let index = await api.applyContent(profile.id, [
        { provider: item.provider, projectId: item.projectId, versionId: item.versionId },
      ]);
      const entry = index.entries[0];
      expect(entry).toMatchObject({ provider: 'curseforge', manualDownload: expect.any(String) });

      const wrong = new File(['not it'], 'EssentialsX.jar');
      await expect(api.provideContentFile(profile.id, entry.id, wrong)).rejects.toThrow(/checksum/);
      const right = new File([`lodestone fake content: ${item.versionId}\n`], 'EssentialsX.jar');
      index = await api.provideContentFile(profile.id, entry.id, right);
      expect(index.entries[0].manualDownload).toBeNull();
    } finally {
      await api.updateSettings({ curseforgeApiKey: '' });
    }
  });
});
