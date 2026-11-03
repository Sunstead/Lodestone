import { Badge } from '@sunstead/ui/components/badge';
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
import { Button } from '@sunstead/ui/components/button';
import {
  SidebarInset,
  SidebarProvider,
} from '@sunstead/ui/components/resizable-sidebar';
import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { Skeleton } from '@sunstead/ui/components/skeleton';
import { Toaster } from '@sunstead/ui/components/sonner';
import { ThemeProvider } from '@sunstead/ui/theme-provider';
import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { TriangleAlert } from 'lucide-react';
import { useEffect, useState } from 'react';
import Plus from '@/assets/icons/plus.svg?react';
import backgroundImg from '@/assets/wallpaper/wallpaper_minecraft_pc_bundle_2560x1440.png';

import { detectMode, getMode, onModeChange } from '#/api/client';
import { useDeleteInstance, useInstances, useInstanceAction, useInstanceStatus, useProfiles, useWorlds } from '#/api/queries';
import type { ApiMode, Instance } from '#/api/types';
import { CreateInstanceDialog } from '#/components/create-instance-dialog';
import { LaunchWorldDialog } from '#/components/launch-world-dialog';
import { LocationHeader } from '#/components/location-header';
import type { Crumb } from '#/components/location-header';
import {
  DEFAULT_PAGE,
  LodestoneSidebar,
  pagesOf,
  SECTION_LABEL,
  isPage,
} from '#/components/server-navigation';
import type { Page, Section } from '#/components/server-navigation';
import { BevelButton, ServerHero } from '#/components/server-hero';
import { AllBackupsPage } from '#/routes/-pages/all-backups';
import { AppSettingsPage } from '#/routes/-pages/app-settings';
import { ConsolePage } from '#/routes/-pages/console';
import { DashboardPage } from '#/routes/-pages/dashboard';
import { PlayersPage } from '#/routes/-pages/players';
import { ProfileDetailPage } from '#/routes/-pages/profile-detail';
import { ProfilesPage } from '#/routes/-pages/profiles';
import { WorldDetailPage } from '#/routes/-pages/world-detail';
import { WorldsPage } from '#/routes/-pages/worlds';

/**
 * The whole location lives in three search params, but only one asset is
 * ever set for a given section:
 *
 *   ?instance=<id>  instance section
 *   ?profile=<id>   profile section
 *   ?world=<id>     world section
 *   (none set)      the directory, showing whichever list `page` names
 *
 * Keeping a single asset param populated means conflicting states — a
 * world open while the ribbon shows the worlds list — cannot be expressed.
 */
type Location = {
  section: Section;
  assetId: string;
  page: Page;
};

export const Route = createFileRoute('/')({
  validateSearch: (
    search: Record<string, unknown>,
  ): { instance: string; profile: string; world: string; page: Page | '' } => ({
    instance: typeof search.instance === 'string' ? search.instance : '',
    profile: typeof search.profile === 'string' ? search.profile : '',
    world: typeof search.world === 'string' ? search.world : '',
    page: isPage(search.page) ? search.page : '',
  }),
  component: Home,
});

function DemoBadge() {
  const [mode, setMode] = useState<ApiMode>(getMode());

  useEffect(() => {
    const unsubscribe = onModeChange(setMode);
    void detectMode();
    return unsubscribe;
  }, []);

  if (mode === 'live') return null;

  return (
    <Badge
      variant='secondary'
      className='pointer-events-none fixed right-3 bottom-3 z-50 gap-1.5 border border-border bg-card/90 text-xs backdrop-blur'
      title='The Rust backend is not reachable, so every call is served by the in-memory demo store.'
    >
      <span className='size-1.5 rounded-full bg-yellow-500' />
      Demo data · no backend
    </Badge>
  );
}

function Home() {
  const search = Route.useSearch();
  const navigate = useNavigate({ from: '/' });
  const instances = useInstances();
  const profiles = useProfiles();
  const worlds = useWorlds();
  const [createOpen, setCreateOpen] = useState(false);
  const [launchOpen, setLaunchOpen] = useState(false);
  const [launchWorldId, setLaunchWorldId] = useState<string | undefined>(undefined);
  const [pendingDeleteServer, setPendingDeleteServer] = useState<Instance | null>(null);
  const removeInstance = useDeleteInstance();

  const instanceList = instances.data ?? [];
  const profileList = profiles.data ?? [];
  const worldList = worlds.data ?? [];

  // Resolve which asset the URL points at, and drop anything that no longer
  // exists so a deleted asset falls back to its directory instead of a blank.
  const instanceId = instanceList.some((entry) => entry.id === search.instance)
    ? search.instance
    : '';
  const profileId = profileList.some((entry) => entry.id === search.profile) ? search.profile : '';
  const worldId = worldList.some((entry) => entry.id === search.world) ? search.world : '';

  const location = resolveLocation({ instanceId, profileId, worldId, page: search.page });
  const { section, page: activePage } = location;

  const instance = instanceList.find((entry) => entry.id === instanceId);
  const profile = profileList.find((entry) => entry.id === profileId);
  const world = worldList.find((entry) => entry.id === worldId);

  const status = useInstanceStatus(section === 'instance' ? instanceId : '');
  const action = useInstanceAction(instanceId);

  /** Build the canonical URL for a location. Only its own asset is kept. */
  const urlFor = (next: { section: Section; assetId?: string; page?: Page }) => {
    const base = { instance: '', profile: '', world: '' };
    const assetId = next.assetId ?? '';
    const page = next.page ?? DEFAULT_PAGE[next.section];
    if (next.section === 'instance') return { ...base, instance: assetId, page };
    if (next.section === 'profile') return { ...base, profile: assetId, page };
    if (next.section === 'world') return { ...base, world: assetId, page };
    return { ...base, page };
  };

  const go = (next: { section: Section; assetId?: string; page?: Page }, replace = false) => {
    void navigate({ search: urlFor(next), replace });
  };

  const selectInstance = (id: string) => go({ section: 'instance', assetId: id });
  const selectProfile = (id: string) => go({ section: 'profile', assetId: id });
  const selectWorld = (id: string) => go({ section: 'world', assetId: id });
  const openDirectory = (page: Page) => go({ section: 'directory', page });
  const openConfigurations = () => openDirectory('profiles');
  const openWorldsPage = () => openDirectory('worlds');
  const openBackupsPage = () => openDirectory('all-backups');
  const openSettingsPage = () => openDirectory('app-settings');
  const selectPage = (page: Page) => go({ section, assetId: location.assetId, page });

  /** Open the launch flow, optionally seeded with a world to use. */
  const launchWorld = (worldId?: string) => {
    setLaunchWorldId(worldId);
    setLaunchOpen(true);
  };

  /** Open the full instance dialog for people who want to pick an existing configuration. */
  const openCreateServer = () => setCreateOpen(true);

  /** Sidebar dropdown: jump to the configuration or world a server is made of. */
  const openServerConfig = (server: Instance) => selectProfile(server.profileId);
  const openServerWorld = (server: Instance) => selectWorld(server.worldId);

  // Keep the URL honest after lists load or an asset is deleted: fall back to
  // the first instance, otherwise to the directory, never to a stale id.
  useEffect(() => {
    if (instances.isLoading || instanceList.length === 0) return;
    if (search.instance !== instanceId && search.instance !== '') {
      go({ section: 'instance', assetId: instanceId }, true);
    }
  }, [instances.isLoading, instanceList.length, search.instance, instanceId]);

  useEffect(() => {
    if (profiles.isLoading || search.profile === '' || profileId !== '') return;
    go({ section: 'directory', page: 'profiles' }, true);
  }, [profiles.isLoading, search.profile, profileId]);

  useEffect(() => {
    if (worlds.isLoading || search.world === '' || worldId !== '') return;
    go({ section: 'directory', page: 'worlds' }, true);
  }, [worlds.isLoading, search.world, worldId]);

  const pages = pagesOf(section);
  const ribbonLabel =
    section === 'directory'
      ? (pages.find((entry) => entry.id === activePage)?.label ?? 'Directory')
      : `${SECTION_LABEL[section]} · ${activePage}`;

  const crumbs: Crumb[] = buildCrumbs({
    section,
    profile,
    world,
    activePage,
    openConfigurations,
    openWorldsPage,
  });

  return (
    <ThemeProvider app='lodestone'>
      <div className='h-dvh w-full overflow-hidden'>
        <SidebarProvider className='h-full min-h-0!'>
          <LodestoneSidebar
            instances={instanceList}
            activeInstanceId={instanceId}
            activePage={activePage}
            section={section}
            onInstanceSelect={selectInstance}
            onInstanceOpenConfig={openServerConfig}
            onInstanceOpenWorld={openServerWorld}
            onInstanceDelete={setPendingDeleteServer}
            onOpenConfigurations={openConfigurations}
            onOpenWorlds={openWorldsPage}
            onOpenBackups={openBackupsPage}
            onOpenSettings={openSettingsPage}
          >
            <BevelButton className='py-6 text-lg' onClick={() => launchWorld()}>
              <Plus aria-hidden='true' className='size-6' />
              <span>Launch world</span>
            </BevelButton>
          </LodestoneSidebar>

          <SidebarInset className='min-h-0! min-w-0 overflow-hidden'>
            <ScrollArea
              key={`${section}-${location.assetId}-${activePage}`}
              className='server-content-scroll-area min-h-0 flex-1'
            >
              {section === 'instance' ? (
                <>
                  {instance ? (
                    <ServerHero
                      instance={instance}
                      status={status.data}
                      busy={action.isPending}
                      backgroundImage={backgroundImg}
                      onStart={() => action.mutate('start')}
                      onStop={() => action.mutate('stop')}
                      onRestart={() => action.mutate('restart')}
                      onOpenProfile={() => selectProfile(instance.profileId)}
                      onOpenWorld={() => selectWorld(instance.worldId)}
                    />
                  ) : (
                    <NoInstances loading={instances.isLoading} onLaunch={() => launchWorld()} onCreate={openCreateServer} />
                  )}

                  {/* The hero names the instance and owns start/stop, so this strip is just page tabs. */}
                  <LocationHeader
                    variant='tabs-only'
                    pages={pages}
                    activePage={activePage}
                    onPageSelect={selectPage}
                    ribbonLabel={ribbonLabel}
                  />

                  {instance ? (
                    activePage === 'console' ? (
                      <ConsolePage instanceId={instanceId} />
                    ) : activePage === 'players' ? (
                      <PlayersPage instanceId={instanceId} />
                    ) : (
                      <DashboardPage
                        instanceId={instanceId}
                        status={status.data}
                        playersOnline={status.data?.players.online ?? instance.players.online}
                        onOpenProfile={() => selectProfile(instance.profileId)}
                        onOpenWorld={() => selectWorld(instance.worldId)}
                      />
                    )
                  ) : null}
                </>
              ) : (
                <>
                  {section === 'directory' ? (
                    <LocationHeader variant='title-only' ribbonLabel={ribbonLabel} />
                  ) : (
                    <LocationHeader
                      crumbs={crumbs}
                      pages={pages}
                      activePage={activePage}
                      onPageSelect={selectPage}
                      ribbonLabel={ribbonLabel}
                    />
                  )}

                  {section === 'world' ? (
                    world ? (
                      <WorldDetailPage
                        worldId={world.id}
                        page={activePage}
                        onOpenInstance={selectInstance}
                        onLaunch={() => launchWorld(world.id)}
                      />
                    ) : (
                      <Loading />
                    )
                  ) : section === 'profile' ? (
                    profile ? (
                      <ProfileDetailPage
                        profileId={profile.id}
                        page={activePage}
                        onOpenInstance={selectInstance}
                        onLaunch={() => launchWorld()}
                      />
                    ) : (
                      <Loading />
                    )
                  ) : activePage === 'profiles' ? (
                    <ProfilesPage onOpenProfile={selectProfile} onOpenInstance={selectInstance} />
                  ) : activePage === 'all-backups' ? (
                    <AllBackupsPage onOpenWorld={selectWorld} onOpenWorlds={openWorldsPage} />
                  ) : activePage === 'app-settings' ? (
                    <AppSettingsPage />
                  ) : (
                    <WorldsPage
                      onOpenWorld={selectWorld}
                      onOpenInstance={selectInstance}
                      onLaunchWorld={launchWorld}
                    />
                  )}
                </>
              )}
            </ScrollArea>
          </SidebarInset>
        </SidebarProvider>

        <CreateInstanceDialog
          open={createOpen}
          onOpenChange={setCreateOpen}
          onCreated={(id) => selectInstance(id)}
        />
        <LaunchWorldDialog
          open={launchOpen}
          onOpenChange={(next) => {
            setLaunchOpen(next);
            if (!next) setLaunchWorldId(undefined);
          }}
          worldId={launchWorldId}
          onLaunched={(id) => selectInstance(id)}
        />
        <AlertDialog
          open={pendingDeleteServer !== null}
          onOpenChange={(open) => (open ? undefined : setPendingDeleteServer(null))}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Delete this server?</AlertDialogTitle>
              <AlertDialogDescription>
                {pendingDeleteServer
                  ? `${pendingDeleteServer.name} will be removed. Its configuration and the world it ran are kept — you can launch that world again at any time.`
                  : ''}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm'>
              <TriangleAlert className='mt-0.5 size-4 shrink-0 text-destructive' />
              <p>The container is removed. Nothing is deleted from the world's data on disk.</p>
            </div>
            <AlertDialogFooter>
              <AlertDialogCancel render={<Button variant='outline' />}>Cancel</AlertDialogCancel>
              <AlertDialogAction
                render={
                  <BevelButton
                    tone='danger'
                    className='px-4 py-4 text-sm'
                    onClick={() => {
                      if (pendingDeleteServer) removeInstance.mutate(pendingDeleteServer.id);
                      setPendingDeleteServer(null);
                    }}
                  />
                }
              >
                Delete server
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
        <DemoBadge />
        <Toaster />
      </div>
    </ThemeProvider>
  );
}

/**
 * Turn the raw params into exactly one section. Instance wins when several
 * are set only because instance selection is the common case; the canonical
 * URLs we build never set more than one anyway.
 */
function resolveLocation({
  instanceId,
  profileId,
  worldId,
  page,
}: {
  instanceId: string;
  profileId: string;
  worldId: string;
  page: Page | '';
}): Location {
  if (worldId) {
    // A world's own pages are the only ones it shows.
    const fallback = DEFAULT_PAGE.world;
    const valid = pagesOf('world').some((entry) => entry.id === page);
    return { section: 'world', assetId: worldId, page: valid ? (page as Page) : fallback };
  }
  if (profileId) {
    const fallback = DEFAULT_PAGE.profile;
    const valid = pagesOf('profile').some((entry) => entry.id === page);
    return { section: 'profile', assetId: profileId, page: valid ? (page as Page) : fallback };
  }
  if (instanceId) {
    const fallback = DEFAULT_PAGE.instance;
    const valid = pagesOf('instance').some((entry) => entry.id === page);
    return { section: 'instance', assetId: instanceId, page: valid ? (page as Page) : fallback };
  }
  // Directory: only the list pages are valid here.
  const directoryPage: Page = pagesOf('directory').some((entry) => entry.id === page)
    ? (page as Page)
    : 'worlds';
  return { section: 'directory', assetId: '', page: directoryPage };
}

function buildCrumbs({
  section,
  profile,
  world,
  activePage,
  openConfigurations,
  openWorldsPage,
}: {
  section: Section;
  profile?: { name: string };
  world?: { name: string };
  activePage: Page;
  openConfigurations: () => void;
  openWorldsPage: () => void;
}): Crumb[] {
  if (section === 'world') {
    return [
      { label: SECTION_LABEL.world, onClick: openWorldsPage },
      { label: world?.name ?? 'World' },
    ];
  }
  if (section === 'profile') {
    return [
      { label: SECTION_LABEL.profile, onClick: openConfigurations },
      { label: profile?.name ?? 'Configuration' },
    ];
  }
  const label =
    activePage === 'profiles'
      ? 'Configurations'
      : activePage === 'all-backups'
        ? 'Backups'
        : activePage === 'app-settings'
          ? 'Settings'
          : 'Worlds';
  return [{ label }];
}

function NoInstances({
  loading,
  onLaunch,
  onCreate,
}: {
  loading: boolean;
  onLaunch: () => void;
  onCreate: () => void;
}) {
  return (
    <div className='mx-auto flex w-full max-w-[1600px] flex-col items-start gap-3 p-6'>
      <p className='text-lg font-medium'>{loading ? 'Loading servers…' : 'No servers yet'}</p>
      {loading ? null : (
        <>
          <p className='text-sm text-muted-foreground'>
            Launch a world to create one. A server pairs a configuration with a world — both are kept
            even if you delete the server.
          </p>
          <div className='flex flex-wrap items-center gap-2'>
            <BevelButton className='px-4 py-5 text-base' onClick={onLaunch}>
              <Plus aria-hidden='true' />
              Launch world
            </BevelButton>
            <Button variant='outline' size='sm' onClick={onCreate}>
              Use an existing configuration
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

function Loading() {
  return (
    <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
      <Skeleton className='h-28 w-full' />
      <Skeleton className='h-96 w-full' />
    </div>
  );
}
