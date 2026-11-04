import { Alert, AlertDescription, AlertTitle } from '@sunstead/ui/components/alert';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@sunstead/ui/components/alert-dialog';
import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import { Card, CardContent } from '@sunstead/ui/components/card';
import { Checkbox } from '@sunstead/ui/components/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@sunstead/ui/components/dialog';
import { Input } from '@sunstead/ui/components/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@sunstead/ui/components/select';
import { Skeleton } from '@sunstead/ui/components/skeleton';
import { Switch } from '@sunstead/ui/components/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sunstead/ui/components/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@sunstead/ui/components/tooltip';
import { cn } from '@sunstead/ui/utils';
import {
  useApplyContent,
  useCheckContentUpdates,
  useContent,
  useContentSearch,
  useContentVersions,
  usePlanContent,
  useRemoveContent,
  useSetContentEnabled,
  useUpdateContent,
  useUploadContent,
} from '#/api/queries';
import type { ContentEntry, ContentProject, ContentRef, InstallPlan } from '#/api/types';
import { EmptyState, StatStrip } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatNumber, formatRelative } from '#/lib/format';
import {
  ArrowUpCircle,
  ExternalLink,
  Link2,
  Loader2,
  PackageOpen,
  RefreshCw,
  Search,
  Trash2,
  TriangleAlert,
  Upload,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';

const SOURCE_LABEL: Record<ContentEntry['provider'], string> = {
  modrinth: 'Modrinth',
  hangar: 'Hangar',
  curseforge: 'CurseForge',
  upload: 'Uploaded',
};

const LOADER_LABEL: Record<string, string> = { paper: 'Paper', fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge' };

type Sort = 'relevance' | 'downloads' | 'updated' | 'newest';
const SORTS: { value: Sort; label: string }[] = [
  { value: 'relevance', label: 'Relevance' },
  { value: 'downloads', label: 'Downloads' },
  { value: 'updated', label: 'Recently updated' },
  { value: 'newest', label: 'Newest' },
];

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return debounced;
}

function ProjectIcon({ url, name, className }: { url: string | null; name: string; className?: string }) {
  const [broken, setBroken] = useState(false);
  return (
    <div
      className={cn(
        'flex size-10 shrink-0 items-center justify-center overflow-hidden rounded-md border bg-muted text-sm font-semibold text-muted-foreground',
        className,
      )}
    >
      {url && !broken ? (
        <img src={url} alt='' loading='lazy' className='size-full object-cover' onError={() => setBroken(true)} />
      ) : (
        name.slice(0, 2).toUpperCase()
      )}
    </div>
  );
}

/** An icon button with a tooltip naming what it does. */
function IconAction({
  label,
  onClick,
  disabled,
  className,
  children,
  href,
}: {
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
  children: React.ReactNode;
  href?: string;
}) {
  const button = href ? (
    <Button variant='ghost' size='icon-sm' aria-label={label} className={className} render={<a href={href} target='_blank' rel='noreferrer' />}>
      {children}
    </Button>
  ) : (
    <Button variant='ghost' size='icon-sm' aria-label={label} disabled={disabled} onClick={onClick} className={className}>
      {children}
    </Button>
  );
  return (
    <Tooltip>
      <TooltipTrigger render={<span className='inline-flex' />}>{button}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/* ------------------------------ installed ------------------------------ */

function InstalledRow({
  entry,
  target,
  pending,
  onToggle,
  onUpdate,
  onRemove,
}: {
  entry: ContentEntry;
  target: string;
  pending: boolean;
  onToggle: (enabled: boolean) => void;
  onUpdate: () => void;
  onRemove: () => void;
}) {
  return (
    <div className={cn('flex flex-col gap-3 p-4 sm:flex-row sm:items-center', !entry.enabled && 'opacity-60')}>
      <div className='flex min-w-0 flex-1 items-start gap-3'>
        <ProjectIcon url={entry.iconUrl} name={entry.name} />
        <div className='min-w-0 flex-1'>
          <div className='flex flex-wrap items-center gap-x-2 gap-y-1'>
            <p className='truncate font-medium'>{entry.name}</p>
            {entry.versionName ? (
              <span className='font-mono text-xs text-muted-foreground'>{entry.versionName}</span>
            ) : null}
            {entry.update ? (
              <Badge variant='outline' className='gap-1 border-sky-500/40 text-sky-500'>
                <ArrowUpCircle className='size-3' />
                {entry.update.versionName}
              </Badge>
            ) : null}
            {!entry.compatible ? (
              <Badge variant='outline' className='gap-1 border-amber-500/40 text-amber-500'>
                <TriangleAlert className='size-3' />
                Not built for {target}
              </Badge>
            ) : null}
            {entry.requiredBy.length > 0 ? (
              <Badge variant='secondary' className='gap-1'>
                <Link2 className='size-3' />
                Needed by {entry.requiredBy.join(', ')}
              </Badge>
            ) : null}
          </div>
          <p className='mt-0.5 line-clamp-1 text-sm text-muted-foreground'>
            {entry.description || entry.filename}
          </p>
          <p className='mt-0.5 truncate text-xs text-muted-foreground'>
            {[entry.author && `by ${entry.author}`, SOURCE_LABEL[entry.provider], formatBytes(entry.sizeBytes, 1), entry.filename]
              .filter(Boolean)
              .join(' · ')}
          </p>
        </div>
      </div>
      <div className='flex shrink-0 items-center gap-1 sm:justify-end'>
        {entry.update ? (
          <Button variant='outline' size='sm' disabled={pending} onClick={onUpdate}>
            Update
          </Button>
        ) : null}
        {entry.pageUrl ? (
          <IconAction label={`Open on ${SOURCE_LABEL[entry.provider]}`} href={entry.pageUrl}>
            <ExternalLink />
          </IconAction>
        ) : null}
        <IconAction label='Remove' disabled={pending} onClick={onRemove} className='text-destructive hover:text-destructive'>
          <Trash2 />
        </IconAction>
        <Tooltip>
          <TooltipTrigger render={<span className='ml-1 inline-flex' />}>
            <Switch
              checked={entry.enabled}
              disabled={pending}
              onCheckedChange={(value) => onToggle(value)}
              aria-label={`${entry.enabled ? 'Disable' : 'Enable'} ${entry.name}`}
            />
          </TooltipTrigger>
          <TooltipContent>{entry.enabled ? 'Enabled: loaded on start' : 'Disabled: left out on start'}</TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
}

/* -------------------------------- browse ------------------------------- */

function ResultRow({
  project,
  onInstall,
  onChooseVersion,
  busy,
}: {
  project: ContentProject;
  onInstall: () => void;
  onChooseVersion: () => void;
  busy: boolean;
}) {
  return (
    <div className='flex flex-col gap-3 p-4 sm:flex-row sm:items-center'>
      <div className='flex min-w-0 flex-1 items-start gap-3'>
        <ProjectIcon url={project.iconUrl} name={project.name} />
        <div className='min-w-0 flex-1'>
          <div className='flex flex-wrap items-center gap-x-2 gap-y-1'>
            <a
              href={project.pageUrl}
              target='_blank'
              rel='noreferrer'
              className='truncate font-medium underline-offset-4 hover:underline'
            >
              {project.name}
            </a>
            <span className='text-xs text-muted-foreground'>by {project.author}</span>
          </div>
          <p className='mt-0.5 line-clamp-2 text-sm text-muted-foreground'>{project.description}</p>
          <div className='mt-1 flex flex-wrap items-center gap-1.5'>
            <span className='text-xs text-muted-foreground tabular-nums'>{formatNumber(project.downloads)} downloads</span>
            {project.categories.slice(0, 3).map((category) => (
              <Badge key={category} variant='secondary' className='text-[11px] capitalize'>
                {category}
              </Badge>
            ))}
          </div>
        </div>
      </div>
      <div className='flex shrink-0 items-center gap-1'>
        {project.installed ? (
          <Badge variant='outline'>Installed</Badge>
        ) : (
          <>
            <Button variant='ghost' size='sm' onClick={onChooseVersion} disabled={busy}>
              Versions
            </Button>
            <BevelButton className='px-3 py-3.5 text-xs' disabled={busy} onClick={onInstall}>
              Install
            </BevelButton>
          </>
        )}
      </div>
    </div>
  );
}

function VersionPicker({
  profileId,
  project,
  onClose,
  onPick,
}: {
  profileId: string;
  project: ContentProject | null;
  onClose: () => void;
  onPick: (versionId: string) => void;
}) {
  const versions = useContentVersions(profileId, project?.provider ?? 'modrinth', project?.id ?? null);
  return (
    <Dialog open={!!project} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className='sm:max-w-lg'>
        <DialogHeader>
          <DialogTitle>{project?.name} versions</DialogTitle>
          <DialogDescription>Only versions built for this configuration are listed. Newest first.</DialogDescription>
        </DialogHeader>
        <div className='-mx-1 max-h-[55vh] min-w-0 overflow-x-hidden overflow-y-auto px-1'>
          {versions.isLoading ? (
            <div className='space-y-2'>
              {[0, 1, 2].map((i) => (
                <Skeleton key={i} className='h-12' />
              ))}
            </div>
          ) : versions.error ? (
            <p className='text-sm text-destructive'>{versions.error.message}</p>
          ) : (
            <ul className='divide-y rounded-md border'>
              {(versions.data ?? []).map((version) => (
                <li key={version.id} className='flex items-center gap-3 px-3 py-2'>
                  <div className='min-w-0 flex-1'>
                    <p className='truncate text-sm font-medium'>{version.versionNumber}</p>
                    <p className='truncate text-xs text-muted-foreground'>
                      {formatRelative(version.publishedAt)} · {version.gameVersions.slice(0, 4).join(', ')}
                    </p>
                  </div>
                  {version.channel !== 'release' ? (
                    <Badge variant='outline' className='capitalize'>
                      {version.channel}
                    </Badge>
                  ) : null}
                  <Button variant='outline' size='sm' onClick={() => onPick(version.id)}>
                    Install
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/* ---------------------------- install plan ---------------------------- */

/**
 * What an install takes, before anything is downloaded: the requested
 * project, its required dependencies, optional extras to opt into, and
 * anything that blocks it.
 */
function PlanDialog({
  profileId,
  request,
  onClose,
  onInstalled,
}: {
  profileId: string;
  request: ContentRef[] | null;
  onClose: () => void;
  onInstalled: () => void;
}) {
  const planner = usePlanContent(profileId);
  const apply = useApplyContent(profileId);
  const [extras, setExtras] = useState<string[]>([]);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const requestKey = JSON.stringify(request);

  useEffect(() => {
    setExtras([]);
    setPlan(null);
    apply.reset();
  }, [requestKey]);

  useEffect(() => {
    if (!request) return;
    const items: ContentRef[] = [...request, ...extras.map((projectId) => ({ provider: 'modrinth' as const, projectId }))];
    planner.mutate(items, { onSuccess: setPlan });
  }, [requestKey, extras.join(',')]);

  const blocked = !plan || plan.conflicts.length > 0 || plan.install.length === 0;
  const total = plan?.install.reduce((sum, item) => sum + item.sizeBytes, 0) ?? 0;
  const title = plan?.install.find((item) => !item.requiredBy)?.name ?? 'Install';
  const error = planner.error ?? apply.error;

  return (
    <Dialog open={!!request} onOpenChange={(open) => !open && !apply.isPending && onClose()}>
      <DialogContent className='sm:max-w-lg'>
        <DialogHeader>
          <DialogTitle>{plan?.install.some((item) => item.replaces) && plan.install.length === 1 ? `Update ${title}` : `Install ${title}`}</DialogTitle>
          <DialogDescription>
            Files are checked against Modrinth's published hashes. Servers using this configuration pick them up on their next
            start.
          </DialogDescription>
        </DialogHeader>

        <div className='-mx-1 max-h-[55vh] min-w-0 space-y-4 overflow-x-hidden overflow-y-auto px-1'>
          {!plan && planner.isPending ? (
            <div className='space-y-2'>
              <Skeleton className='h-12' />
              <Skeleton className='h-12' />
            </div>
          ) : null}

          {plan && plan.install.length > 0 ? (
            <ul className='divide-y rounded-md border'>
              {plan.install.map((item) => (
                <li key={item.projectId} className='flex items-center gap-3 px-3 py-2.5'>
                  <ProjectIcon url={item.iconUrl} name={item.name} className='size-8' />
                  <div className='min-w-0 flex-1'>
                    <p className='truncate text-sm font-medium'>
                      {item.name} <span className='font-mono text-xs font-normal text-muted-foreground'>{item.versionNumber}</span>
                    </p>
                    <p className='truncate text-xs text-muted-foreground'>
                      {item.replaces
                        ? `Replaces ${item.replaces.versionName}`
                        : item.requiredBy
                          ? `Required by ${item.requiredBy}`
                          : 'Requested'}
                      {' · '}
                      {formatBytes(item.sizeBytes, 1)}
                    </p>
                  </div>
                  {item.requiredBy ? <Badge variant='secondary'>Dependency</Badge> : null}
                </li>
              ))}
            </ul>
          ) : null}

          {plan && plan.install.length === 0 && plan.conflicts.length === 0 ? (
            <p className='text-sm text-muted-foreground'>That's already installed at this version. Nothing to do.</p>
          ) : null}

          {plan && (plan.optional.length > 0 || extras.length > 0) ? (
            <div className='space-y-2'>
              <p className='text-sm font-medium'>Optional extras</p>
              {[...plan.optional.map((o) => ({ id: o.projectId, name: o.name, by: o.suggestedBy }))].map((option) => (
                <label key={option.id} className='flex items-center gap-2 text-sm'>
                  <Checkbox
                    checked={extras.includes(option.id)}
                    onCheckedChange={(checked) =>
                      setExtras((previous) => (checked ? [...previous, option.id] : previous.filter((id) => id !== option.id)))
                    }
                  />
                  <span>
                    {option.name} <span className='text-muted-foreground'>· works with {option.by}</span>
                  </span>
                </label>
              ))}
              {extras.length > 0 && plan.optional.length === 0 ? (
                <p className='text-xs text-muted-foreground'>Selected extras are part of the list above.</p>
              ) : null}
            </div>
          ) : null}

          {plan && plan.conflicts.length > 0 ? (
            <Alert variant='destructive'>
              <TriangleAlert />
              <AlertTitle>Can't install this as is</AlertTitle>
              <AlertDescription>
                <ul className='list-disc pl-4'>
                  {plan.conflicts.map((conflict, index) => (
                    <li key={index}>{conflict.message}</li>
                  ))}
                </ul>
              </AlertDescription>
            </Alert>
          ) : null}

          {error ? <p className='text-sm text-destructive'>{error.message}</p> : null}
        </div>

        <DialogFooter>
          <Button variant='outline' disabled={apply.isPending} onClick={onClose}>
            Cancel
          </Button>
          <BevelButton
            className='px-4 py-4 text-sm'
            disabled={blocked || apply.isPending}
            onClick={() =>
              plan &&
              apply.mutate(
                plan.install.map((item) => ({ provider: item.provider, projectId: item.projectId, versionId: item.versionId })),
                { onSuccess: onInstalled },
              )
            }
          >
            {apply.isPending ? <Loader2 className='animate-spin' /> : null}
            {plan && plan.install.length > 1 ? `Install ${plan.install.length} · ${formatBytes(total, 1)}` : 'Install'}
          </BevelButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/* --------------------------------- panel ------------------------------- */

/** Mods and plugins a profile installs into every server that runs it. */
export function ModsPanel({ profileId }: { profileId: string }) {
  const query = useContent(profileId);
  const toggle = useSetContentEnabled(profileId);
  const remove = useRemoveContent(profileId);
  const update = useUpdateContent(profileId);
  const check = useCheckContentUpdates(profileId);
  const upload = useUploadContent(profileId);
  const fileInput = useRef<HTMLInputElement>(null);

  const [tab, setTab] = useState('installed');
  const [filter, setFilter] = useState('');
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState<Sort>('relevance');
  const [planFor, setPlanFor] = useState<ContentRef[] | null>(null);
  const [versionsFor, setVersionsFor] = useState<ContentProject | null>(null);
  const [removing, setRemoving] = useState<ContentEntry | null>(null);

  const index = query.data;
  const entries = index?.entries ?? [];
  const isPlugins = index?.kind === 'plugins';
  const noun = isPlugins ? 'plugins' : 'mods';
  const target = index ? `${LOADER_LABEL[index.loaders[0]] ?? index.loaders[0] ?? ''} ${index.gameVersion}` : '';
  const updates = entries.filter((entry) => entry.update);
  const pending = toggle.isPending || remove.isPending || update.isPending || upload.isPending;
  const mutationError = toggle.error ?? remove.error ?? update.error ?? upload.error ?? check.error;

  const debounced = useDebounced(search.trim(), 300);
  const results = useContentSearch(tab === 'browse' && index?.kind !== 'vanilla' ? profileId : '', debounced, sort);
  const hits = results.data?.pages.flatMap((page) => page.hits) ?? [];
  const total = results.data?.pages[0]?.total ?? 0;

  const visible = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return needle
      ? entries.filter((e) => e.name.toLowerCase().includes(needle) || e.filename.toLowerCase().includes(needle))
      : entries;
  }, [entries, filter]);

  if (query.isLoading) {
    return (
      <div className='space-y-3'>
        <Skeleton className='h-20' />
        <Skeleton className='h-80' />
      </div>
    );
  }
  if (query.error) {
    return <p className='text-sm text-destructive'>{query.error.message}</p>;
  }
  if (!index || index.kind === 'vanilla') {
    return (
      <EmptyState
        icon={PackageOpen}
        title="Vanilla doesn't load mods or plugins"
        description='Switch this configuration to Paper for plugins, or Fabric or NeoForge for mods, in Settings.'
      />
    );
  }

  return (
    <TooltipProvider delay={300}>
      <div className='flex w-full min-w-0 flex-col gap-4'>
        <StatStrip
          items={[
            { label: 'Runs', value: target, hint: isPlugins ? 'Bukkit, Spigot and Paper plugins' : `${index.loaders.join(', ')} mods` },
            { label: 'Installed', value: String(entries.length), hint: `${entries.filter((e) => e.enabled).length} enabled` },
            {
              label: 'Updates',
              value: index.checkedAt ? String(updates.length) : '—',
              hint: index.checkedAt ? `checked ${formatRelative(index.checkedAt)}` : 'not checked yet',
            },
            { label: 'Size', value: formatBytes(entries.reduce((sum, e) => sum + e.sizeBytes, 0), 1) },
          ]}
        />

        <Tabs value={tab} onValueChange={setTab}>
          <div className='flex flex-wrap items-center gap-2'>
            <TabsList>
              <TabsTrigger value='installed'>Installed ({entries.length})</TabsTrigger>
              <TabsTrigger value='browse'>Browse Modrinth</TabsTrigger>
            </TabsList>
            <div className='ml-auto flex flex-wrap items-center gap-2'>
              {tab === 'installed' ? (
                <>
                  {updates.length > 0 ? (
                    <Button
                      variant='outline'
                      size='sm'
                      disabled={pending}
                      onClick={async () => {
                        for (const entry of updates) await update.mutateAsync(entry.id).catch(() => undefined);
                      }}
                    >
                      <ArrowUpCircle /> Update all ({updates.length})
                    </Button>
                  ) : null}
                  <Button variant='outline' size='sm' disabled={check.isPending} onClick={() => check.mutate()}>
                    <RefreshCw className={check.isPending ? 'animate-spin' : undefined} /> Check for updates
                  </Button>
                  <Button variant='outline' size='sm' disabled={upload.isPending} onClick={() => fileInput.current?.click()}>
                    {upload.isPending ? <Loader2 className='animate-spin' /> : <Upload />} Upload .jar
                  </Button>
                  <input
                    ref={fileInput}
                    type='file'
                    accept='.jar'
                    hidden
                    onChange={(event) => {
                      const file = event.target.files?.[0];
                      if (file) upload.mutate(file);
                      event.target.value = '';
                    }}
                  />
                </>
              ) : null}
            </div>
          </div>

          {mutationError ? <p className='mt-3 text-sm text-destructive'>{mutationError.message}</p> : null}

          <TabsContent value='installed' className='mt-3 space-y-3'>
            {entries.length > 0 ? (
              <div className='relative'>
                <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
                <Input
                  value={filter}
                  onChange={(event) => setFilter(event.target.value)}
                  placeholder={`Filter installed ${noun}…`}
                  className='pl-8'
                  aria-label={`Filter installed ${noun}`}
                />
              </div>
            ) : null}
            <Card className='gap-0 overflow-hidden py-0'>
              {visible.length === 0 ? (
                <CardContent className='p-6'>
                  <EmptyState
                    icon={PackageOpen}
                    title={entries.length === 0 ? `No ${noun} yet` : 'No matches'}
                    description={
                      entries.length === 0
                        ? `Browse Modrinth for ${noun} built for ${target}, or upload a .jar.`
                        : 'Try a different filter.'
                    }
                  >
                    {entries.length === 0 ? (
                      <Button variant='outline' size='sm' onClick={() => setTab('browse')}>
                        <Search /> Browse Modrinth
                      </Button>
                    ) : null}
                  </EmptyState>
                </CardContent>
              ) : (
                <div className='divide-y'>
                  {visible.map((entry) => (
                    <InstalledRow
                      key={entry.id}
                      entry={entry}
                      target={target}
                      pending={pending}
                      onToggle={(enabled) => toggle.mutate({ entry: entry.id, enabled })}
                      onUpdate={() =>
                        entry.projectId &&
                        entry.update &&
                        setPlanFor([{ provider: entry.provider, projectId: entry.projectId, versionId: entry.update.versionId }])
                      }
                      onRemove={() => (entry.requiredBy.length ? setRemoving(entry) : remove.mutate(entry.id))}
                    />
                  ))}
                </div>
              )}
            </Card>
            <p className='text-xs text-muted-foreground'>
              Changes apply when servers using this configuration next start. Lodestone keeps the {noun} folder's .jar files in
              step with this list; config folders beside them are left alone.
            </p>
          </TabsContent>

          <TabsContent value='browse' className='mt-3 space-y-3'>
            <div className='flex flex-wrap items-center gap-2'>
              <div className='relative min-w-48 flex-1'>
                <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
                <Input
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder={`Search Modrinth ${noun} for ${target}…`}
                  className='pl-8'
                  aria-label={`Search Modrinth ${noun}`}
                />
              </div>
              <Select items={SORTS} value={sort} onValueChange={(value) => value && setSort(value as Sort)}>
                <SelectTrigger className='w-44' aria-label='Sort results'>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SORTS.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Card className='gap-0 overflow-hidden py-0'>
              {results.isLoading ? (
                <div className='space-y-px'>
                  {[0, 1, 2, 3].map((i) => (
                    <Skeleton key={i} className='h-20 rounded-none' />
                  ))}
                </div>
              ) : results.error ? (
                <CardContent className='p-6'>
                  <EmptyState icon={TriangleAlert} title="Couldn't search Modrinth" description={results.error.message} />
                </CardContent>
              ) : hits.length === 0 ? (
                <CardContent className='p-6'>
                  <EmptyState
                    icon={Search}
                    title='Nothing found'
                    description={`No ${noun} matching "${debounced}" are built for ${target}.`}
                  />
                </CardContent>
              ) : (
                <div className={cn('divide-y', results.isPlaceholderData && 'opacity-60')}>
                  {hits.map((project) => (
                    <ResultRow
                      key={project.id}
                      project={project}
                      busy={!!planFor}
                      onInstall={() => setPlanFor([{ provider: project.provider, projectId: project.id }])}
                      onChooseVersion={() => setVersionsFor(project)}
                    />
                  ))}
                </div>
              )}
            </Card>
            <div className='flex items-center justify-between gap-2 text-xs text-muted-foreground'>
              <span>
                {total > 0 ? `${formatNumber(Math.min(hits.length, total))} of ${formatNumber(total)}` : null} · Only {noun} that run
                on {target} servers are shown.
              </span>
              {results.hasNextPage ? (
                <Button variant='outline' size='sm' disabled={results.isFetchingNextPage} onClick={() => results.fetchNextPage()}>
                  {results.isFetchingNextPage ? <Loader2 className='animate-spin' /> : null} Load more
                </Button>
              ) : null}
            </div>
          </TabsContent>
        </Tabs>

        <VersionPicker
          profileId={profileId}
          project={versionsFor}
          onClose={() => setVersionsFor(null)}
          onPick={(versionId) => {
            const project = versionsFor!;
            setVersionsFor(null);
            setPlanFor([{ provider: project.provider, projectId: project.id, versionId }]);
          }}
        />
        <PlanDialog
          profileId={profileId}
          request={planFor}
          onClose={() => setPlanFor(null)}
          onInstalled={() => {
            setPlanFor(null);
            setTab('installed');
          }}
        />
        <AlertDialog open={!!removing} onOpenChange={(open) => !open && setRemoving(null)}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Remove {removing?.name}?</AlertDialogTitle>
              <AlertDialogDescription>
                {removing?.requiredBy.join(', ')} {removing && removing.requiredBy.length === 1 ? 'needs' : 'need'} it, and may
                fail to load without it.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel render={<Button variant='outline' />}>Cancel</AlertDialogCancel>
              <AlertDialogAction
                render={
                  <BevelButton
                    tone='danger'
                    className='px-4 py-4 text-sm'
                    onClick={() => {
                      if (removing) remove.mutate(removing.id);
                      setRemoving(null);
                    }}
                  />
                }
              >
                Remove anyway
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </TooltipProvider>
  );
}
