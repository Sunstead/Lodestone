import { Badge } from '@sunstead/ui/components/badge';
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
import { useEffect, useState } from 'react';
import Plus from '@/assets/icons/plus.svg?react';
import backgroundImg from '@/assets/wallpaper/wallpaper_minecraft_pc_bundle_2560x1440.png';

import { detectMode, getMode, onModeChange } from '#/api/client';
import { useInstances, useInstanceAction, useInstanceStatus, useProfiles, useWorlds } from '#/api/queries';
import type { ApiMode } from '#/api/types';
import { CreateInstanceDialog } from '#/components/create-instance-dialog';
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
  const openWorldsDirectory = () => go({ section: 'directory', page: 'worlds' });
  const openProfilesDirectory = () => go({ section: 'directory', page: 'profiles' });
  const selectPage = (page: Page) => go({ section, assetId: location.assetId, page });

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
    section === 'directory' ? 'Directory' : `${SECTION_LABEL[section]} · ${activePage}`;

  const crumbs: Crumb[] = buildCrumbs({
    section,
    profile,
    world,
    activePage,
    openWorldsDirectory,
    openProfilesDirectory,
  });

  return (
    <ThemeProvider app='lodestone'>
      <div className='h-dvh w-full overflow-hidden'>
        <SidebarProvider className='h-full min-h-0!'>
          <LodestoneSidebar
            instances={instanceList}
            profiles={profileList}
            worlds={worldList}
            activeInstanceId={instanceId}
            activeProfileId={profileId}
            activeWorldId={worldId}
            onInstanceSelect={selectInstance}
            onProfileSelect={selectProfile}
            onWorldSelect={selectWorld}
            onOpenProfilesDirectory={openProfilesDirectory}
            onOpenWorldsDirectory={openWorldsDirectory}
          >
            <BevelButton className='py-6 text-lg' onClick={() => setCreateOpen(true)}>
              <Plus aria-hidden='true' className='size-6' />
              <span>Create Instance</span>
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
                    <NoInstances loading={instances.isLoading} onCreate={() => setCreateOpen(true)} />
                  )}

                  {/* The hero names the instance, so the strip is just tabs + controls. */}
                  <LocationHeader
                    variant='tabs-only'
                    pages={pages}
                    activePage={activePage}
                    onPageSelect={selectPage}
                    ribbonLabel={ribbonLabel}
                    actions={
                      instance ? (
                        <Button
                          variant='ghost'
                          size='sm'
                          disabled={action.isPending}
                          onClick={() =>
                            action.mutate(status.data?.state === 'running' ? 'restart' : 'start')
                          }
                        >
                          {status.data?.state === 'running' ? 'Restart' : 'Start'}
                        </Button>
                      ) : undefined
                    }
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
                      />
                    )
                  ) : null}
                </>
              ) : (
                <>
                  <LocationHeader
                    crumbs={crumbs}
                    pages={pages}
                    activePage={activePage}
                    onPageSelect={selectPage}
                    ribbonLabel={ribbonLabel}
                  />

                  {section === 'world' ? (
                    world ? (
                      <WorldDetailPage worldId={world.id} page={activePage} />
                    ) : (
                      <Loading />
                    )
                  ) : section === 'profile' ? (
                    profile ? (
                      <ProfileDetailPage profileId={profile.id} page={activePage} />
                    ) : (
                      <Loading />
                    )
                  ) : activePage === 'profiles' ? (
                    <ProfilesPage onOpenProfile={selectProfile} />
                  ) : (
                    <WorldsPage onOpenWorld={selectWorld} />
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
  // Directory: only the two list pages are valid here.
  const directoryPage: Page = page === 'profiles' ? 'profiles' : 'worlds';
  return { section: 'directory', assetId: '', page: directoryPage };
}

function buildCrumbs({
  section,
  profile,
  world,
  activePage,
  openWorldsDirectory,
  openProfilesDirectory,
}: {
  section: Section;
  profile?: { name: string };
  world?: { name: string };
  activePage: Page;
  openWorldsDirectory: () => void;
  openProfilesDirectory: () => void;
}): Crumb[] {
  if (section === 'world') {
    return [
      { label: SECTION_LABEL.world, onClick: openWorldsDirectory },
      { label: world?.name ?? 'World' },
    ];
  }
  if (section === 'profile') {
    return [
      { label: SECTION_LABEL.profile, onClick: openProfilesDirectory },
      { label: profile?.name ?? 'Profile' },
    ];
  }
  return [{ label: activePage === 'profiles' ? 'Profiles' : 'Worlds' }];
}

function NoInstances({ loading, onCreate }: { loading: boolean; onCreate: () => void }) {
  return (
    <div className='mx-auto flex w-full max-w-[1600px] flex-col items-start gap-3 p-6'>
      <p className='text-lg font-medium'>{loading ? 'Loading instances…' : 'No instances yet'}</p>
      {loading ? null : (
        <>
          <p className='text-sm text-muted-foreground'>
            An instance pairs a profile with a world. Both are kept even if you delete the instance.
          </p>
          <BevelButton className='px-4 py-5 text-base' onClick={onCreate}>
            <Plus aria-hidden='true' />
            Create Instance
          </BevelButton>
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
