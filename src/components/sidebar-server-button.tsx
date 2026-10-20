import { Button } from '@sunstead/ui/components/button';
import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@sunstead/ui/components/resizable-sidebar';
import { Box, MoreVertical } from 'lucide-react';

export function SidebarServerButton({
  name,
  version,
  active = false,
  onClick,
}: {
  name: string;
  version: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        onClick={onClick}
        aria-current={active ? 'page' : undefined}
        className={`h-max w-full max-w-full border-l-4 py-2 group-has-data-[sidebar=menu-action]/menu-item:pr-12 ${
          active ? 'border-l-main bg-sidebar-accent' : 'border-l-border'
        }`}
      >
        <div className='flex w-full min-w-0 items-center gap-2'>
          <Box aria-hidden='true' className='size-8 min-h-8 min-w-8 shrink-0' />
          <div className='min-w-0 flex-1'>
            <p className='truncate text-base text-foreground'>{name}</p>
            <p className='truncate text-sm text-muted-foreground'>{version}</p>
          </div>
          <span
            role='img'
            aria-label='Online'
            className='size-2 shrink-0 rounded-full bg-green-500'
          />
        </div>
      </SidebarMenuButton>
      <SidebarMenuAction
        className='top-0! bottom-0! my-auto'
        render={
          <Button
            variant='ghost'
            size='icon'
            aria-label={`${name} options`}
            className='size-8! shrink-0'
          >
            <MoreVertical />
          </Button>
        }
      />
    </SidebarMenuItem>
  );
}
