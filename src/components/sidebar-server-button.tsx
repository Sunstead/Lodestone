import { Button } from '@sunstead/ui/components/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@sunstead/ui/components/dropdown-menu';
import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@sunstead/ui/components/resizable-sidebar';
import { ServerStateDot } from '#/components/server-state';
import type { Instance } from '#/api/types';
import { Box, Boxes, Layers, MoreVertical, Play, RotateCw, Square, Trash2 } from 'lucide-react';

export function SidebarInstanceButton({
  instance,
  active = false,
  onClick,
  onOpenConfig,
  onOpenWorld,
  onStart,
  onStop,
  onRestart,
  onDelete,
}: {
  instance: Instance;
  active?: boolean;
  onClick: () => void;
  onOpenConfig: () => void;
  onOpenWorld: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onDelete: () => void;
}) {
  const { name, state, profile, world } = instance;
  const running = state === 'running';
  const transitioning = state === 'starting' || state === 'stopping';

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
            <p className='truncate text-sm text-muted-foreground'>
              {profile.software} {profile.version}
            </p>
          </div>
          <ServerStateDot state={state} className='size-2.5' />
        </div>
      </SidebarMenuButton>

      <DropdownMenu>
        <SidebarMenuAction
          className='top-0! bottom-0! my-auto'
          render={
            <DropdownMenuTrigger
              render={
                <Button
                  variant='ghost'
                  size='icon'
                  aria-label={`${name} actions`}
                  className='size-8! shrink-0'
                >
                  <MoreVertical />
                </Button>
              }
            />
          }
        />
        <DropdownMenuContent align='end' side='bottom' className='w-52'>
          {running || transitioning ? (
            <>
              <DropdownMenuItem disabled={transitioning} onClick={onRestart}>
                <RotateCw />
                Restart
              </DropdownMenuItem>
              <DropdownMenuItem disabled={transitioning} variant='destructive' onClick={onStop}>
                <Square />
                Stop
              </DropdownMenuItem>
            </>
          ) : (
            <DropdownMenuItem onClick={onStart}>
              <Play />
              Start
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={onOpenConfig}>
            <Boxes />
            Configuration
            <span className='ml-auto truncate text-xs text-muted-foreground'>{profile.name}</span>
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onOpenWorld}>
            <Layers />
            World
            <span className='ml-auto truncate text-xs text-muted-foreground'>{world.name}</span>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem variant='destructive' onClick={onDelete}>
            <Trash2 />
            Delete server
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  );
}
