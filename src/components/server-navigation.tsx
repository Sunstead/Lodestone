import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuItem,
  SidebarMenuButton,
  SidebarRail,
} from '@sunstead/ui/components/resizable-sidebar';
import {
  Box,
  Container,
  FileText,
  Gamepad2,
  Gauge,
  Layers,
  Puzzle,
  Settings,
  Terminal,
  UserGroup,
  type LucideIcon,
} from 'lucide-react';
import type { ReactNode } from 'react';
import { SidebarInstanceButton } from './sidebar-server-button';
import { useInstanceAction } from '#/api/queries';
import type { Instance } from '#/api/types';

/**
 * Lodestone has three asset kinds plus the directories that list them.
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
 *   directory  the lists of configurations and worlds
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
  { id: 'profiles', label: 'Configurations', icon: Box, section: 'directory' },
  { id: 'all-backups', label: 'Backups', icon: Container, section: 'directory' },
  { id: 'app-settings', label: 'Settings', icon: Settings, section: 'directory' },
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
  instance: 'Server',
  profile: 'Configurations',
  world: 'Worlds',
  directory: 'Directory',
};

/**
 * The sidebar lists the servers you run, then a flat set of links to the
 * shared libraries (configurations, worlds) and the app's own pages
 * (backups, settings). Assets are no longer enumerated here — each list
 * lives on its own page, so the sidebar stays short no matter how many
 * worlds or configurations exist.
 */
export function LodestoneSidebar({
  instances,
  activeInstanceId,
  activePage,
  section,
  onInstanceOpenConfig,
  onInstanceOpenWorld,
  onInstanceDelete,
  onInstanceSelect,
  onOpenConfigurations,
  onOpenWorlds,
  onOpenBackups,
  onOpenSettings,
  children,
}: {
  instances: Instance[];
  activeInstanceId: string;
  activePage: Page;
  section: Section;
  onInstanceSelect: (id: string) => void;
  onInstanceOpenConfig: (instance: Instance) => void;
  onInstanceOpenWorld: (instance: Instance) => void;
  onInstanceDelete: (instance: Instance) => void;
  onOpenConfigurations: () => void;
  onOpenWorlds: () => void;
  onOpenBackups: () => void;
  onOpenSettings: () => void;
  children: ReactNode;
}) {
  const running = instances.filter((instance) => instance.state === 'running').length;

  const links = [
    { label: 'Configurations', icon: Box, page: 'profiles' as Page, onClick: onOpenConfigurations },
    { label: 'Worlds', icon: Layers, page: 'worlds' as Page, onClick: onOpenWorlds },
    { label: 'Backups', icon: Container, page: 'all-backups' as Page, onClick: onOpenBackups },
    { label: 'Settings', icon: Settings, page: 'app-settings' as Page, onClick: onOpenSettings },
  ];

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
            Servers
            <span className='ml-auto text-xs font-normal text-muted-foreground tabular-nums'>
              {running}/{instances.length} online
            </span>
          </SidebarGroupLabel>
          <SidebarMenu className='space-y-1 px-2'>
            {instances.length === 0 ? (
              <p className='px-2 py-1.5 text-sm text-muted-foreground'>
                No servers yet — launch a world to create one.
              </p>
            ) : (
              instances.map((instance) => (
                <SidebarServerRow
                  key={instance.id}
                  instance={instance}
                  active={instance.id === activeInstanceId}
                  onSelect={() => onInstanceSelect(instance.id)}
                  onOpenConfig={() => onInstanceOpenConfig(instance)}
                  onOpenWorld={() => onInstanceOpenWorld(instance)}
                  onDelete={() => onInstanceDelete(instance)}
                />
              ))
            )}
          </SidebarMenu>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Library</SidebarGroupLabel>
          <SidebarMenu className='space-y-1 px-2'>
            {links.map(({ label, icon: Icon, page, onClick }) => {
              const active = section === 'directory' && activePage === page;
              return (
                <SidebarMenuItem key={label}>
                  <SidebarMenuButton
                    onClick={onClick}
                    aria-current={active ? 'page' : undefined}
                    className={`gap-3 border-l-4 py-2 ${
                      active ? 'border-l-main bg-sidebar-accent' : 'border-l-transparent'
                    }`}
                  >
                    <Icon aria-hidden='true' className='size-4 text-muted-foreground' />
                    <span className='text-base text-foreground'>{label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              );
            })}
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
 * One server row in the sidebar. Owns its own action mutation so the
 * dropdown can start, stop and restart the server without the sidebar
 * re-rendering the whole list.
 */
function SidebarServerRow({
  instance,
  active,
  onSelect,
  onOpenConfig,
  onOpenWorld,
  onDelete,
}: {
  instance: Instance;
  active: boolean;
  onSelect: () => void;
  onOpenConfig: () => void;
  onOpenWorld: () => void;
  onDelete: () => void;
}) {
  const action = useInstanceAction(instance.id);

  return (
    <SidebarInstanceButton
      instance={instance}
      active={active}
      onClick={onSelect}
      onOpenConfig={onOpenConfig}
      onOpenWorld={onOpenWorld}
      onStart={() => action.mutate('start')}
      onStop={() => action.mutate('stop')}
      onRestart={() => action.mutate('restart')}
      onDelete={onDelete}
    />
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
