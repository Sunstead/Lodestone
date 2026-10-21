import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Input } from '@sunstead/ui/components/input';
import { Progress } from '@sunstead/ui/components/progress';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@sunstead/ui/components/select';
import { Separator } from '@sunstead/ui/components/separator';
import { Skeleton } from '@sunstead/ui/components/skeleton';
import { Switch } from '@sunstead/ui/components/switch';
import { useInstances, useUpdateWorld, useWorld, useWorldStatus } from '#/api/queries';
import type { Difficulty, GameMode, LevelType, World } from '#/api/types';
import { KeyValue, SectionHeading, StatStrip } from '#/components/page-parts';
import { BackupsPanel } from '#/routes/-pages/backups-panel';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatNumber, formatRelative } from '#/lib/format';
import {
  Blocks,
  Container,
  Database,
  Gamepad2,
  Globe,
  Lock,
  Play,
  Save,
  Sparkles,
  TriangleAlert,
  Undo2,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';

const LEVEL_TYPES: { value: LevelType; label: string }[] = [
  { value: 'minecraft:normal', label: 'Normal' },
  { value: 'minecraft:flat', label: 'Superflat' },
  { value: 'minecraft:large_biomes', label: 'Large biomes' },
  { value: 'minecraft:amplified', label: 'Amplified' },
  { value: 'minecraft:single_biome_surface', label: 'Single biome' },
];

const DIFFICULTIES: { value: Difficulty; label: string }[] = [
  { value: 'peaceful', label: 'Peaceful' },
  { value: 'easy', label: 'Easy' },
  { value: 'normal', label: 'Normal' },
  { value: 'hard', label: 'Hard' },
];

const GAMEMODES: { value: GameMode; label: string }[] = [
  { value: 'survival', label: 'Survival' },
  { value: 'creative', label: 'Creative' },
  { value: 'adventure', label: 'Adventure' },
  { value: 'spectator', label: 'Spectator' },
];

/** Gameplay rules that belong to the world and travel with it between instances. */
type GameplayDraft = Pick<
  World,
  | 'name'
  | 'difficulty'
  | 'gamemode'
  | 'hardcore'
  | 'spawnProtection'
  | 'spawnMonsters'
  | 'spawnNpcs'
  | 'pvp'
  | 'allowFlight'
  | 'enableCommandBlock'
  | 'allowNether'
  | 'maxWorldSize'
>;

function gameplayOf(world: World): GameplayDraft {
  return {
    name: world.name,
    difficulty: world.difficulty,
    gamemode: world.gamemode,
    hardcore: world.hardcore,
    spawnProtection: world.spawnProtection,
    spawnMonsters: world.spawnMonsters,
    spawnNpcs: world.spawnNpcs,
    pvp: world.pvp,
    allowFlight: world.allowFlight,
    enableCommandBlock: world.enableCommandBlock,
    allowNether: world.allowNether,
    maxWorldSize: world.maxWorldSize,
  };
}

function ToggleRow({
  id,
  label,
  description,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <div className='flex items-center justify-between gap-4 px-4 py-3.5'>
      <div className='min-w-0'>
        <label htmlFor={id} className='font-medium'>
          {label}
        </label>
        <p className='mt-0.5 text-xs text-muted-foreground'>{description}</p>
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

export function WorldDetailPage({
  worldId,
  page,
  onOpenInstance,
  onLaunch,
}: {
  worldId: string;
  page: string;
  onOpenInstance?: (instanceId: string) => void;
  onLaunch?: () => void;
}) {
  const world = useWorld(worldId);
  const status = useWorldStatus(worldId);
  const instances = useInstances();
  const update = useUpdateWorld(worldId);
  const [draft, setDraft] = useState<GameplayDraft | null>(null);

  const serverName = useMemo(() => {
    const data = world.data;
    if (!data?.attachedInstanceId) return null;
    return (instances.data ?? []).find((entry) => entry.id === data.attachedInstanceId)?.name ?? null;
  }, [world.data, instances.data]);

  useEffect(() => {
    setDraft(null);
  }, [worldId]);

  const data = world.data;
  const base = useMemo(() => (data ? gameplayOf(data) : null), [data]);
  const current = draft ?? base;

  const changes = useMemo(() => {
    if (!current || !base) return {};
    const result: Partial<GameplayDraft> = {};
    for (const key of Object.keys(current) as (keyof GameplayDraft)[]) {
      if (current[key] !== base[key]) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (result as any)[key] = current[key];
      }
    }
    return result;
  }, [current, base]);

  const dirtyCount = Object.keys(changes).length;

  const patch = (next: Partial<GameplayDraft>) => {
    setDraft((previous) => ({ ...(previous ?? base!), ...next }));
  };

  if (world.isLoading || !data || !current) {
    return (
      <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
        <Skeleton className='h-28 w-full' />
        <Skeleton className='h-96 w-full' />
      </div>
    );
  }

  // Backups are owned entirely by the world; that page renders its own chrome.
  if (page === 'backups') {
    return (
      <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
        <SectionHeading
          title='Backups'
          icon={Container}
          description={
            <span className='flex flex-wrap items-center gap-2'>
              <code className='text-xs'>{data.folder}</code>
              <span>These archives belong to the world, not to any container.</span>
            </span>
          }
        />
        <BackupsPanel worldId={data.id} worldName={data.name} />
      </div>
    );
  }

  const live = status.data;
  const generated = data.generated;

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <SectionHeading
        title={page === 'gameplay' ? 'Gameplay rules' : 'Terrain generation'}
        icon={page === 'gameplay' ? Gamepad2 : Blocks}
        description={
          <span className='flex flex-wrap items-center gap-2'>
            <code className='text-xs'>{data.folder}</code>
            {data.attachedInstanceId ? (
              onOpenInstance ? (
                <button
                  type='button'
                  className='rounded-sm bg-muted px-1.5 py-0.5 text-xs font-medium hover:underline'
                  onClick={() => onOpenInstance(data.attachedInstanceId!)}
                >
                  mounted by {serverName ?? data.attachedInstanceId}
                </button>
              ) : (
                <Badge variant='secondary' className='gap-1'>
                  mounted by {serverName ?? data.attachedInstanceId}
                </Badge>
              )
            ) : (
              <Badge className='bg-main/15 text-main'>free</Badge>
            )}
          </span>
        }
        action={
          page === 'gameplay' ? (
            <>
              {!data.attachedInstanceId && onLaunch ? (
                <Button variant='outline' size='sm' onClick={onLaunch}>
                  <Play />
                  Launch with this world
                </Button>
              ) : null}
              {dirtyCount > 0 ? (
                <Button variant='ghost' size='sm' onClick={() => setDraft(null)}>
                  <Undo2 />
                  Discard {dirtyCount}
                </Button>
              ) : null}
              <BevelButton
                size='sm'
                className='px-4 py-4 text-sm'
                disabled={!dirtyCount || update.isPending}
                onClick={() => update.mutate(changes, { onSuccess: () => setDraft(null) })}
              >
                <Save />
                {update.isPending ? 'Saving…' : dirtyCount ? `Save ${dirtyCount}` : 'Saved'}
              </BevelButton>
            </>
          ) : undefined
        }
      />

      <StatStrip
        items={[
          {
            label: 'On disk',
            value: formatBytes(data.sizeBytes, 1),
            hint: live ? `${formatNumber(live.chunks)} chunks` : undefined,
          },
          {
            label: 'Entities',
            value: live ? formatNumber(live.entities) : '—',
            hint: live ? `${formatNumber(live.blockEntities)} block entities` : undefined,
          },
          {
            label: 'In-game day',
            value: live ? formatNumber(live.day) : '—',
            hint: live?.timeOfDay,
          },
          {
            label: 'Players',
            value: live ? formatNumber(live.playerCount) : '—',
            hint: live?.lastPlayedAt ? `last played ${formatRelative(live.lastPlayedAt)}` : 'never played',
          },
        ]}
      />

      {page === 'gameplay' ? (
        <div className='grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]'>
          <Card className='gap-0 overflow-hidden py-0'>
            <CardHeader className='border-b bg-muted/30 px-4 py-3'>
              <CardTitle className='text-base'>Rules for this world</CardTitle>
              <CardDescription>
                These follow the world, so two servers can share a configuration but play differently.
              </CardDescription>
            </CardHeader>
            <CardContent className='divide-y p-0'>
              <div className='grid grid-cols-1 gap-4 p-4 sm:grid-cols-3'>
                <div className='space-y-1.5'>
                  <label htmlFor='world-difficulty' className='text-sm font-medium'>
                    Difficulty
                  </label>
                  <Select
                    value={current.difficulty}
                    items={DIFFICULTIES}
                    onValueChange={(value) => value !== null && patch({ difficulty: value as Difficulty })}
                  >
                    <SelectTrigger id='world-difficulty' className='w-full'>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {DIFFICULTIES.map((entry) => (
                        <SelectItem key={entry.value} value={entry.value}>
                          {entry.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className='space-y-1.5'>
                  <label htmlFor='world-gamemode' className='text-sm font-medium'>
                    Gamemode
                  </label>
                  <Select
                    value={current.gamemode}
                    items={GAMEMODES}
                    onValueChange={(value) => value !== null && patch({ gamemode: value as GameMode })}
                  >
                    <SelectTrigger id='world-gamemode' className='w-full'>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {GAMEMODES.map((entry) => (
                        <SelectItem key={entry.value} value={entry.value}>
                          {entry.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className='space-y-1.5'>
                  <label htmlFor='world-spawn-protection' className='text-sm font-medium'>
                    Spawn protection
                  </label>
                  <Input
                    id='world-spawn-protection'
                    type='number'
                    value={current.spawnProtection}
                    onChange={(event) => patch({ spawnProtection: Number(event.target.value) })}
                    className='tabular-nums'
                  />
                </div>
              </div>
              <ToggleRow
                id='world-pvp'
                label='PvP'
                description='Players can damage each other.'
                checked={current.pvp}
                onChange={(value) => patch({ pvp: value })}
              />
              <ToggleRow
                id='world-hardcore'
                label='Hardcore'
                description='Ban players on death. Applies from the next start.'
                checked={current.hardcore}
                onChange={(value) => patch({ hardcore: value })}
              />
              <ToggleRow
                id='world-spawn-monsters'
                label='Spawn monsters'
                description='Hostile mobs can spawn.'
                checked={current.spawnMonsters}
                onChange={(value) => patch({ spawnMonsters: value })}
              />
              <ToggleRow
                id='world-spawn-npcs'
                label='Spawn villagers'
                description='Village NPCs can spawn.'
                checked={current.spawnNpcs}
                onChange={(value) => patch({ spawnNpcs: value })}
              />
              <ToggleRow
                id='world-allow-flight'
                label='Allow flight'
                description='Permits flight in survival and prevents the flying kick.'
                checked={current.allowFlight}
                onChange={(value) => patch({ allowFlight: value })}
              />
              <ToggleRow
                id='world-command-block'
                label='Command blocks'
                description='Enables command blocks in this world.'
                checked={current.enableCommandBlock}
                onChange={(value) => patch({ enableCommandBlock: value })}
              />
              <ToggleRow
                id='world-allow-nether'
                label='Allow nether'
                description='Enables travel to the nether dimension.'
                checked={current.allowNether}
                onChange={(value) => patch({ allowNether: value })}
              />
            </CardContent>
          </Card>

          <div className='space-y-4'>
            <Card>
              <CardHeader>
                <CardTitle className='flex items-center gap-2'>
                  <Globe className='size-4' /> Dimensions
                </CardTitle>
                <CardDescription>Generated alongside the overworld.</CardDescription>
              </CardHeader>
              <CardContent className='space-y-3'>
                <div className='flex items-center justify-between text-sm'>
                  <span className='flex items-center gap-2'>
                    <Sparkles className='size-3.5 text-muted-foreground' /> Nether
                  </span>
                  <Badge variant={live?.dimensions.nether ? 'default' : 'secondary'}>
                    {live?.dimensions.nether ? 'present' : 'absent'}
                  </Badge>
                </div>
                <div className='flex items-center justify-between text-sm'>
                  <span className='flex items-center gap-2'>
                    <Sparkles className='size-3.5 text-muted-foreground' /> End
                  </span>
                  <Badge variant={live?.dimensions.end ? 'default' : 'secondary'}>
                    {live?.dimensions.end ? 'present' : 'absent'}
                  </Badge>
                </div>
                <Separator />
                <div className='divide-y'>
                  <KeyValue label='Max world size' value={`${formatNumber(current.maxWorldSize)} blocks`} />
                  <KeyValue label='Datapacks enabled' value={String(data.dataPacks.enabled.length)} />
                  <KeyValue label='Datapacks disabled' value={String(data.dataPacks.disabled.length)} />
                </div>
              </CardContent>
            </Card>

            <SwitchWorldHint world={data} />
          </div>
        </div>
      ) : (
        <div className='grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]'>
          <Card className='gap-0 overflow-hidden py-0'>
            <CardHeader className='border-b bg-muted/30 px-4 py-3'>
              <CardTitle className='flex items-center gap-2 text-base'>
                <Blocks className='size-4' /> Generation settings
              </CardTitle>
              <CardDescription>
                {generated
                  ? 'This world has generated. These fields are locked because changing them does nothing to existing terrain.'
                  : 'Still pristine — you can pick these freely before the first start.'}
              </CardDescription>
            </CardHeader>
            <CardContent className='divide-y p-0'>
              <div className='grid grid-cols-1 gap-4 p-4 sm:grid-cols-2'>
                <div className='space-y-1.5'>
                  <span className='flex items-center gap-1.5 text-sm font-medium'>
                    Level type
                    {generated ? <Lock aria-hidden='true' className='size-3 text-muted-foreground' /> : null}
                  </span>
                  <p className='text-sm text-muted-foreground'>
                    {LEVEL_TYPES.find((entry) => entry.value === data.levelType)?.label ?? data.levelType}
                  </p>
                </div>
                <div className='space-y-1.5'>
                  <span className='flex items-center gap-1.5 text-sm font-medium'>
                    Seed
                    {generated ? <Lock aria-hidden='true' className='size-3 text-muted-foreground' /> : null}
                  </span>
                  <p className='font-mono text-sm text-muted-foreground'>{data.seed ?? 'random'}</p>
                </div>
                <div className='space-y-1.5'>
                  <span className='text-sm font-medium'>Generate structures</span>
                  <p className='text-sm text-muted-foreground'>
                    {data.generateStructures ? 'Villages, temples and ruins enabled' : 'Structures disabled'}
                  </p>
                </div>
                <div className='space-y-1.5'>
                  <span className='text-sm font-medium'>Created</span>
                  <p className='text-sm text-muted-foreground'>{formatRelative(data.createdAt)}</p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <Database className='size-4' /> Region data
              </CardTitle>
              <CardDescription>Where the world lives on the host.</CardDescription>
            </CardHeader>
            <CardContent className='space-y-3'>
              <Progress value={live ? Math.min(100, live.chunks / 1000) : 0} aria-label='Chunks generated' />
              <div className='divide-y'>
                <KeyValue label='Chunks' value={live ? formatNumber(live.chunks) : '—'} />
                <KeyValue label='Entities' value={live ? formatNumber(live.entities) : '—'} />
                <KeyValue label='Block entities' value={live ? formatNumber(live.blockEntities) : '—'} />
                <KeyValue label='Disk' value={formatBytes(live?.diskBytes ?? data.sizeBytes, 1)} />
              </div>
              <p className='flex items-center gap-2 text-xs text-muted-foreground'>
                <Play className='size-3.5' />
                {data.attachedInstanceId
                  ? 'A server is running this world right now.'
                  : 'No server has it mounted; the data sits safely on disk.'}
              </p>
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}

/**
 * Explains the hot-swap behaviour: moving this world onto another instance
 * stops the container, remounts the volume and starts it again.
 */
function SwitchWorldHint({ world }: { world: World }) {
  if (!world.attachedInstanceId) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle className='flex items-center gap-2'>
          <TriangleAlert className='size-4' /> Mounted elsewhere
        </CardTitle>
        <CardDescription>In use by a server.</CardDescription>
      </CardHeader>
      <CardContent className='space-y-3 text-sm'>
        <p className='text-muted-foreground'>
          A world is mounted by one server at a time. Moving it onto another server stops the
          container, remounts the volume and starts it again — both the configuration and this world
          are kept.
        </p>
        <div className='flex items-start gap-2 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-3'>
          <TriangleAlert className='mt-0.5 size-4 shrink-0 text-yellow-500' />
          <p>
            If the replacement world generates a different level type, existing chunks stay as they
            were — Minecraft only applies the new type to terrain it has not generated yet.
          </p>
        </div>
      </CardContent>
    </Card>
  );
}
