import { Button } from '@sunstead/ui/components/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@sunstead/ui/components/tooltip';
import { cn } from '@sunstead/ui/utils';
import { ServerStateBadge } from '#/components/server-state';
import { formatBytes, formatNumber, percent } from '#/lib/format';
import { Box, Cpu, Layers, MemoryStick, Users } from 'lucide-react';
import type { Instance, InstanceStatus } from '#/api/types';

/**
 * Bevel button that carries the Lodestone/Minecraft look from the hero
 * into the rest of the app. `tone` picks the flat colour underneath.
 */
const TONE: Record<'main' | 'gray' | 'danger', string> = {
  main: 'bevel-main',
  gray: 'bevel-gray',
  danger: 'bevel-danger',
};

export function BevelButton({
  tone = 'main',
  className,
  ...props
}: React.ComponentProps<typeof Button> & { tone?: 'main' | 'gray' | 'danger' }) {
  return (
    <Button
      className={cn(
        'bevel bg-(--bevel-base) font-semibold uppercase text-accent-foreground hover:bg-(--bevel-base)',
        TONE[tone],
        className,
      )}
      {...props}
    />
  );
}

function Meter({ value, label }: { value: number; label: string }) {
  return (
    <div className='space-y-1'>
      <div className='h-1.5 w-full overflow-hidden rounded-full bg-muted'>
        <div
          role='progressbar'
          aria-label={label}
          aria-valuenow={Math.round(value)}
          aria-valuemin={0}
          aria-valuemax={100}
          className='h-full bg-primary transition-[width] duration-500'
          style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
        />
      </div>
    </div>
  );
}

/**
 * Live instance header: wallpaper hero plus a status strip that carries
 * the numbers an admin actually watches. Everything is read from
 * `/instances/:id/status`, and the subtitle names the profile and world
 * the container is made of.
 */
export function ServerHero({
  instance,
  status,
  playersOnline,
  busy,
  backgroundImage,
  onStart,
  onStop,
  onRestart,
  onOpenProfile,
  onOpenWorld,
  children,
}: {
  instance: Instance;
  status?: InstanceStatus;
  playersOnline?: number;
  busy?: boolean;
  backgroundImage: string;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onOpenProfile: () => void;
  onOpenWorld: () => void;
  children?: React.ReactNode;
}) {
  const state = status?.state ?? instance.state;
  const online = playersOnline ?? status?.players.online ?? instance.players.online;
  const max = status?.players.max ?? instance.players.max;
  const running = state === 'running';
  const transitioning = state === 'starting' || state === 'stopping';

  const metrics = [
    {
      label: 'Players',
      icon: Users,
      value: `${online}/${max}`,
      meter: percent(online, max),
    },
    {
      label: 'Memory',
      icon: MemoryStick,
      value: status ? formatBytes(status.memory.usedBytes, 1) : '—',
      hint: status ? `of ${formatBytes(status.memory.limitBytes, 0)}` : undefined,
      meter: status ? percent(status.memory.usedBytes, status.memory.limitBytes) : 0,
    },
    {
      label: 'CPU',
      icon: Cpu,
      value: status ? `${Math.round(status.cpuPercent)}%` : '—',
      meter: status?.cpuPercent ?? 0,
    },
    {
      label: 'Tick rate',
      icon: Box,
      value: status && running ? status.tps.toFixed(2) : '—',
      hint: status && running ? `${formatNumber(status.tickMs)} ms per tick` : undefined,
      meter: status ? Math.min(100, (status.tickMs / 50) * 100) : 0,
    },
  ];

  return (
    <header
      className='relative flex w-full flex-col justify-end bg-cover bg-center'
      style={{ backgroundImage: `url(${backgroundImage})` }}
    >
      <div
        aria-hidden='true'
        className='absolute inset-0 z-0 bg-linear-to-r from-black/45 via-background/10 to-black/45'
      />

      <div className='relative z-10 flex flex-wrap items-center gap-4 p-4 py-10'>
        <div className='flex min-w-0 items-center gap-4'>
          <Box aria-hidden='true' className='size-16 shrink-0 text-foreground drop-shadow-lg sm:size-24' />
          <div className='min-w-0'>
            <div className='flex flex-wrap items-center gap-3'>
              <h1 className='truncate text-3xl font-bold text-foreground text-shadow-lg sm:text-5xl'>{instance.name}</h1>
              <ServerStateBadge state={state} className='bg-accent/60 text-foreground ring-1 ring-foreground/15 backdrop-blur-lg' />
            </div>
            <div className='mt-2 flex flex-wrap items-center gap-2'>
              <button
                type='button'
                onClick={onOpenProfile}
                className='flex items-center gap-1.5 rounded-sm bg-accent/50 px-2 py-1 text-sm font-semibold text-foreground text-shadow-lg ring-1 ring-foreground/15 backdrop-blur-lg transition-colors hover:bg-accent/70'
                title='Open this profile'
              >
                <Box aria-hidden='true' className='size-3.5' />
                {instance.profile.name}
              </button>
              <span aria-hidden='true' className='text-foreground/50'>
                ·
              </span>
              <button
                type='button'
                onClick={onOpenWorld}
                className='flex items-center gap-1.5 rounded-sm bg-accent/50 px-2 py-1 text-sm font-semibold text-foreground text-shadow-lg ring-1 ring-foreground/15 backdrop-blur-lg transition-colors hover:bg-accent/70'
                title='Open this world'
              >
                <Layers aria-hidden='true' className='size-3.5' />
                {instance.world.name}
              </button>
              <span className='text-sm font-semibold text-foreground/70 text-shadow-lg'>{instance.address}</span>
            </div>
          </div>
        </div>

        <div className='ml-auto flex flex-wrap items-center gap-2'>
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger
                render={
                  <BevelButton
                    tone='gray'
                    disabled={busy || transitioning || !running}
                    onClick={onRestart}
                    className='px-5 py-5 text-base'
                  >
                    Restart
                  </BevelButton>
                }
              />
              <TooltipContent>Graceful stop, then start again</TooltipContent>
            </Tooltip>
          </TooltipProvider>

          {running || transitioning ? (
            <BevelButton
              tone='danger'
              disabled={busy || state === 'stopping'}
              onClick={onStop}
              className='px-6 py-5 text-base'
            >
              Stop
            </BevelButton>
          ) : (
            <BevelButton
              disabled={busy}
              onClick={onStart}
              className='px-8 py-5 text-base'
            >
              Start
            </BevelButton>
          )}
          {children}
        </div>
      </div>

      <div className='relative z-10 grid grid-cols-2 gap-px border-t border-border bg-sidebar/50 backdrop-blur-lg md:grid-cols-4'>
        {metrics.map((metric) => (
          <div key={metric.label} className='flex items-center gap-3 px-4 py-3'>
            <metric.icon aria-hidden='true' className='size-5 shrink-0 text-foreground/60' />
            <div className='min-w-0 flex-1'>
              <p className='text-xs font-medium tracking-wide text-foreground/60 uppercase'>{metric.label}</p>
              <p className='text-lg font-semibold tabular-nums text-foreground'>
                {metric.value}
                {metric.hint ? <span className='ml-2 text-xs font-normal text-foreground/50'>{metric.hint}</span> : null}
              </p>
              <div className='mt-1 opacity-80'>
                <Meter value={metric.meter} label={`${metric.label} usage`} />
              </div>
            </div>
          </div>
        ))}
      </div>
    </header>
  );
}
