import { SidebarMenuButton } from '@sunstead/ui/components/resizable-sidebar';
import { Box } from 'lucide-react';

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
    <SidebarMenuButton
      onClick={onClick}
      aria-current={active ? 'page' : undefined}
      className={`h-max w-full max-w-full border-l-4 py-2 ${
        active ? 'border-l-main bg-sidebar-accent' : 'border-l-border'
      }`}
    >
      <div className='flex w-full min-w-0 items-center gap-2'>
        <Box aria-hidden='true' className='size-8 shrink-0 min-w-8 min-h-8' />
        <div className='min-w-0 flex-1'>
          <p className='truncate text-base text-foreground'>{name}</p>
          <p className='truncate text-sm text-muted-foreground'>{version}</p>
        </div>
        <span
          aria-label='Online'
          className='size-2 shrink-0 rounded-full bg-green-500'
        />
      </div>
    </SidebarMenuButton>
  );
}
