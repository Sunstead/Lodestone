import { Button } from '@sunstead/ui/components/button';
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarRail,
} from '@sunstead/ui/components/resizable-sidebar';
import {
  Container,
  FileText,
  Home,
  Puzzle,
  Settings,
  Terminal,
  UserGroup,
  type LucideIcon,
} from 'lucide-react';
import type { ReactNode } from 'react';
import { SidebarServerButton } from './sidebar-server-button';

export const SERVERS = [
  { id: 'survival', name: 'Survival', version: 'Fabric 26.3' },
  { id: 'creative', name: 'Creative', version: 'Vanilla 26.3' },
  { id: 'modded', name: 'Modded', version: 'Forge 26.2' },
] as const;

export type ServerId = (typeof SERVERS)[number]['id'];

export const SERVER_PAGES = [
  { id: 'dashboard', label: 'Dashboard', icon: Home },
  { id: 'console', label: 'Console', icon: Terminal },
  { id: 'properties', label: 'Server Properties', icon: FileText },
  { id: 'mods', label: 'Mods', icon: Puzzle },
  { id: 'players', label: 'Players', icon: UserGroup },
  { id: 'backups', label: 'Backups', icon: Container },
  { id: 'settings', label: 'Settings', icon: Settings },
] as const satisfies readonly {
  id: string;
  label: string;
  icon: LucideIcon;
}[];

export type ServerPage = (typeof SERVER_PAGES)[number]['id'];

export function isServerId(value: unknown): value is ServerId {
  return SERVERS.some((server) => server.id === value);
}

export function isServerPage(value: unknown): value is ServerPage {
  return SERVER_PAGES.some((page) => page.id === value);
}

export function ServerSidebar({
  activeServer,
  onServerSelect,
  children,
}: {
  activeServer: ServerId;
  onServerSelect: (id: ServerId) => void;
  children: ReactNode;
}) {
  return (
    <Sidebar>
      <SidebarHeader className='border-b text-xl font-semibold'>
        <div className='px-2 py-1.5'>LODESTONE</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarMenu className='gap-2 px-2'>{children}</SidebarMenu>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Servers</SidebarGroupLabel>
          <SidebarMenu className='space-y-1 px-2'>
            {SERVERS.map((server) => (
              <SidebarServerButton
                key={server.id}
                name={server.name}
                version={server.version}
                active={server.id === activeServer}
                onClick={() => onServerSelect(server.id)}
              />
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarRail />
    </Sidebar>
  );
}

export function ServerRibbon({
  activePage,
  onPageSelect,
}: {
  activePage: ServerPage;
  onPageSelect: (page: ServerPage) => void;
}) {
  return (
    <nav
      aria-label='Server pages'
      className='scrollbar-hidden flex min-w-0 flex-1 items-stretch overflow-x-auto overflow-y-hidden'
    >
      {SERVER_PAGES.map(({ id, label, icon: Icon }) => (
        <Button
          key={id}
          type='button'
          variant='ghost'
          aria-current={activePage === id ? 'page' : undefined}
          onClick={() => onPageSelect(id)}
          className={`h-full shrink-0 px-4 py-4 ${
            activePage === id
              ? 'shadow-[inset_0_-4px_0_0_var(--color-main)]'
              : ''
          }`}
        >
          <Icon aria-hidden='true' />
          <span>{label}</span>
        </Button>
      ))}
    </nav>
  );
}
