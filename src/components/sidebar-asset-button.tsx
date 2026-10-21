import {
  SidebarMenuButton,
  SidebarMenuItem,
} from '@sunstead/ui/components/resizable-sidebar';
import { cn } from '@sunstead/ui/utils';
import { Box, Layers } from 'lucide-react';

/**
 * A profile or a world in the sidebar. Both are first-class assets that
 * outlive any instance, so the row shows the attachment badge instead of
 * a container state dot.
 */
export function SidebarAssetButton({
  kind,
  name,
  version,
  badge,
  active = false,
  onClick,
}: {
  kind: 'profile' | 'world';
  name: string;
  version: string;
  badge: { label: string; tone: 'main' | 'muted' };
  active?: boolean;
  onClick: () => void;
}) {
  const Icon = kind === 'world' ? Layers : Box;

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        onClick={onClick}
        aria-current={active ? 'page' : undefined}
        className={`h-max w-full max-w-full border-l-4 py-2 ${
          active ? 'border-l-main bg-sidebar-accent' : 'border-l-border'
        }`}
      >
        <div className='flex w-full min-w-0 items-center gap-2'>
          <Icon aria-hidden='true' className='size-8 min-h-8 min-w-8 shrink-0 text-muted-foreground' />
          <div className='min-w-0 flex-1'>
            <p className='truncate text-base text-foreground'>{name}</p>
            <p className='truncate text-sm text-muted-foreground'>{version}</p>
          </div>
          <span
            className={cn(
              'shrink-0 rounded-sm px-1.5 py-0.5 text-[10px] font-semibold tracking-wide uppercase',
              badge.tone === 'main' ? 'bg-main/15 text-main' : 'bg-muted text-muted-foreground',
            )}
          >
            {badge.label}
          </span>
        </div>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}
