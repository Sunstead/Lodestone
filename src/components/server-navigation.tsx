import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarRail,
} from '@sunstead/ui/components/resizable-sidebar';
import {
  Box,
  Container,
  FileText,
  Gamepad2,
  Gauge,
  Layers,
  List,
  Puzzle,
  Settings,
  Terminal,
  UserGroup,
  type LucideIcon,
} from 'lucide-react';
import type { ReactNode } from 'react';
import { SidebarInstanceButton } from './sidebar-server-button';
import { SidebarAssetButton } from './sidebar-asset-button';
import type { Instance, Profile, World } from '#/api/types';

/**
 * Lodestone has three asset kinds plus the two directories that list them.
 * A location is exactly one of these sections; the pages below it belong
 * to that section and never leak into another one's ribbon.
 */
export type Section = 'instance' | 'profile' | 'world' | 'directory';

/**
 * Every page is owned by exactly one section:
 *
 *   instance   what the container is doing right now
 *   profile    software, version, mods, properties, limits
 *   world      terrain, gameplay rules, backups
 *   directory  the lists of profiles and worlds
 */
export const PAGES = [
  { id: 'dashboard', label: 'Dashboard', icon: Gauge, section: 'instance' },
  { id: 'console', label: 'Console', icon: Terminal, section: 'instance' },
  { id: 'players', label: 'Players', icon: UserGroup, section: 'instance' },

  { id: 'gameplay', label: 'Gameplay', icon: Gamepad2, section: 'world' },
  { id: 'generation', label: 'Generation', icon: Layers, section: 'world' },
  { id: 'backups', label: 'Backups', icon: Container, section: 'world' },

  { id: 'settings', label: 'Settings', icon: Settings, section: 'profile' },
  { id: 'properties', label: 'Properties', icon: FileText, section: 'profile' },
  { id: 'mods', label: 'Mods & Plugins', icon: Puzzle, section: 'profile' },

  { id: 'worlds', label: 'Worlds', icon: Layers, section: 'directory' },
  { id: 'profiles', label: 'Profiles', icon: Box, section: 'directory' },
] as const satisfies readonly {
  id: string;
  label: string;
  icon: LucideIcon;
  section: Section;
}[];

export type Page = (typeof PAGES)[number]['id'];
export type PageMeta = (typeof PAGES)[number];

export function isPage(value: unknown): value is Page {
  return PAGES.some((page) => page.id === value);
}

/** Pages that can be shown while a given section is active. */
export function pagesOf(section: Section): readonly PageMeta[] {
  return PAGES.filter((page) => page.section === section);
}

/** The page a section opens on when you click its asset in the sidebar. */
export const DEFAULT_PAGE: Record<Section, Page> = {
  instance: 'dashboard',
  profile: 'settings',
  world: 'gameplay',
  directory: 'worlds',
};

/** Human name used in breadcrumbs and the ribbon's accessible label. */
export const SECTION_LABEL: Record<Section, string> = {
  instance: 'Instance',
  profile: 'Profiles',
  world: 'Worlds',
  directory: 'Directory',
};

export function LodestoneSidebar({
  instances,
  profiles,
  worlds,
  activeInstanceId,
  activeProfileId,
  activeWorldId,
  onInstanceSelect,
  onProfileSelect,
  onWorldSelect,
  onOpenProfilesDirectory,
  onOpenWorldsDirectory,
  children,
}: {
  instances: Instance[];
  profiles: Profile[];
  worlds: World[];
  activeInstanceId: string;
  activeProfileId: string;
  activeWorldId: string;
  onInstanceSelect: (id: string) => void;
  onProfileSelect: (id: string) => void;
  onWorldSelect: (id: string) => void;
  onOpenProfilesDirectory: () => void;
  onOpenWorldsDirectory: () => void;
  children: ReactNode;
}) {
  const running = instances.filter((instance) => instance.state === 'running').length;

  return (
    <Sidebar>
      <SidebarHeader className='border-b text-xl font-semibold'>
        <div className='px-2 py-1.5 tracking-wide'>LODESTONE</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarMenu className='gap-2 px-2'>{children}</SidebarMenu>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>
            Instances
            <span className='ml-auto text-xs font-normal text-muted-foreground tabular-nums'>
              {running}/{instances.length} online
            </span>
          </SidebarGroupLabel>
          <SidebarMenu className='space-y-1 px-2'>
            {instances.map((instance) => (
              <SidebarInstanceButton
                key={instance.id}
                name={instance.name}
                version={`${instance.profile.software} ${instance.profile.version}`}
                state={instance.state}
                active={instance.id === activeInstanceId}
                onClick={() => onInstanceSelect(instance.id)}
              />
            ))}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Profiles</SidebarGroupLabel>
          <SidebarGroupAction
            aria-label='Show all profiles'
            title='Show all profiles'
            onClick={onOpenProfilesDirectory}
          >
            <List />
          </SidebarGroupAction>
          <SidebarMenu className='space-y-1 px-2'>
            {profiles.map((profile) => (
              <SidebarAssetButton
                key={profile.id}
                kind='profile'
                name={profile.name}
                version={`${profile.software} ${profile.version}`}
                active={profile.id === activeProfileId}
                badge={
                  profile.instanceCount > 0
                    ? { label: `${profile.instanceCount} attached`, tone: 'muted' }
                    : { label: 'free', tone: 'main' }
                }
                onClick={() => onProfileSelect(profile.id)}
              />
            ))}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Worlds</SidebarGroupLabel>
          <SidebarGroupAction
            aria-label='Show all worlds'
            title='Show all worlds'
            onClick={onOpenWorldsDirectory}
          >
            <List />
          </SidebarGroupAction>
          <SidebarMenu className='space-y-1 px-2'>
            {worlds.map((world) => (
              <SidebarAssetButton
                key={world.id}
                kind='world'
                name={world.name}
                version={`${world.folder} · ${world.levelType.replace('minecraft:', '')}`}
                active={world.id === activeWorldId}
                badge={
                  world.attachedInstanceId
                    ? { label: 'attached', tone: 'muted' }
                    : { label: 'free', tone: 'main' }
                }
                onClick={() => onWorldSelect(world.id)}
              />
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className='border-t p-2 text-xs text-muted-foreground'>
        <span className='px-2'>Docker Engine · SQLite</span>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}

/**
 * The page ribbon: the tabs that belong to the current section and nothing
 * else. It is a plain tab strip — the asset identity lives in the header
 * breadcrumb above it, so no directory entry is mixed in here.
 */
export function PageRibbon({
  pages,
  activePage,
  onPageSelect,
  label,
}: {
  pages: readonly { id: Page; label: string; icon: LucideIcon }[];
  activePage: Page;
  onPageSelect: (page: Page) => void;
  /** Names the asset these pages belong to, for screen readers. */
  label: string;
}) {
  return (
    <nav
      aria-label={`${label} pages`}
      className='scrollbar-hidden flex min-w-0 flex-1 items-stretch overflow-x-auto overflow-y-hidden'
    >
      {pages.map(({ id, label: pageLabel, icon: Icon }) => (
        <a
          key={id}
          href={`?page=${id}`}
          aria-current={activePage === id ? 'page' : undefined}
          onClick={(event) => {
            event.preventDefault();
            onPageSelect(id);
          }}
          className={`flex h-full shrink-0 items-center gap-2 px-4 py-4 text-sm font-medium transition-colors hover:bg-muted/60 ${
            activePage === id ? 'shadow-[inset_0_-4px_0_0_var(--color-main)]' : 'text-muted-foreground'
          }`}
        >
          <Icon aria-hidden='true' className='size-4' />
          <span>{pageLabel}</span>
        </a>
      ))}
    </nav>
  );
}
