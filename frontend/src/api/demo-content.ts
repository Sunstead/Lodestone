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
  restrictedOn?: string[];
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

type Source = Exclude<ContentSource, 'upload'>;
const SOURCES: { id: Source; label: string; description: string }[] = [
  { id: 'modrinth', label: 'Modrinth', description: 'Mods and plugins' },
  { id: 'hangar', label: 'Hangar', description: 'Paper plugins' },
  { id: 'curseforge', label: 'CurseForge', description: 'Mods and Bukkit plugins' },
];
const label = (source: ContentSource): string => SOURCES.find((s) => s.id === source)?.label ?? source;

/** Mirrors the backend: CurseForge needs a key, which the demo's Settings can "set". */
let curseforgeKey = false;
export function setCurseforgeKey(set: boolean): void {
  curseforgeKey = set;
}

function unavailable(source: ContentSource, target: Target): string | null {
  if (source === 'hangar' && target.kind !== 'plugin') return 'Hangar only has Paper plugins.';
  if (source === 'curseforge' && !curseforgeKey) return 'CurseForge needs an API key. An admin can add one in Settings.';
  return null;
}

function usable(source: ContentSource, target: Target): Source {
  if (!SOURCES.some((s) => s.id === source)) throw new Error(`${source} isn't set up on this Lodestone.`);
  const reason = unavailable(source, target);
  if (reason) throw new Error(reason);
  return source as Source;
}

function pageFor(source: ContentSource, project: FakeProject): string {
  if (source === 'hangar') return `https://hangar.papermc.io/${project.author.replace(/ /g, '')}/${project.slug}`;
  if (source === 'curseforge') return `https://www.curseforge.com/minecraft/mc-mods/${project.slug}`;
  return `https://modrinth.com/project/${project.slug}`;
}

/** The placeholder "jar" for a version, as the backend's fake provider makes it. */
export const jarText = (versionId: string): string => `lodestone fake content: ${versionId}\n`;

/**
 * Where to fetch a file Lodestone may not download. The demo hands over the
 * placeholder jar itself, so the manual-download flow can be tried end to end.
 */
const manualUrl = (versionId: string): string =>
  `data:application/java-archive;base64,${typeof btoa === 'function' ? btoa(jarText(versionId)) : Buffer.from(jarText(versionId)).toString('base64')}`;

const accepts = (target: Target, loaders: string[], gameVersions: string[]): boolean =>
  loaders.some((loader) => target.loaders.includes(loader)) && gameVersions.includes(target.gameVersion);

const describe = (target: Target): string =>
  `${{ vanilla: 'Vanilla', paper: 'Paper', fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge' }[target.software]} ${target.gameVersion}`;

function findProject(id: string, source: ContentSource = 'modrinth'): FakeProject {
  const project = PROJECTS.find((p) => p.id === id || p.slug === id);
  if (!project) throw new Error(`${label(source)} has no such project or version.`);
  return project;
}

const restricted = (project: FakeProject, source: ContentSource): boolean =>
  (project.restrictedOn ?? []).includes(source);

function toVersion(project: FakeProject, version: FakeVersion, target: Target, source: Source = 'modrinth'): ContentVersion {
  // Plausible jar sizes (60 KB to 6 MB), stable per version.
  let hash = 0;
  for (const ch of version.id) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  const bytes = 60_000 + (hash % 6_000_000);
  return {
    provider: source,
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

function findVersion(
  versionId: string,
  target: Target,
  source: Source = 'modrinth',
): { project: FakeProject; version: ContentVersion } {
  for (const project of PROJECTS) {
    const version = project.versions.find((v) => v.id === versionId);
    if (version) return { project, version: toVersion(project, version, target, source) };
  }
  throw new Error(`${label(source)} has no such project or version.`);
}

const compatibleVersions = (project: FakeProject, target: Target, source: Source = 'modrinth'): ContentVersion[] =>
  project.versions.map((v) => toVersion(project, v, target, source)).filter((v) => v.compatible);

const pick = (versions: ContentVersion[]): ContentVersion | undefined =>
  versions.find((v) => v.channel === 'release') ?? versions[0];

/* ------------------------------ lockfiles ------------------------------ */

type Lock = {
  entries: ContentEntry[];
  /** Dependencies each entry declared, for `requiredBy`. */
  deps: Map<string, ContentDependency[]>;
  /** Loader/versions each entry was built for, for `compatible`. */
  builtFor: Map<string, { loaders: string[]; gameVersions: string[] }>;
  /** Entries waiting for a manual download: entry id -> the file's text. */
  waiting: Map<string, string>;
  checkedAt: string | null;
};

const locks = new Map<string, Lock>();
let counter = 0;

function lock(profileId: string): Lock {
  let found = locks.get(profileId);
  if (!found) {
    found = { entries: [], deps: new Map(), builtFor: new Map(), waiting: new Map(), checkedAt: null };
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
    const waiting = source.waiting.get(entry.id);
    if (waiting) copy.waiting.set(id, waiting);
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
      manualDownload: l.waiting.has(entry.id) && entry.versionId ? manualUrl(entry.versionId) : null,
    };
  });
  return {
    kind: contentKind(profile.softwareId),
    loaders: LOADERS[profile.softwareId],
    gameVersion: profile.version,
    entries,
    checkedAt: l.checkedAt,
    providers: target
      ? SOURCES.map((source) => {
          const reason = unavailable(source.id, target);
          return { ...source, available: !reason, reason };
        })
      : [],
  };
}

export function search(
  profile: Profile,
  params: { q: string; provider?: ContentSource; sort?: string; offset?: number; limit?: number },
): ContentSearchResult {
  const target = targetOf(profile);
  const source = usable(params.provider ?? 'modrinth', target);
  const needle = params.q.trim().toLowerCase();
  const installed = new Set(
    lock(profile.id)
      .entries.filter((e) => e.provider === source)
      .map((e) => e.projectId),
  );
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
        provider: source,
        id: p.id,
        slug: p.slug,
        name: p.name,
        author: p.author,
        description: p.description,
        iconUrl: null,
        pageUrl: pageFor(source, p),
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
  const target = targetOf(profile);
  const source = usable(provider, target);
  return compatibleVersions(findProject(projectId, source), target, source);
}

export function plan(profile: Profile, requests: ContentRef[]): InstallPlan {
  if (requests.length === 0) throw new Error('Pick something to install.');
  const target = targetOf(profile);
  const installed = lock(profile.id);
  const out: InstallPlan = { install: [], optional: [], conflicts: [] };
  const seen = new Set<string>();
  const queue: { ref: ContentRef; requiredBy: string | null }[] = requests.map((ref) => ({ ref, requiredBy: null }));
  const optional: { projectId: string; by: string }[] = [];
  const optionalSources = new Map<string, Source>();

  while (queue.length) {
    const { ref, requiredBy } = queue.shift()!;
    const source = usable(ref.provider, target);
    const installedProject = (id: string) =>
      installed.entries.find((e) => e.provider === source && e.projectId === id);
    if (seen.has(`${source}:${ref.projectId}`)) continue;
    seen.add(`${source}:${ref.projectId}`);
    const existing = installedProject(ref.projectId);
    if (requiredBy && existing) continue;
    const project = findProject(ref.projectId, source);
    let chosen: ContentVersion | undefined;
    if (ref.versionId) {
      const { version } = findVersion(ref.versionId, target, source);
      if (version.compatible) chosen = version;
      else if (requiredBy) chosen = pick(compatibleVersions(project, target, source));
      else {
        out.conflicts.push({
          name: project.name,
          message: `${project.name} ${version.versionNumber} isn't built for ${describe(target)}.`,
        });
        continue;
      }
    } else {
      chosen = pick(compatibleVersions(project, target, source));
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
        queue.push({ ref: { provider: source, projectId: dep.projectId, versionId: dep.versionId ?? undefined }, requiredBy: project.name });
      } else if (dep.kind === 'optional' && !installedProject(dep.projectId)) {
        optional.push({ projectId: dep.projectId, by: project.name });
        optionalSources.set(dep.projectId, source);
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
      provider: source,
      projectId: project.id,
      versionId: chosen.id,
      name: project.name,
      versionNumber: chosen.versionNumber,
      filename: chosen.filename,
      sizeBytes: chosen.sizeBytes,
      iconUrl: null,
      requiredBy,
      replaces: existing ? { entryId: existing.id, versionName: existing.versionName } : null,
      manualDownload: restricted(project, source) ? manualUrl(chosen.id) : null,
    };
    out.install.push(item);
  }
  for (const { projectId, by } of optional) {
    if (out.install.some((i) => i.projectId === projectId) || out.optional.some((o) => o.projectId === projectId)) continue;
    const source = optionalSources.get(projectId) ?? 'modrinth';
    const project = findProject(projectId, source);
    if (compatibleVersions(project, target, source).length === 0) continue;
    const suggestion: PlanSuggestion = { provider: source, projectId, name: project.name, suggestedBy: by };
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
    const source = usable(item.provider, target);
    const { project, version } = findVersion(item.versionId, target, source);
    if (project.id !== item.projectId) throw new Error(`Version ${item.versionId} doesn't belong to that project.`);
    if (!version.compatible) throw new Error(`${version.name} isn't built for ${describe(target)}.`);
    return { project, version, source };
  });
  const now = new Date().toISOString();
  for (const { project, version, source } of resolved) {
    const needed =
      resolved.some(({ version: other }) =>
        other.dependencies.some((d) => d.kind === 'required' && d.projectId === project.id),
      ) || l.entries.some((e) => requires(l, e, project.id));
    const previous = l.entries.find((e) => e.provider === source && e.projectId === project.id);
    const id = previous?.id ?? `content-demo-${++counter}`;
    const entry: ContentEntry = {
      id,
      provider: source,
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
      pageUrl: pageFor(source, project),
      enabled: previous?.enabled ?? true,
      dependency: previous?.dependency ?? needed,
      requiredBy: [],
      compatible: true,
      update: null,
      manualDownload: null,
      installedAt: previous?.installedAt ?? now,
    };
    if (restricted(project, source)) l.waiting.set(id, jarText(version.id));
    else l.waiting.delete(id);
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
    if (entry.provider === 'upload' || !entry.projectId || unavailable(entry.provider, target)) continue;
    const source = entry.provider as Source;
    const latest = pick(compatibleVersions(findProject(entry.projectId, source), target, source));
    entry.update =
      latest && latest.id !== entry.versionId ? { versionId: latest.id, versionName: latest.versionNumber } : null;
  }
  l.checkedAt = new Date().toISOString();
  return index(profile);
}

export function update(profile: Profile, entryId: string): ContentIndex {
  const entry = entryOf(profile, entryId);
  if (!entry.update || !entry.projectId) throw new Error(`${entry.name} is up to date.`);
  const result = plan(profile, [{ provider: entry.provider, projectId: entry.projectId, versionId: entry.update.versionId }]);
  if (result.conflicts.length) throw new Error(result.conflicts[0].message);
  return apply(
    profile,
    result.install.map((i) => ({ provider: i.provider, projectId: i.projectId, versionId: i.versionId })),
  );
}

/** A file for an entry that has to be downloaded by hand. */
export function provideFile(profile: Profile, entryId: string, filename: string, text: string): ContentIndex {
  const entry = entryOf(profile, entryId);
  const l = lock(profile.id);
  const expected = l.waiting.get(entryId);
  if (expected !== undefined && expected !== text) {
    throw new Error(
      `${filename} isn't ${entry.filename}: its checksum doesn't match. Download exactly that file from its download page.`,
    );
  }
  l.waiting.delete(entryId);
  return index(profile);
}

/** `text` is the file's contents, to match a waiting manual download. */
export function upload(profile: Profile, filename: string, size: number, text?: string): ContentIndex {
  const target = targetOf(profile);
  if (!filename.toLowerCase().endsWith('.jar')) throw new Error('Upload a .jar file.');
  const l = lock(profile.id);
  const waiting = [...l.waiting.entries()].find(([, expected]) => expected === text);
  if (waiting) {
    l.waiting.delete(waiting[0]);
    return index(profile);
  }
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
    manualDownload: null,
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
