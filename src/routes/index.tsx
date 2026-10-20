import { SidebarServerButton } from '#/components/sidebar-server-button';
import { Avatar, AvatarFallback } from '@sunstead/ui/components/avatar';
import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Progress } from '@sunstead/ui/components/progress';
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from '@sunstead/ui/components/resizable-sidebar';
import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { Separator } from '@sunstead/ui/components/separator';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@sunstead/ui/components/table';
import { ThemeProvider } from '@sunstead/ui/theme-provider';
import { createFileRoute } from '@tanstack/react-router';
import {
  Activity,
  ArchiveRestore,
  Box,
  Clock,
  Container,
  Cpu,
  FileText,
  Gauge,
  HardDrive,
  HomeIcon,
  MemoryStick,
  Network,
  Play,
  Plus,
  Puzzle,
  Settings,
  Terminal,
  UserGroup,
  Users,
  type LucideIcon,
} from 'lucide-react';
import backgroundImg from '@/assets/wallpaper/wallpaper_minecraft_pc_bundle_2560x1440.png';

export const Route = createFileRoute('/')({ component: Home });

/* -------------------------------------------------------------------------- */
/*  Sample data (swap for real values from your server host / RCON / query)   */
/* -------------------------------------------------------------------------- */

// Deterministic fake time series so the sample renders identically every time.
const wave = (base: number, amp: number, n: number, seed: number) =>
  Array.from({ length: n }, (_, i) =>
    Math.max(
      0,
      Math.min(
        100,
        base +
          Math.sin(i / 3 + seed) * amp +
          Math.sin(i * 1.7 + seed * 2) * (amp / 3),
      ),
    ),
  );

const RAM_HISTORY = wave(62, 9, 30, 1);
const CPU_HISTORY = wave(34, 14, 30, 4);

const PLAYERS = [
  { name: 'Steve_Builds', ping: 24, playtime: '2h 14m', dimension: 'Overworld' },
  { name: 'CreeperSlayer', ping: 41, playtime: '1h 52m', dimension: 'Nether' },
  { name: 'redstone_rae', ping: 18, playtime: '1h 05m', dimension: 'Overworld' },
  { name: 'Alexandrite', ping: 77, playtime: '48m', dimension: 'The End' },
  { name: 'mossy_cobble', ping: 33, playtime: '31m', dimension: 'Overworld' },
  { name: 'DiamondDan', ping: 112, playtime: '12m', dimension: 'Overworld' },
  { name: 'pixelpilot', ping: 29, playtime: '4m', dimension: 'Nether' },
];

const CONSOLE_LINES = [
  { time: '14:32:08', level: 'INFO', text: 'Steve_Builds issued server command: /time set day' },
  { time: '14:31:40', level: 'INFO', text: 'pixelpilot joined the game' },
  { time: '14:30:12', level: 'WARN', text: "Can't keep up! Is the server overloaded? Running 2312ms behind, skipping 46 ticks" },
  { time: '14:28:55', level: 'INFO', text: 'Alexandrite has made the advancement [Free the End]' },
  { time: '14:26:03', level: 'INFO', text: 'Saving the game (this may take a moment!)' },
  { time: '14:26:04', level: 'INFO', text: 'Saved the game' },
  { time: '14:21:47', level: 'ERROR', text: 'Failed to load chunk at [-312, 88]: region file r.-10.2.mca is truncated' },
  { time: '14:19:30', level: 'INFO', text: 'DiamondDan joined the game' },
  { time: '14:12:09', level: 'INFO', text: 'CreeperSlayer was blown up by Creeper' },
];

const BACKUPS = [
  { name: 'Scheduled', when: 'Today, 12:00', size: '1.82 GB' },
  { name: 'Scheduled', when: 'Today, 06:00', size: '1.81 GB' },
  { name: 'Manual', when: 'Yesterday, 21:14', size: '1.79 GB' },
  { name: 'Scheduled', when: 'Yesterday, 18:00', size: '1.78 GB' },
];

const SLOW_MODS = [
  { name: 'Lithium', value: 0.4, label: '0.4 ms' },
  { name: 'Create', value: 6.8, label: '6.8 ms' },
  { name: 'Ferrite Core', value: 0.9, label: '0.9 ms' },
  { name: 'Farmers Delight', value: 1.7, label: '1.7 ms' },
];

/* -------------------------------------------------------------------------- */
/*  Small building blocks                                                     */
/* -------------------------------------------------------------------------- */

function Sparkline({
  series,
  height = 160,
}: {
  series: { label: string; data: number[]; stroke: string; fill?: boolean }[];
  height?: number;
}) {
  const W = 300;
  const H = 100;
  const toPoints = (data: number[]) =>
    data.map((v, i) => [(i / (data.length - 1)) * W, H - (v / 100) * H] as const);

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio='none'
      className='w-full overflow-visible'
      style={{ height }}
      role='img'
      aria-label={series.map((s) => s.label).join(' and ') + ' over the last 30 minutes'}
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
      {series.map((s) => {
        const pts = toPoints(s.data);
        const line = pts.map(([x, y], i) => `${i ? 'L' : 'M'}${x},${y}`).join(' ');
        return (
          <g key={s.label}>
            {s.fill && (
              <path
                d={`${line} L${W},${H} L0,${H} Z`}
                className={s.stroke.replace('stroke-', 'fill-')}
                fillOpacity={0.12}
              />
            )}
            <path
              d={line}
              fill='none'
              className={s.stroke}
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
  children,
}: {
  title: string;
  icon: LucideIcon;
  value: string;
  unit?: string;
  footnote?: string;
  children?: React.ReactNode;
}) {
  return (
    <Card className='gap-3 py-4'>
      <CardHeader className='flex flex-row items-center justify-between gap-2 px-4'>
        <CardTitle className='text-sm font-medium text-muted-foreground'>
          {title}
        </CardTitle>
        <Icon className='size-4 text-muted-foreground' />
      </CardHeader>
      <CardContent className='space-y-2 px-4'>
        <p className='text-3xl font-bold tabular-nums leading-none'>
          {value}
          {unit && (
            <span className='ml-1 text-base font-semibold text-muted-foreground'>
              {unit}
            </span>
          )}
        </p>
        {children}
        {footnote && <p className='text-xs text-muted-foreground'>{footnote}</p>}
      </CardContent>
    </Card>
  );
}

function KeyValue({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className='flex items-center justify-between gap-4 py-2 text-sm'>
      <span className='text-muted-foreground'>{label}</span>
      <span className='text-right font-medium tabular-nums'>{value}</span>
    </div>
  );
}

function MeterRow({
  label,
  detail,
  percent,
}: {
  label: string;
  detail: string;
  percent: number;
}) {
  return (
    <div className='space-y-1.5'>
      <div className='flex items-center justify-between text-sm'>
        <span className='font-medium'>{label}</span>
        <span className='tabular-nums text-muted-foreground'>{detail}</span>
      </div>
      <Progress value={percent} />
    </div>
  );
}

const levelStyles: Record<string, string> = {
  INFO: 'text-muted-foreground',
  WARN: 'text-yellow-500',
  ERROR: 'text-red-500',
};

const pingTone = (ms: number) =>
  ms < 50 ? 'text-green-500' : ms < 100 ? 'text-yellow-500' : 'text-red-500';

/* -------------------------------------------------------------------------- */
/*  Dashboard                                                                 */
/* -------------------------------------------------------------------------- */

function Dashboard() {
  return (
    <div className='mx-auto w-full min-w-0 max-w-[1600px] space-y-4 p-4 md:p-6'>
      {/* Headline stats */}
      <section
        aria-label='Server summary'
        className='grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4'
      >
        <StatCard
          title='Uptime'
          icon={Clock}
          value='3d 14h'
          unit='22m'
          footnote='Running since Oct 17, 06:41'
        >
          <Badge className='gap-1.5'>
            <span className='size-1.5 rounded-full bg-green-400' />
            Online
          </Badge>
        </StatCard>

        <StatCard
          title='Players online'
          icon={Users}
          value='7'
          unit='/ 20'
          footnote='Peak today: 13 players'
        >
          <Progress value={35} aria-label='Player slots used' />
        </StatCard>

        <StatCard
          title='Memory'
          icon={MemoryStick}
          value='5.2'
          unit='/ 8 GB'
          footnote='2.8 GB free in the heap'
        >
          <Progress value={65} aria-label='Memory used' />
        </StatCard>

        <StatCard
          title='Tick rate'
          icon={Gauge}
          value='19.97'
          unit='TPS'
          footnote='Average tick takes 31 ms (limit 50 ms)'
        >
          <Progress value={62} aria-label='Tick time budget used' />
        </StatCard>
      </section>

      {/* Charts + players */}
      <section
        aria-label='Performance and players'
        className='grid grid-cols-1 gap-4 xl:grid-cols-4'
      >
        <Card className='xl:col-span-2'>
          <CardHeader>
            <CardTitle>Resource usage</CardTitle>
            <CardDescription>Last 30 minutes</CardDescription>
          </CardHeader>
          <CardContent className='space-y-3'>
            <Sparkline
              series={[
                { label: 'Memory', data: RAM_HISTORY, stroke: 'stroke-(--color-main)', fill: true },
                { label: 'CPU', data: CPU_HISTORY, stroke: 'stroke-muted-foreground' },
              ]}
            />
            <div className='flex flex-wrap items-center gap-x-6 gap-y-1 text-sm'>
              <span className='flex items-center gap-2'>
                <span className='h-0.5 w-4 bg-main' />
                Memory
                <span className='tabular-nums text-muted-foreground'>65%</span>
              </span>
              <span className='flex items-center gap-2'>
                <span className='h-0.5 w-4 bg-muted-foreground' />
                CPU
                <span className='tabular-nums text-muted-foreground'>38%</span>
              </span>
            </div>
          </CardContent>
        </Card>

        <Card className='md:col-span-1 xl:col-span-2'>
          <CardHeader>
            <CardTitle>Players</CardTitle>
            <CardDescription>7 of 20 slots in use</CardDescription>
          </CardHeader>
          <CardContent className='px-2 sm:px-6'>
            <ScrollArea className='h-58 [&>[data-radix-scroll-area-viewport]>div]:block!'>
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
                  {PLAYERS.map((p) => (
                    <TableRow key={p.name}>
                      <TableCell>
                        <div className='flex items-center gap-2'>
                          <Avatar className='size-7 rounded-md'>
                            <AvatarFallback className='rounded-md text-xs'>
                              {p.name.slice(0, 2).toUpperCase()}
                            </AvatarFallback>
                          </Avatar>
                          <span className='font-medium'>{p.name}</span>
                        </div>
                      </TableCell>
                      <TableCell className='hidden text-muted-foreground sm:table-cell'>
                        {p.dimension}
                      </TableCell>
                      <TableCell className={`text-right tabular-nums ${pingTone(p.ping)}`}>
                        {p.ping} ms
                      </TableCell>
                      <TableCell className='text-right tabular-nums text-muted-foreground'>
                        {p.playtime}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </ScrollArea>
          </CardContent>
        </Card>
      </section>

      {/* Host + world + info */}
      <section
        aria-label='Host and world details'
        className='grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4'
      >
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Cpu className='size-4' /> Host
            </CardTitle>
            <CardDescription>Machine running this server</CardDescription>
          </CardHeader>
          <CardContent className='space-y-4'>
            <MeterRow label='CPU' detail='38% of 4 cores' percent={38} />
            <MeterRow label='Disk' detail='41.6 / 80 GB' percent={52} />
            <MeterRow label='Swap' detail='0.2 / 2 GB' percent={10} />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Network className='size-4' /> Network
            </CardTitle>
            <CardDescription>Traffic since last restart</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Inbound now' value='84 KB/s' />
            <KeyValue label='Outbound now' value='312 KB/s' />
            <KeyValue label='Total received' value='9.4 GB' />
            <KeyValue label='Total sent' value='38.1 GB' />
            <KeyValue label='Packets dropped' value='0.02%' />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Activity className='size-4' /> World
            </CardTitle>
            <CardDescription>Loaded right now</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Chunks' value='1,248' />
            <KeyValue label='Entities' value='3,917' />
            <KeyValue label='Block entities' value='642' />
            <KeyValue label='World size' value='6.3 GB' />
            <KeyValue label='In-game time' value='Day 412, 14:20' />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Box className='size-4' /> Server info
            </CardTitle>
            <CardDescription>Connection and rules</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Address' value='play.sunstead.gg:25565' />
            <KeyValue label='Version' value='Fabric 26.3' />
            <KeyValue label='Java' value='Temurin 21.0.4' />
            <KeyValue label='Difficulty' value='Hard' />
            <KeyValue label='Whitelist' value={<Badge variant='secondary'>On</Badge>} />
          </CardContent>
        </Card>
      </section>

      {/* Logs, backups, mods */}
      <section
        aria-label='Activity'
        className='grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-4'
      >
        <Card className='lg:col-span-2'>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Terminal className='size-4' /> Recent console output
            </CardTitle>
            <CardDescription>Latest 9 lines from latest.log</CardDescription>
          </CardHeader>
          <CardContent>
            <ScrollArea className='h-60 rounded-md border bg-muted/30 [&>[data-radix-scroll-area-viewport]>div]:block!'>
              <ul className='space-y-1 p-3 font-mono text-xs leading-relaxed'>
                {CONSOLE_LINES.map((line, i) => (
                  <li key={i} className='flex gap-2'>
                    <span className='shrink-0 text-muted-foreground'>{line.time}</span>
                    <span className={`w-11 shrink-0 font-semibold ${levelStyles[line.level]}`}>
                      {line.level}
                    </span>
                    <span className='min-w-0 wrap-break-word'>{line.text}</span>
                  </li>
                ))}
              </ul>
            </ScrollArea>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <ArchiveRestore className='size-4' /> Backups
            </CardTitle>
            <CardDescription>Next scheduled backup at 18:00</CardDescription>
          </CardHeader>
          <CardContent>
            <ul>
              {BACKUPS.map((b, i) => (
                <li key={i}>
                  {i > 0 && <Separator />}
                  <div className='flex items-center justify-between gap-2 py-2.5 text-sm'>
                    <div>
                      <p className='font-medium'>{b.name}</p>
                      <p className='text-xs text-muted-foreground'>{b.when}</p>
                    </div>
                    <span className='tabular-nums text-muted-foreground'>{b.size}</span>
                  </div>
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <HardDrive className='size-4' /> Tick time by mod
            </CardTitle>
            <CardDescription>Average per tick, 4 of 38 mods</CardDescription>
          </CardHeader>
          <CardContent className='space-y-4'>
            {SLOW_MODS.map((m) => (
              <MeterRow
                key={m.name}
                label={m.name}
                detail={m.label}
                percent={(m.value / 10) * 100}
              />
            ))}
          </CardContent>
        </Card>
      </section>
    </div>
  );
}

/* -------------------------------------------------------------------------- */
/*  Page                                                                      */
/* -------------------------------------------------------------------------- */

function Home() {
  return (
    <div className='h-dvh w-full overflow-hidden'>
      <ThemeProvider app='lodestone'>
        <SidebarProvider className='h-full min-h-0!'>
          <Sidebar>
            <SidebarHeader className='text-xl font-semibold border-b'>
              <div className='py-1.5 px-2'>LODESTONE</div>
            </SidebarHeader>
            <SidebarContent>
              <SidebarGroup>
                <SidebarMenu className='gap-2 px-2'>
                  <Button className='bevel py-6 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-main'>
                    <Plus className='size-6' />
                    Create Server
                  </Button>
                  {/* <Button className='bevel py-6 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-gray justify-start'>
                    <Folder className='size-6' /> Add Existing
                  </Button> */}
                </SidebarMenu>
              </SidebarGroup>
              <SidebarGroup>
                <SidebarGroupLabel>Servers</SidebarGroupLabel>
                <SidebarMenu className='px-2 space-y-1'>
                  <SidebarServerButton active />
                  <SidebarServerButton />
                  <SidebarServerButton />
                </SidebarMenu>
              </SidebarGroup>
            </SidebarContent>
            <SidebarRail />
          </Sidebar>
          <SidebarInset className='min-h-0! min-w-0 overflow-hidden'>
            <div className='min-h-0 flex-1 overflow-y-auto overflow-x-hidden'>
              <div
                className='min-h-64 w-full relative bg-cover bg-center flex flex-col justify-end pb-14'
                style={{
                  backgroundImage: `url(${backgroundImg})`,
                }}
              >
                <div className='absolute inset-0 bg-linear-to-r from-black/40 to-black/40 via-transparent z-0'></div>

                <div className='flex flex-wrap items-center gap-4 p-4 z-10 my-auto'>
                  <div className='flex min-w-0 items-center gap-x-4'>
                    <Box className='size-20 min-w-20 sm:size-32 sm:min-w-32' />
                    <div className='min-w-0'>
                      <p className='truncate text-4xl font-bold sm:text-6xl text-shadow-lg'>Survival</p>
                      <p className='text-xl text-muted-foreground text-shadow-lg font-semibold'>
                        Fabric 26.3
                      </p>
                    </div>
                  </div>
                  <div className='flex items-center ml-auto'>
                    <Button className='bevel py-6 px-8 text-lg font-semibold uppercase text-accent-foreground bg-(--bevel-base) hover:bg-(--bevel-base) bevel-main'>
                      <Play className='size-6' /> Start
                    </Button>
                  </div>
                </div>
              </div>

              <div className='sticky top-0 z-20 -mt-14 flex h-14 w-full items-stretch overflow-x-auto border-y bg-sidebar/80 backdrop-blur-md'>
                <SidebarTrigger className='mx-2 shrink-0 self-center' />
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <HomeIcon /> Dashboard
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Terminal /> Console
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <FileText /> Server Properties
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Puzzle /> Mods
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <UserGroup /> Players
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Container /> Backups
                </Button>
                <Button
                  variant='ghost'
                  className='p-4 py-4 h-full active:shadow-[inset_0_-4px_0_0_var(--color-main)]'
                >
                  <Settings /> Settings
                </Button>
              </div>

              <Dashboard />
            </div>
          </SidebarInset>
        </SidebarProvider>
      </ThemeProvider>
    </div>
  );
}