import { Badge } from '@sunstead/ui/components/badge';
import { cn } from '@sunstead/ui/utils';
import type { ServerState } from '#/api/types';

const STATE_META: Record<
  ServerState,
  { label: string; dot: string; text: string; pulse?: boolean }
> = {
  running: { label: 'Online', dot: 'bg-green-500', text: 'text-green-500' },
  starting: { label: 'Starting', dot: 'bg-yellow-500', text: 'text-yellow-500', pulse: true },
  stopping: { label: 'Stopping', dot: 'bg-yellow-500', text: 'text-yellow-500', pulse: true },
  stopped: { label: 'Offline', dot: 'bg-muted-foreground', text: 'text-muted-foreground' },
  error: { label: 'Crashed', dot: 'bg-red-500', text: 'text-red-500' },
};

export function ServerStateDot({ state, className }: { state: ServerState; className?: string }) {
  const meta = STATE_META[state];
  return (
    <span
      role='img'
      aria-label={meta.label}
      title={meta.label}
      className={cn('size-2 shrink-0 rounded-full', meta.dot, meta.pulse && 'animate-pulse', className)}
    />
  );
}

export function ServerStateBadge({ state, className }: { state: ServerState; className?: string }) {
  const meta = STATE_META[state];
  return (
    <Badge className={cn('gap-1.5 font-medium', className)}>
      <ServerStateDot state={state} />
      {meta.label}
    </Badge>
  );
}

export const serverStateLabel = (state: ServerState): string => STATE_META[state].label;
export const serverStateTone = (state: ServerState): string => STATE_META[state].text;
