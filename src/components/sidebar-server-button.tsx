import { Button } from '@sunstead/ui/components/button';
import { SidebarMenuButton } from '@sunstead/ui/components/resizable-sidebar';
import { cn } from '@sunstead/ui/utils';
import { Box, MoreVertical } from 'lucide-react';

export function SidebarServerButton({ active = false }: { active?: boolean }) {
  return (
    <SidebarMenuButton
      className={cn(
        'border-l-4 h-max py-2 max-w-full w-full',
        active ? 'border-l-main bg-sidebar-accent' : 'border-l-border',
      )}
    >
      <div className='flex items-center gap-x-2 w-full max-w-full'>
        <Box className='size-8 min-w-8 min-h-8 shrink-0' />

        <div className='w-full min-w-0'>
          <p className='text-foreground text-base truncate'>Survival</p>
          <p className='text-muted-foreground text-sm truncate'>Fabric 26.3</p>
        </div>

        <div className='flex items-center ml-auto gap-x-2'>
          <span className='size-2 bg-green-500 rounded-full' />
          <Button variant='ghost' size='icon' className='shrink-0'>
            <MoreVertical />
          </Button>
        </div>
      </div>
    </SidebarMenuButton>
  );
}
