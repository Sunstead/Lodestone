/**
 * Mods and plugins for the demo backend. Same catalog as the backend's fake
 * provider (`shared/fake-content.json`) and the same rules: search filtered
 * by loader and version, install plans with dependencies, update checks.
 * `contract.test.ts` holds both to the same behaviour.
 */

import fakeContent from '../../../shared/fake-content.json';
import type {
  ContentDependency,
  ContentEntry,
  ContentIndex,
  ContentKind,
  ContentProject,
  ContentRef,
  ContentSearchResult,
  ContentSource,
  ContentVersion,
  InstallPlan,
  ModKind,
  PlanConflict,
  PlanItem,
  PlanSuggestion,
  Profile,
  ServerSoftwareId,
  VersionChannel,
} from './types';

type FakeVersion = {
  id: string;
  versionNumber: string;
  channel: string;
  publishedAt: string;
  gameVersions: string[];
  loaders: string[];
  dependencies: { projectId: string | null; versionId: string | null; kind: string }[];
};

type FakeProject = {
  id: string;
  slug: string;
  name: string;
  author: string;
  description: string;
  categories: string[];
  downloads: number;
  versions: FakeVersion[];
};

const PROJECTS = (fakeContent as { projects: FakeProject[] }).projects;

type Target = { software: ServerSoftwareId; kind: ModKind; loaders: string[]; gameVersion: string };

export const LOADERS: Record<ServerSoftwareId, string[]> = {
  vanilla: [],
  paper: ['paper', 'spigot', 'bukkit'],
  fabric: ['fabric'],
  forge: ['forge'],
  neoforge: ['neoforge'],
};

function contentKind(software: ServerSoftwareId): ContentKind {
  return software === 'vanilla' ? 'vanilla' : software === 'paper' ? 'plugins' : 'mods';
}

function targetOf(profile: Profile): Target {
  if (profile.softwareId === 'vanilla') {
    throw new Error(
      "Vanilla servers can't load mods or plugins. Switch this configuration to Paper for plugins, or Fabric or NeoForge for mods.",
    );
  }
  return {
    software: profile.softwareId,
    kind: profile.softwareId === 'paper' ? 'plugin' : 'mod',
    loaders: LOADERS[profile.softwareId],
    gameVersion: profile.version,
  };
}

const accepts = (target: Target, loaders: string[], gameVersions: string[]): boolean =>
  loaders.some((loader) => target.loaders.includes(loader)) && gameVersions.includes(target.gameVersion);

const describe = (target: Target): string =>
  `${{ vanilla: 'Vanilla', paper: 'Paper', fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge' }[target.software]} ${target.gameVersion}`;

function findProject(id: string): FakeProject {
  const project = PROJECTS.find((p) => p.id === id || p.slug === id);
  if (!project) throw new Error('Modrinth has no such project or version.');
  return project;
}

function toVersion(project: FakeProject, version: FakeVersion, target: Target): ContentVersion {
  // Plausible jar sizes (60 KB to 6 MB), stable per version.
  let hash = 0;
  for (const ch of version.id) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  const bytes = 60_000 + (hash % 6_000_000);
  return {
    provider: 'modrinth',
    id: version.id,
    projectId: project.id,
    name: `${project.name} ${version.versionNumber}`,
    versionNumber: version.versionNumber,
    channel: version.channel as VersionChannel,
    publishedAt: version.publishedAt,
    downloads: Math.floor(project.downloads / 10),
    gameVersions: version.gameVersions,
    loaders: version.loaders,
    filename: `${project.slug}-${version.versionNumber.replace('+', '-')}.jar`,
    sizeBytes: bytes,
    dependencies: version.dependencies as ContentDependency[],
    compatible: accepts(target, version.loaders, version.gameVersions),
  };
}

function findVersion(versionId: string, target: Target): { project: FakeProject; version: ContentVersion } {
  for (const project of PROJECTS) {
    const version = project.versions.find((v) => v.id === versionId);
    if (version) return { project, version: toVersion(project, version, target) };
  }
  throw new Error('Modrinth has no such project or version.');
}

const compatibleVersions = (project: FakeProject, target: Target): ContentVersion[] =>
  project.versions.map((v) => toVersion(project, v, target)).filter((v) => v.compatible);

const pick = (versions: ContentVersion[]): ContentVersion | undefined =>
  versions.find((v) => v.channel === 'release') ?? versions[0];

/* ------------------------------ lockfiles ------------------------------ */

type Lock = {
  entries: ContentEntry[];
  /** Dependencies each entry declared, for `requiredBy`. */
  deps: Map<string, ContentDependency[]>;
  /** Loader/versions each entry was built for, for `compatible`. */
  builtFor: Map<string, { loaders: string[]; gameVersions: string[] }>;
  checkedAt: string | null;
};

const locks = new Map<string, Lock>();
let counter = 0;

function lock(profileId: string): Lock {
  let found = locks.get(profileId);
  if (!found) {
    found = { entries: [], deps: new Map(), builtFor: new Map(), checkedAt: null };
    locks.set(profileId, found);
  }
  return found;
}

export function forget(profileId: string): void {
  locks.delete(profileId);
}

export function copyContent(from: string, to: string): void {
  const source = lock(from);
  const copy = lock(to);
  for (const entry of source.entries) {
    const id = `content-demo-${++counter}`;
    copy.entries.push({ ...entry, id });
    copy.deps.set(id, source.deps.get(entry.id) ?? []);
    const built = source.builtFor.get(entry.id);
    if (built) copy.builtFor.set(id, built);
  }
}

function requires(l: Lock, entry: ContentEntry, projectId: string): boolean {
  return (l.deps.get(entry.id) ?? []).some((d) => d.kind === 'required' && d.projectId === projectId);
}

export function index(profile: Profile): ContentIndex {
  const l = lock(profile.id);
  const target = profile.softwareId === 'vanilla' ? null : targetOf(profile);
  const entries = l.entries.map((entry) => {
    const built = l.builtFor.get(entry.id);
    return {
      ...entry,
      requiredBy: entry.projectId
        ? l.entries.filter((o) => o.id !== entry.id && o.enabled && requires(l, o, entry.projectId!)).map((o) => o.name)
        : [],
      compatible:
        !!target &&
        entry.kind === target.kind &&
        (entry.provider === 'upload' || (!!built && accepts(target, built.loaders, built.gameVersions))),
    };
  });
  return {
    kind: contentKind(profile.softwareId),
    loaders: LOADERS[profile.softwareId],
    gameVersion: profile.version,
    entries,
    checkedAt: l.checkedAt,
    providers: target ? [{ id: 'modrinth', label: 'Modrinth', available: true, reason: null }] : [],
  };
}

export function search(
  profile: Profile,
  params: { q: string; sort?: string; offset?: number; limit?: number },
): ContentSearchResult {
  const target = targetOf(profile);
  const needle = params.q.trim().toLowerCase();
  const installed = new Set(lock(profile.id).entries.map((e) => e.projectId));
  let hits = PROJECTS.filter((p) => p.versions.some((v) => accepts(target, v.loaders, v.gameVersions))).filter(
    (p) =>
      !needle ||
      p.name.toLowerCase().includes(needle) ||
      p.slug.includes(needle) ||
      p.description.toLowerCase().includes(needle),
  );
  hits =
    params.sort === 'updated' || params.sort === 'newest'
      ? hits.sort((a, b) => b.versions[0].publishedAt.localeCompare(a.versions[0].publishedAt))
      : hits.sort((a, b) => b.downloads - a.downloads);
  const offset = params.offset ?? 0;
  const limit = Math.min(Math.max(params.limit ?? 20, 1), 50);
  return {
    hits: hits.slice(offset, offset + limit).map(
      (p): ContentProject => ({
        provider: 'modrinth',
        id: p.id,
        slug: p.slug,
        name: p.name,
        author: p.author,
        description: p.description,
        iconUrl: null,
        pageUrl: `https://modrinth.com/project/${p.slug}`,
        downloads: p.downloads,
        categories: p.categories,
        kind: target.kind,
        installed: installed.has(p.id),
      }),
    ),
    total: hits.length,
    offset,
    limit,
  };
}

export function versions(profile: Profile, provider: ContentSource, projectId: string): ContentVersion[] {
  if (provider !== 'modrinth') throw new Error(`${provider} isn't set up on this Lodestone.`);
  return compatibleVersions(findProject(projectId), targetOf(profile));
}

export function plan(profile: Profile, requests: ContentRef[]): InstallPlan {
  if (requests.length === 0) throw new Error('Pick something to install.');
  const target = targetOf(profile);
  const installed = lock(profile.id);
  const out: InstallPlan = { install: [], optional: [], conflicts: [] };
  const seen = new Set<string>();
  const queue: { ref: ContentRef; requiredBy: string | null }[] = requests.map((ref) => ({ ref, requiredBy: null }));
  const optional: { projectId: string; by: string }[] = [];
  const installedProject = (id: string) => installed.entries.find((e) => e.projectId === id);

  while (queue.length) {
    const { ref, requiredBy } = queue.shift()!;
    if (seen.has(ref.projectId)) continue;
    seen.add(ref.projectId);
    const existing = installedProject(ref.projectId);
    if (requiredBy && existing) continue;
    const project = findProject(ref.projectId);
    let chosen: ContentVersion | undefined;
    if (ref.versionId) {
      const { version } = findVersion(ref.versionId, target);
      if (version.compatible) chosen = version;
      else if (requiredBy) chosen = pick(compatibleVersions(project, target));
      else {
        out.conflicts.push({
          name: project.name,
          message: `${project.name} ${version.versionNumber} isn't built for ${describe(target)}.`,
        });
        continue;
      }
    } else {
      chosen = pick(compatibleVersions(project, target));
    }
    if (!chosen) {
      out.conflicts.push({
        name: project.name,
        message: requiredBy
          ? `${requiredBy} needs ${project.name}, which has no version for ${describe(target)}.`
          : `${project.name} has no version for ${describe(target)}.`,
      });
      continue;
    }
    if (existing?.versionId === chosen.id) continue;

    for (const entry of installed.entries) {
      const refuses = (installed.deps.get(entry.id) ?? []).some(
        (d) => d.kind === 'incompatible' && d.projectId === project.id,
      );
      if (refuses && entry.enabled) {
        out.conflicts.push({ name: project.name, message: `${entry.name} doesn't work with ${project.name}, which is installed.` });
      }
    }
    for (const dep of chosen.dependencies) {
      if (!dep.projectId) continue;
      if (dep.kind === 'required') {
        queue.push({ ref: { provider: 'modrinth', projectId: dep.projectId, versionId: dep.versionId ?? undefined }, requiredBy: project.name });
      } else if (dep.kind === 'optional' && !installedProject(dep.projectId)) {
        optional.push({ projectId: dep.projectId, by: project.name });
      } else if (dep.kind === 'incompatible') {
        const clash = installedProject(dep.projectId);
        const other =
          clash && clash.enabled
            ? clash.name
            : requests.some((r) => r.projectId === dep.projectId)
              ? findProject(dep.projectId).name
              : null;
        if (other) out.conflicts.push({ name: project.name, message: `${project.name} doesn't work with ${other}.` });
      }
    }
    const item: PlanItem = {
      provider: 'modrinth',
      projectId: project.id,
      versionId: chosen.id,
      name: project.name,
      versionNumber: chosen.versionNumber,
      filename: chosen.filename,
      sizeBytes: chosen.sizeBytes,
      iconUrl: null,
      requiredBy,
      replaces: existing ? { entryId: existing.id, versionName: existing.versionName } : null,
    };
    out.install.push(item);
  }
  for (const { projectId, by } of optional) {
    if (out.install.some((i) => i.projectId === projectId) || out.optional.some((o) => o.projectId === projectId)) continue;
    const project = findProject(projectId);
    if (compatibleVersions(project, target).length === 0) continue;
    const suggestion: PlanSuggestion = { provider: 'modrinth', projectId, name: project.name, suggestedBy: by };
    out.optional.push(suggestion);
  }
  return out;
}

export function apply(profile: Profile, items: ContentRef[]): ContentIndex {
  if (items.length === 0) return index(profile);
  const target = targetOf(profile);
  const l = lock(profile.id);
  const resolved = items.map((item) => {
    if (!item.versionId) throw new Error('Pick a version for each item to install.');
    const { project, version } = findVersion(item.versionId, target);
    if (project.id !== item.projectId) throw new Error(`Version ${item.versionId} doesn't belong to that project.`);
    if (!version.compatible) throw new Error(`${version.name} isn't built for ${describe(target)}.`);
    return { project, version };
  });
  const now = new Date().toISOString();
  for (const { project, version } of resolved) {
    const needed =
      resolved.some(({ version: other }) =>
        other.dependencies.some((d) => d.kind === 'required' && d.projectId === project.id),
      ) || l.entries.some((e) => requires(l, e, project.id));
    const previous = l.entries.find((e) => e.projectId === project.id);
    const id = previous?.id ?? `content-demo-${++counter}`;
    const entry: ContentEntry = {
      id,
      provider: 'modrinth',
      projectId: project.id,
      versionId: version.id,
      name: project.name,
      versionName: version.versionNumber,
      filename: version.filename,
      sizeBytes: version.sizeBytes,
      kind: target.kind,
      author: project.author,
      description: project.description,
      iconUrl: null,
      pageUrl: `https://modrinth.com/project/${project.slug}`,
      enabled: previous?.enabled ?? true,
      dependency: previous?.dependency ?? needed,
      requiredBy: [],
      compatible: true,
      update: null,
      installedAt: previous?.installedAt ?? now,
    };
    if (previous) l.entries[l.entries.indexOf(previous)] = entry;
    else l.entries.push(entry);
    l.deps.set(id, version.dependencies);
    l.builtFor.set(id, { loaders: version.loaders, gameVersions: version.gameVersions });
  }
  return index(profile);
}

function entryOf(profile: Profile, entryId: string): ContentEntry {
  const entry = lock(profile.id).entries.find((e) => e.id === entryId);
  if (!entry) throw new Error(`No mod or plugin with id \`${entryId}\`.`);
  return entry;
}

export function setEnabled(profile: Profile, entryId: string, enabled: boolean): ContentIndex {
  entryOf(profile, entryId).enabled = enabled;
  return index(profile);
}

export function remove(profile: Profile, entryId: string): ContentIndex {
  entryOf(profile, entryId);
  const l = lock(profile.id);
  l.entries = l.entries.filter((e) => e.id !== entryId);
  return index(profile);
}

export function checkUpdates(profile: Profile): ContentIndex {
  const target = targetOf(profile);
  const l = lock(profile.id);
  for (const entry of l.entries) {
    if (entry.provider !== 'modrinth' || !entry.projectId) continue;
    const latest = pick(compatibleVersions(findProject(entry.projectId), target));
    entry.update =
      latest && latest.id !== entry.versionId ? { versionId: latest.id, versionName: latest.versionNumber } : null;
  }
  l.checkedAt = new Date().toISOString();
  return index(profile);
}

export function update(profile: Profile, entryId: string): ContentIndex {
  const entry = entryOf(profile, entryId);
  if (!entry.update || !entry.projectId) throw new Error(`${entry.name} is up to date.`);
  const result = plan(profile, [{ provider: 'modrinth', projectId: entry.projectId, versionId: entry.update.versionId }]);
  if (result.conflicts.length) throw new Error(result.conflicts[0].message);
  return apply(
    profile,
    result.install.map((i) => ({ provider: i.provider, projectId: i.projectId, versionId: i.versionId })),
  );
}

export function upload(profile: Profile, filename: string, size: number): ContentIndex {
  const target = targetOf(profile);
  if (!filename.toLowerCase().endsWith('.jar')) throw new Error('Upload a .jar file.');
  const l = lock(profile.id);
  if (l.entries.some((e) => e.filename === filename)) {
    throw new Error(`This configuration already has a file named ${filename}.`);
  }
  l.entries.push({
    id: `content-demo-${++counter}`,
    provider: 'upload',
    projectId: null,
    versionId: null,
    name: filename.replace(/\.jar$/i, ''),
    versionName: '',
    filename,
    sizeBytes: size,
    kind: target.kind,
    author: '',
    description: '',
    iconUrl: null,
    pageUrl: null,
    enabled: true,
    dependency: false,
    requiredBy: [],
    compatible: true,
    update: null,
    installedAt: new Date().toISOString(),
  });
  return index(profile);
}

/** Seeds a profile with exact versions, then checks for updates. */
export function seed(profile: Profile, versionIds: string[], disabled: string[] = []): void {
  const target = targetOf(profile);
  apply(
    profile,
    versionIds.map((versionId) => {
      const { project } = findVersion(versionId, target);
      return { provider: 'modrinth' as const, projectId: project.id, versionId };
    }),
  );
  for (const entry of lock(profile.id).entries) {
    if (entry.projectId && disabled.includes(entry.projectId)) entry.enabled = false;
  }
  checkUpdates(profile);
}

export type { PlanConflict };
