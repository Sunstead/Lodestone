import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Input } from '@sunstead/ui/components/input';
import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { Separator } from '@sunstead/ui/components/separator';
import { Switch } from '@sunstead/ui/components/switch';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sunstead/ui/components/tabs';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@sunstead/ui/components/tooltip';
import { useInstallMod, useProfileMods, useToggleMod, useUninstallMod, useUpdateMod } from '#/api/queries';
import type { CatalogEntry, ModEntry, ModKind } from '#/api/types';
import { EmptyState, StatStrip } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatNumber, formatRelative } from '#/lib/format';
import {
  ArrowUpCircle,
  CloudDownload,
  Package,
  PackageOpen,
  RefreshCw,
  Search,
  Trash2,
} from 'lucide-react';
import { useMemo, useState } from 'react';

const SOURCE_LABEL: Record<ModEntry['source'], string> = {
  modrinth: 'Modrinth',
  spigotmc: 'SpigotMC',
  curseforge: 'CurseForge',
  hangar: 'Hangar',
  'built-in': 'Bundled',
};

function InstalledRow({
  entry,
  pending,
  onToggle,
  onUpdate,
  onUninstall,
}: {
  entry: ModEntry;
  pending: boolean;
  onToggle: (enabled: boolean) => void;
  onUpdate: () => void;
  onUninstall: () => void;
}) {
  return (
    <div className='flex flex-col gap-3 p-4 sm:flex-row sm:items-center'>
      <div className='flex min-w-0 flex-1 gap-3'>
        <span
          aria-hidden='true'
          className='flex size-10 shrink-0 items-center justify-center rounded-md border bg-muted/40'
        >
          <Package className='size-5 text-muted-foreground' />
        </span>
        <div className='min-w-0'>
          <div className='flex flex-wrap items-center gap-2'>
            <p className='truncate font-medium'>{entry.name}</p>
            <Badge variant='outline' className='font-mono text-[11px]'>
              v{entry.version}
            </Badge>
            {entry.updateAvailable ? (
              <Badge className='gap-1 bg-main/15 text-main'>
                <ArrowUpCircle className='size-3' />
                v{entry.latestVersion}
              </Badge>
            ) : null}
            {!entry.enabled ? <Badge variant='secondary'>Disabled</Badge> : null}
          </div>
          <p className='mt-0.5 line-clamp-1 text-sm text-muted-foreground'>{entry.description}</p>
          <p className='mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground'>
            <span>by {entry.author}</span>
            <span>{SOURCE_LABEL[entry.source]}</span>
            <span>{formatBytes(entry.sizeBytes)}</span>
            <span>{formatNumber(entry.downloads)} downloads</span>
          </p>
        </div>
      </div>

      <div className='flex items-center gap-2 sm:shrink-0'>
        <label className='flex items-center gap-2 text-xs text-muted-foreground'>
          {entry.enabled ? 'Enabled' : 'Off'}
          <Switch
            checked={entry.enabled}
            disabled={pending}
            aria-label={`${entry.enabled ? 'Disable' : 'Enable'} ${entry.name}`}
            onCheckedChange={onToggle}
          />
        </label>
        {entry.updateAvailable ? (
          <Button variant='outline' size='sm' disabled={pending} onClick={onUpdate}>
            <RefreshCw />
            Update
          </Button>
        ) : null}
        <Button
          variant='ghost'
          size='icon-sm'
          aria-label={`Remove ${entry.name}`}
          disabled={pending}
          onClick={onUninstall}
          className='text-destructive hover:text-destructive'
        >
          <Trash2 />
        </Button>
      </div>
    </div>
  );
}

function CatalogRow({
  entry,
  pending,
  onInstall,
}: {
  entry: CatalogEntry;
  pending: boolean;
  onInstall: () => void;
}) {
  return (
    <div className='flex flex-col gap-3 p-4 sm:flex-row sm:items-center'>
      <div className='min-w-0 flex-1'>
        <div className='flex flex-wrap items-center gap-2'>
          <p className='truncate font-medium'>{entry.name}</p>
          <Badge variant='outline' className='font-mono text-[11px]'>
            v{entry.latestVersion}
          </Badge>
          <Badge variant='secondary' className='capitalize'>
            {entry.kind}
          </Badge>
        </div>
        <p className='mt-0.5 line-clamp-1 text-sm text-muted-foreground'>{entry.description}</p>
        <p className='mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground'>
          <span>by {entry.author}</span>
          <span>{SOURCE_LABEL[entry.source]}</span>
          <span>{formatBytes(entry.sizeBytes)}</span>
          <span>{entry.categories.join(' · ')}</span>
        </p>
      </div>
      <BevelButton size='sm' className='px-4 py-4 text-sm sm:shrink-0' disabled={pending} onClick={onInstall}>
        <CloudDownload />
        Install
      </BevelButton>
    </div>
  );
}

/** Mods and plugins a profile installs into every instance that runs it. */
export function ModsPanel({ profileId }: { profileId: string }) {
  const query = useProfileMods(profileId);
  const install = useInstallMod(profileId);
  const uninstall = useUninstallMod(profileId);
  const toggle = useToggleMod(profileId);
  const update = useUpdateMod(profileId);

  const [tab, setTab] = useState('installed');
  const [filter, setFilter] = useState('');
  const [kind, setKind] = useState<'all' | ModKind>('all');

  const installed = query.data?.installed ?? [];
  const catalog = query.data?.catalog ?? [];
  const pending = install.isPending || uninstall.isPending || toggle.isPending || update.isPending;

  const enabled = installed.filter((entry) => entry.enabled);
  const updates = installed.filter((entry) => entry.updateAvailable);

  const filterFn = <T extends { name: string; description: string; kind: ModKind }>(entry: T) => {
    const needle = filter.trim().toLowerCase();
    if (kind !== 'all' && entry.kind !== kind) return false;
    if (!needle) return true;
    return entry.name.toLowerCase().includes(needle) || entry.description.toLowerCase().includes(needle);
  };

  const visibleInstalled = useMemo(() => installed.filter(filterFn), [installed, filter, kind]);
  const visibleCatalog = useMemo(() => catalog.filter(filterFn), [catalog, filter, kind]);

  const loaderIsPlugin = query.data?.loader.toLowerCase().includes('paper') ?? false;

  return (
    <div className='flex w-full min-w-0 flex-col gap-4'>
      <StatStrip
        items={[
          { label: 'Loader', value: query.data?.loader ?? '—' },
          { label: 'Installed', value: String(installed.length), hint: `${enabled.length} enabled` },
          { label: 'Updates', value: String(updates.length), hint: updates.map((entry) => entry.name).join(', ') || 'all current' },
          {
            label: 'On disk',
            value: formatBytes(installed.reduce((total, entry) => total + entry.sizeBytes, 0), 1),
          },
        ]}
      />

      <Tabs value={tab} onValueChange={setTab}>
        <div className='flex flex-wrap items-center gap-3'>
          <TabsList>
            <TabsTrigger value='installed'>Installed ({installed.length})</TabsTrigger>
            <TabsTrigger value='browse'>Browse ({catalog.length})</TabsTrigger>
          </TabsList>

          <div className='relative min-w-48 flex-1'>
            <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
            <Input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder='Search by name or description…'
              className='pl-8'
              aria-label='Search mods'
            />
          </div>

          <div className='flex items-center gap-1 rounded-md border p-1'>
            {(['all', loaderIsPlugin ? 'plugin' : 'mod'] as const).map((value) => (
              <Button
                key={value}
                variant={kind === value ? 'secondary' : 'ghost'}
                size='sm'
                className='capitalize'
                aria-pressed={kind === value}
                onClick={() => setKind(value)}
              >
                {value === 'all' ? 'All' : `${value}s`}
              </Button>
            ))}
          </div>
        </div>

        <TabsContent value='installed' className='mt-3'>
          <Card className='gap-0 overflow-hidden py-0'>
            {visibleInstalled.length === 0 ? (
              <CardContent className='p-6'>
                <EmptyState
                  icon={PackageOpen}
                  title={installed.length === 0 ? 'Nothing installed yet' : 'No matches'}
                  description={
                    installed.length === 0
                      ? 'Browse the catalog to add your first plugin, or drop jars into the profile folder yourself.'
                      : 'Try a different search or switch the filter back to All.'
                  }
                />
              </CardContent>
            ) : (
              <ScrollArea className='max-h-[34rem] [&>[data-slot=scroll-area-viewport]>div]:block!'>
                <div className='divide-y'>
                  {visibleInstalled.map((entry) => (
                    <InstalledRow
                      key={entry.id}
                      entry={entry}
                      pending={pending}
                      onToggle={(value) => toggle.mutate({ modId: entry.id, enabled: value })}
                      onUpdate={() => update.mutate(entry.id)}
                      onUninstall={() => uninstall.mutate(entry.id)}
                    />
                  ))}
                </div>
              </ScrollArea>
            )}
          </Card>
        </TabsContent>

        <TabsContent value='browse' className='mt-3'>
          {catalog.length === 0 ? (
            <Card>
              <CardContent className='p-6'>
                <EmptyState
                  icon={Package}
                  title='No catalog for this loader'
                  description='Vanilla profiles take no plugins or mods. Use a Paper or Fabric profile to browse the catalog.'
                />
              </CardContent>
            </Card>
          ) : (
            <Card className='gap-0 overflow-hidden py-0'>
              <CardHeader className='border-b bg-muted/30 px-4 py-3'>
                <CardTitle className='text-base'>Available for {query.data?.loader}</CardTitle>
                <CardDescription>
                  Filtered to versions this loader accepts. Installing adds the jar to the profile, so every instance that uses it picks it up on next restart.
                </CardDescription>
              </CardHeader>
              <ScrollArea className='max-h-[34rem] [&>[data-slot=scroll-area-viewport]>div]:block!'>
                <div className='divide-y'>
                  {visibleCatalog.map((entry) => (
                    <CatalogRow
                      key={entry.id}
                      entry={entry}
                      pending={pending}
                      onInstall={() => install.mutate(entry.id)}
                    />
                  ))}
                </div>
              </ScrollArea>
            </Card>
          )}
        </TabsContent>
      </Tabs>

      <Card>
        <CardHeader>
          <CardTitle className='text-base'>How changes are applied</CardTitle>
          <CardDescription>What Lodestone does behind the scenes</CardDescription>
        </CardHeader>
        <CardContent className='grid gap-4 text-sm sm:grid-cols-3'>
          <div>
            <p className='font-medium'>1. Download</p>
            <p className='mt-1 text-muted-foreground'>
              The jar is fetched from {loaderIsPlugin ? 'Modrinth/Hangar' : 'Modrinth/CurseForge'} and verified against the
              published hash.
            </p>
          </div>
          <div>
            <p className='font-medium'>2. Place</p>
            <p className='mt-1 text-muted-foreground'>
              Written to the profile's <code className='text-xs'>{loaderIsPlugin ? '/plugins' : '/mods'}</code> folder.
              Disabled jars move to a <code className='text-xs'>.disabled</code> folder.
            </p>
          </div>
          <div>
            <p className='font-medium'>3. Restart</p>
            <p className='mt-1 text-muted-foreground'>
              Every instance using this profile is flagged out of date and picks the change up on its next restart.
            </p>
          </div>
          <Separator className='sm:col-span-3' />
          <div className='sm:col-span-3'>
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger
                  render={
                    <span className='text-xs text-muted-foreground'>
                      Last catalog sync {formatRelative(new Date(Date.now() - 12 * 60_000).toISOString())}
                    </span>
                  }
                />
                <TooltipContent>Catalog metadata is cached in SQLite and refreshed every 6 hours.</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
