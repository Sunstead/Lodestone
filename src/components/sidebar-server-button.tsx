import { Button } from '@sunstead/ui/components/button';
import { SidebarMenuButton } from '@sunstead/ui/components/resizable-sidebar';
import { Box, MoreVertical } from 'lucide-react';

export function SidebarServerButton() {
  return (
    <SidebarMenuButton className='border-l-4 h-max py-2 max-w-full w-full'>
      <div className='flex items-center gap-x-2 w-full max-w-full'>
        <Box className='size-8 min-w-8 min-h-8 shrink-0' />
        
        <div className='w-full min-w-0'>
          <p className='text-foreground text-base truncate'>Survival</p>
          <p className='text-muted-foreground text-sm truncate'>Fabric 26.3</p>
        </div>
        
        <Button variant='ghost' size='icon' className='ml-auto shrink-0'>
          <MoreVertical />
        </Button>
      </div>
    </SidebarMenuButton>
  );
}