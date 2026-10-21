import { Avatar, AvatarFallback } from '@sunstead/ui/components/avatar';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Progress } from '@sunstead/ui/components/progress';
import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { Skeleton } from '@sunstead/ui/components/skeleton';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@sunstead/ui/components/table';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@sunstead/ui/components/tooltip';
import { useConsoleTail, useInstanceAction, useInstanceStatus, usePlayers } from '#/api/queries';
import type { ConsoleLine, InstanceStatus } from '#/api/types';
import { KeyValue, SectionHeading } from '#/components/page-parts';
import { ServerStateBadge } from '#/components/server-state';
import {
  formatBytes,
  formatClock,
  formatDuration,
  formatNumber,
  formatRelative,
  percent,
  pingTone,
} from '#/lib/format';
import {
  Activity,
  Clock,
  Cpu,
  Gauge,
  HardDrive,
  Layers,
  MemoryStick,
  Network,
  Package,
  RefreshCw,
  Save,
  Square,
  Terminal,
  Users,
  type LucideIcon,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

const LEVEL_TONE: Record<string, string> = {
  DEBUG: 'text-sky-400',
  INFO: 'text-muted-foreground',
  WARN: 'text-yellow-500',
  ERROR: 'text-red-500',
};

/** Rolling window of samples for the sparkline, filled by live polling. */
function useHistory(value: number | undefined, length = 30): number[] {
  const [samples, setSamples] = useState<number[]>(() => Array.from({ length }, () => 0));
  const previous = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (value === undefined) return;
    if (previous.current === value) return;
    previous.current = value;
    setSamples((current) => [...current.slice(1), value]);
  }, [value]);
  return samples;
}

function Sparkline({
  series,
  height = 150,
}: {
  series: { label: string; data: number[]; tone: 'main' | 'muted'; scale?: number }[];
  height?: number;
}) {
  const W = 300;
  const H = 100;
  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio='none'
      className='w-full overflow-visible'
      style={{ height }}
      role='img'
      aria-label={`${series.map((entry) => entry.label).join(' and ')} over the last 30 samples`}
    >
      {[25, 50, 75].map((y) => (
        <line
          key={y}
          x1={0}
          x2={W}
          y1={H - y}
          y2={H - y}
          className='stroke-border'
          strokeWidth={1}
          strokeDasharray='3 4'
          vectorEffect='non-scaling-stroke'
        />
      ))}
      {series.map((entry) => {
        const scale = entry.scale ?? 100;
        const points = entry.data.map(
          (value, index) =>
            [
              (index / Math.max(1, entry.data.length - 1)) * W,
              H - Math.min(100, Math.max(0, (value / scale) * 100)) * (H / 100),
            ] as const,
        );
        const line = points.map(([x, y], index) => `${index ? 'L' : 'M'}${x},${y}`).join(' ');
        return (
          <g key={entry.label}>
            {entry.tone === 'main' ? (
              <path d={`${line} L${W},${H} L0,${H} Z`} className='fill-main' fillOpacity={0.15} />
            ) : null}
            <path
              d={line}
              fill='none'
              className={entry.tone === 'main' ? 'stroke-main' : 'stroke-muted-foreground'}
              strokeWidth={2}
              strokeLinejoin='round'
              strokeLinecap='round'
              vectorEffect='non-scaling-stroke'
            />
          </g>
        );
      })}
    </svg>
  );
}

function StatCard({
  title,
  icon: Icon,
  value,
  unit,
  footnote,
  percentValue,
  meterLabel,
}: {
  title: string;
  icon: LucideIcon;
  value: string;
  unit?: string;
  footnote?: React.ReactNode;
  percentValue?: number;
  meterLabel?: string;
}) {
  return (
    <Card className='gap-3 py-4'>
      <CardHeader className='flex flex-row items-center justify-between gap-2 px-4'>
        <CardTitle className='text-sm font-medium text-muted-foreground'>{title}</CardTitle>
        <Icon className='size-4 text-muted-foreground' />
      </CardHeader>
      <CardContent className='space-y-2 px-4'>
        <p className='text-3xl font-bold leading-none tabular-nums'>
          {value}
          {unit ? <span className='ml-1 text-base font-semibold text-muted-foreground'>{unit}</span> : null}
        </p>
        {percentValue !== undefined ? (
          <Progress value={percentValue} aria-label={meterLabel ?? title} />
        ) : null}
        {footnote ? <div className='text-xs text-muted-foreground'>{footnote}</div> : null}
      </CardContent>
    </Card>
  );
}

function MeterRow({ label, detail, percent: value }: { label: string; detail: string; percent: number }) {
  return (
    <div className='space-y-1.5'>
      <div className='flex items-center justify-between text-sm'>
        <span className='font-medium'>{label}</span>
        <span className='text-muted-foreground tabular-nums'>{detail}</span>
      </div>
      <Progress value={value} aria-label={`${label} usage`} />
    </div>
  );
}

const tpsTone = (tps: number) =>
  tps >= 19.5 ? 'text-green-500' : tps >= 18 ? 'text-yellow-500' : 'text-red-500';

export function DashboardPage({
  instanceId,
  status,
  playersOnline,
}: {
  instanceId: string;
  status?: InstanceStatus;
  playersOnline: number;
}) {
  const statusQuery = useInstanceStatus(instanceId);
  const players = usePlayers(instanceId);
  const consoleTail = useConsoleTail(instanceId);
  const action = useInstanceAction(instanceId);
  const [latest, setLatest] = useState<ConsoleLine[]>([]);

  useEffect(() => {
    if (consoleTail.data) setLatest(consoleTail.data.slice(-9));
  }, [consoleTail.data]);

  const live = status ?? statusQuery.data;
  const memoryHistory = useHistory(live?.memory.usedBytes ? percent(live.memory.usedBytes, live.memory.limitBytes) : undefined);
  const cpuHistory = useHistory(live ? live.cpuPercent : undefined);

  if (!live) {
    return (
      <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
        <div className='grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4'>
          {Array.from({ length: 4 }).map((_, index) => (
            <Skeleton key={index} className='h-36 w-full' />
          ))}
        </div>
        <Skeleton className='h-72 w-full' />
      </div>
    );
  }

  const onlinePlayers = players.data?.players.filter((player) => player.online) ?? [];
  const running = live.state === 'running';

  return (
    <div className='mx-auto w-full min-w-0 max-w-[1600px] space-y-4 p-4 md:p-6'>
      <SectionHeading
        title='Overview'
        description={`Snapshot of ${live.container.name} right now`}
        icon={Activity}
        action={
          <>
            <Button variant='outline' size='sm' disabled={!running || action.isPending} onClick={() => action.mutate('restart')}>
              <RefreshCw />
              Restart
            </Button>
            <Button
              variant='outline'
              size='sm'
              disabled={!running || action.isPending}
              onClick={() => action.mutate('stop')}
            >
              <Square />
              Stop
            </Button>
          </>
        }
      />

      <section aria-label='Instance summary' className='grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4'>
        <StatCard
          title='Uptime'
          icon={Clock}
          value={live.uptimeSeconds != null ? formatDuration(live.uptimeSeconds) : '—'}
          footnote={
            live.startedAt ? (
              <>Running since {formatRelative(live.startedAt)}</>
            ) : (
              <ServerStateBadge state={live.state} />
            )
          }
        />
        <StatCard
          title='Players online'
          icon={Users}
          value={String(playersOnline)}
          unit={`/ ${live.players.max}`}
          percentValue={percent(playersOnline, live.players.max)}
          meterLabel='Player slots used'
          footnote={players.data ? `${players.data.players.length} known players` : undefined}
        />
        <StatCard
          title='Memory'
          icon={MemoryStick}
          value={formatBytes(live.memory.usedBytes, 1)}
          unit={`/ ${formatBytes(live.memory.limitBytes, 0)}`}
          percentValue={percent(live.memory.usedBytes, live.memory.limitBytes)}
          meterLabel='Memory used'
          footnote={`${formatBytes(Math.max(0, live.memory.limitBytes - live.memory.usedBytes), 1)} headroom`}
        />
        <StatCard
          title='Tick rate'
          icon={Gauge}
          value={running ? live.tps.toFixed(2) : '—'}
          unit='TPS'
          percentValue={running ? Math.min(100, (live.tickMs / 50) * 100) : 0}
          meterLabel='Tick time budget used'
          footnote={running ? `Average tick ${formatNumber(live.tickMs)} ms (budget 50 ms)` : 'Instance not running'}
        />
      </section>

      <section aria-label='Performance and players' className='grid grid-cols-1 gap-4 xl:grid-cols-4'>
        <Card className='xl:col-span-2'>
          <CardHeader>
            <CardTitle>Resource usage</CardTitle>
            <CardDescription>Live samples from the container, one per poll</CardDescription>
          </CardHeader>
          <CardContent className='space-y-3'>
            <Sparkline
              series={[
                { label: 'Memory', data: memoryHistory, tone: 'main' },
                { label: 'CPU', data: cpuHistory, tone: 'muted' },
              ]}
            />
            <div className='flex flex-wrap items-center gap-x-6 gap-y-1 text-sm'>
              <span className='flex items-center gap-2'>
                <span className='h-0.5 w-4 bg-main' />
                Memory
                <span className='text-muted-foreground tabular-nums'>
                  {Math.round(percent(live.memory.usedBytes, live.memory.limitBytes))}%
                </span>
              </span>
              <span className='flex items-center gap-2'>
                <span className='h-0.5 w-4 bg-muted-foreground' />
                CPU
                <span className='text-muted-foreground tabular-nums'>{Math.round(live.cpuPercent)}%</span>
              </span>
            </div>
          </CardContent>
        </Card>

        <Card className='md:col-span-1 xl:col-span-2'>
          <CardHeader>
            <CardTitle>Players</CardTitle>
            <CardDescription>
              {onlinePlayers.length === 0 ? 'Nobody online right now' : `${onlinePlayers.length} online now`}
            </CardDescription>
          </CardHeader>
          <CardContent className='px-2 sm:px-6'>
            <ScrollArea className='h-58 [&>[data-slot=scroll-area-viewport]>div]:block!'>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Player</TableHead>
                    <TableHead className='hidden sm:table-cell'>Location</TableHead>
                    <TableHead className='text-right'>Ping</TableHead>
                    <TableHead className='text-right'>Session</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {onlinePlayers.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={4} className='py-8 text-center text-sm text-muted-foreground'>
                        {running ? 'Waiting for someone to join…' : 'Start the instance to see players here.'}
                      </TableCell>
                    </TableRow>
                  ) : (
                    onlinePlayers.map((player) => (
                      <TableRow key={player.uuid}>
                        <TableCell>
                          <div className='flex items-center gap-2'>
                            <Avatar className='size-7 rounded-md'>
                              <AvatarFallback className='rounded-md text-xs'>
                                {player.name.slice(0, 2).toUpperCase()}
                              </AvatarFallback>
                            </Avatar>
                            <span className='font-medium'>{player.name}</span>
                          </div>
                        </TableCell>
                        <TableCell className='hidden text-muted-foreground capitalize sm:table-cell'>
                          {player.dimension ?? '—'}
                        </TableCell>
                        <TableCell className={`text-right tabular-nums ${pingTone(player.pingMs)}`}>
                          {player.pingMs === null ? '—' : `${player.pingMs} ms`}
                        </TableCell>
                        <TableCell className='text-right text-muted-foreground tabular-nums'>
                          {formatDuration(player.playtimeSeconds)}
                        </TableCell>
                      </TableRow>
                    ))
                  )}
                </TableBody>
              </Table>
            </ScrollArea>
          </CardContent>
        </Card>
      </section>

      <section aria-label='Host, network and composition' className='grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3'>
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Cpu className='size-4' /> Host
            </CardTitle>
            <CardDescription>Machine running this container</CardDescription>
          </CardHeader>
          <CardContent className='space-y-4'>
            <MeterRow
              label='CPU'
              detail={`${Math.round(live.host.cpuPercent)}% of ${live.host.cpuCores} cores`}
              percent={live.host.cpuPercent}
            />
            <MeterRow
              label='Disk'
              detail={`${formatBytes(live.host.diskUsedBytes, 1)} / ${formatBytes(live.host.diskTotalBytes, 0)}`}
              percent={percent(live.host.diskUsedBytes, live.host.diskTotalBytes)}
            />
            <MeterRow
              label='Swap'
              detail={`${formatBytes(live.host.swapUsedBytes, 1)} / ${formatBytes(live.host.swapTotalBytes, 0)}`}
              percent={percent(live.host.swapUsedBytes, live.host.swapTotalBytes)}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Network className='size-4' /> Network
            </CardTitle>
            <CardDescription>Traffic since the container started</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Inbound now' value={`${formatBytes(live.network.rxPerSec, 0)}/s`} />
            <KeyValue label='Outbound now' value={`${formatBytes(live.network.txPerSec, 0)}/s`} />
            <KeyValue label='Total received' value={formatBytes(live.network.rxTotal, 1)} />
            <KeyValue label='Total sent' value={formatBytes(live.network.txTotal, 1)} />
            <KeyValue label='Packets dropped' value={`${live.network.dropped.toFixed(2)}%`} />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Layers className='size-4' /> Composition
            </CardTitle>
            <CardDescription>What this container is built from</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Image' value={<code className='text-xs'>{live.container.image}</code>} />
            <KeyValue label='Container' value={live.container.name} />
            <KeyValue label='Health check' value={live.container.healthy ? 'passing' : 'not running'} />
            <KeyValue
              label='World data'
              value={
                <span className='text-xs text-muted-foreground'>
                  Mounted from the world asset — open it from the sidebar
                </span>
              }
            />
          </CardContent>
        </Card>
      </section>

      <section aria-label='Activity' className='grid grid-cols-1 gap-4 lg:grid-cols-3'>
        <Card className='lg:col-span-2'>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Terminal className='size-4' /> Recent console output
            </CardTitle>
            <CardDescription>Latest lines — open the Console tab for the live stream</CardDescription>
          </CardHeader>
          <CardContent>
            <ScrollArea className='h-60 rounded-md border bg-background/85 [&>[data-slot=scroll-area-viewport]>div]:block!'>
              <ul className='space-y-1 p-3 font-mono text-xs leading-relaxed text-muted-foreground'>
                {latest.length === 0 ? (
                  <li className='text-muted-foreground'>Waiting for output…</li>
                ) : (
                  latest.map((line) => (
                    <li key={line.id} className='flex gap-2'>
                      <span className='shrink-0 text-muted-foreground/70 tabular-nums'>{formatClock(line.ts)}</span>
                      <span className={`w-11 shrink-0 font-semibold ${LEVEL_TONE[line.level]}`}>{line.level}</span>
                      <span className='min-w-0 wrap-break-word'>{line.message}</span>
                    </li>
                  ))
                )}
              </ul>
            </ScrollArea>
          </CardContent>
        </Card>

        <div className='space-y-4'>
          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <HardDrive className='size-4' /> Container disk
              </CardTitle>
              <CardDescription>Image, logs and everything outside the world</CardDescription>
            </CardHeader>
            <CardContent className='space-y-4'>
              <MeterRow
                label='Used by this container'
                detail={`${formatBytes(live.disk.usedBytes, 1)} / ${formatBytes(live.disk.totalBytes, 0)}`}
                percent={percent(live.disk.usedBytes, live.disk.totalBytes)}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <Package className='size-4' /> Quick actions
              </CardTitle>
              <CardDescription>Runs against the container directly</CardDescription>
            </CardHeader>
            <CardContent className='space-y-2'>
              <Button
                variant='outline'
                className='w-full justify-start'
                disabled={action.isPending}
                onClick={() => action.mutate('restart')}
              >
                <RefreshCw />
                Restart instance
              </Button>
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Button variant='outline' className='w-full justify-start' disabled={!running} onClick={() => action.mutate('stop')}>
                        <Save />
                        Save and stop
                      </Button>
                    }
                  />
                  <TooltipContent>The container receives SIGTERM. Minecraft saves players and chunks first, and the world is left on disk.</TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <p className={`flex items-center gap-2 pt-1 text-xs ${tpsTone(live.tps)}`}>
                <Activity className='size-3.5' />
                {running ? `Tick rate ${live.tps.toFixed(2)} across this instance` : 'Instance offline'}
              </p>
            </CardContent>
          </Card>
        </div>
      </section>
    </div>
  );
}
