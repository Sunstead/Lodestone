import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@sunstead/ui/components/dialog';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@sunstead/ui/components/field';
import { Input } from '@sunstead/ui/components/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@sunstead/ui/components/select';
import { Separator } from '@sunstead/ui/components/separator';
import { useQueryClient } from '@tanstack/react-query';
import {
  useCreateInstance,
  useCreateProfile,
  useCreateWorld,
  useInstances,
  useSoftware,
  useWorlds,
} from '#/api/queries';
import { runAction } from '#/api/client';
import type {
  CreateInstanceRequest,
  CreateProfileRequest,
  CreateWorldRequest,
  Difficulty,
  GameMode,
  Instance,
  LevelType,
  ServerSoftwareId,
} from '#/api/types';
import { BevelButton } from '#/components/server-hero';
import { formatBytes } from '#/lib/format';
import {
  Check,
  ChevronDown,
  Layers,
  Loader2,
  Play,
  Server,
  Sparkles,
  TriangleAlert,
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

type Phase = 'configure' | 'launching' | 'done';

/**
 * The one-screen "launch a world" flow. It picks a world (or makes one),
 * pairs it with a configuration built from a recommended software + version,
 * then creates and starts the server in a single step.
 *
 * The full instance dialog still exists for people who want to reuse an
 * existing configuration; this is the shortcut for the common case.
 */
export function LaunchWorldDialog({
  open,
  onOpenChange,
  worldId,
  onLaunched,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Preselects a world when launched from its page. */
  worldId?: string;
  onLaunched: (instanceId: string) => void;
}) {
  const worlds = useWorlds();
  const instances = useInstances();
  const software = useSoftware();
  const queryClient = useQueryClient();
  const createInstance = useCreateInstance();
  const createProfile = useCreateProfile();
  const createWorld = useCreateWorld();

  const [phase, setPhase] = useState<Phase>('configure');
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [created, setCreated] = useState<Instance | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [worldSource, setWorldSource] = useState<'existing' | 'new'>('existing');
  const [selectedWorldId, setSelectedWorldId] = useState('');
  const [serverName, setServerName] = useState('');

  const options = software.data?.software ?? [];
  const [softwareId, setSoftwareId] = useState<ServerSoftwareId>('paper');
  const [version, setVersion] = useState('1.21.4');
  const selectedSoftware = options.find((entry) => entry.id === softwareId);
  const versions =
    selectedSoftware?.versions.map((entry) => ({ value: entry.version, label: entry.version })) ?? [];

  const [maxMemory, setMaxMemory] = useState(4096);
  const [minecraftPort, setMinecraftPort] = useState(25565);
  const [rconPort, setRconPort] = useState(25575);

  const [worldDraft, setWorldDraft] = useState<CreateWorldRequest>({
    name: '',
    levelType: 'minecraft:normal',
    seed: null,
    generateStructures: true,
    difficulty: 'normal',
    gamemode: 'survival',
    hardcore: false,
    maxWorldSize: 29_999_984,
    allowNether: true,
  });

  const freeWorlds = useMemo(
    () => (worlds.data ?? []).filter((world) => world.attachedInstanceId === null),
    [worlds.data],
  );

  // Reset to a fresh state each time the dialog opens.
  useEffect(() => {
    if (!open) return;
    setPhase('configure');
    setAdvancedOpen(false);
    setCreated(null);
    setError(null);
    setWorldSource(freeWorlds.length > 0 ? 'existing' : 'new');
    const preselected = worldId && freeWorlds.some((world) => world.id === worldId) ? worldId : '';
    setSelectedWorldId(preselected || freeWorlds[0]?.id || '');
    setServerName('');
    const nextPort = 25565 + (instances.data?.length ?? 0) * 2;
    setMinecraftPort(nextPort);
    setRconPort(nextPort + 10);
    setWorldDraft((previous) => ({ ...previous, name: '' }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const chosenWorld =
    worldSource === 'existing'
      ? freeWorlds.find((world) => world.id === selectedWorldId)
      : undefined;

  const suggestedName = useMemo(() => {
    if (worldSource === 'existing') {
      const world = freeWorlds.find((entry) => entry.id === selectedWorldId);
      return world ? world.name : '';
    }
    return worldDraft.name;
  }, [worldSource, freeWorlds, selectedWorldId, worldDraft.name]);

  const busy = createInstance.isPending || createProfile.isPending || createWorld.isPending;

  const canLaunch =
    (worldSource === 'existing' ? selectedWorldId !== '' : worldDraft.name.trim().length > 0) &&
    (serverName.trim().length > 0 || suggestedName.trim().length > 0) &&
    versions.length > 0;

  const launch = async () => {
    setError(null);
    setPhase('launching');
    try {
      let finalWorldId = selectedWorldId;
      if (worldSource === 'new') {
        const world = await createWorld.mutateAsync(worldDraft);
        finalWorldId = world.id;
      }

      const profileBody: CreateProfileRequest = {
        name: `${selectedSoftware?.label ?? softwareId} ${version} · ${suggestedName || serverName}`.trim(),
        description: 'Created by the Launch world flow',
        softwareId,
        version,
        memory: { minMb: Math.min(2048, maxMemory), maxMb: maxMemory, initialMb: Math.min(2048, maxMemory) },
        cpuCores: null,
        javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=200',
        autoStart: true,
        restartOnCrash: true,
        crashThreshold: 3,
      };
      const profile = await createProfile.mutateAsync(profileBody);

      const body: CreateInstanceRequest = {
        name: serverName.trim() || suggestedName.trim(),
        profileId: profile.id,
        worldId: finalWorldId,
        minecraftPort,
        rconPort,
        queryPort: null,
      };
      const instance = await createInstance.mutateAsync(body);
      setCreated(instance);
      await runAction(instance.id, 'start');
      void queryClient.invalidateQueries({ queryKey: ['instances'] });
      setPhase('done');
    } catch {
      setError('Something went wrong while launching. Check the configuration and try again.');
      setPhase('configure');
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='sm:max-w-xl'>
        <DialogHeader>
          <DialogTitle>Launch a world</DialogTitle>
          <DialogDescription>
            Pick a world, choose the server software, and Lodestone creates a server and starts it.
            The world and its new configuration are both kept.
          </DialogDescription>
        </DialogHeader>

        {phase === 'configure' ? (
          <div className='max-h-[64vh] space-y-5 overflow-y-auto pr-1'>
            <FieldGroup>
              <Field>
                <FieldLabel>World</FieldLabel>
                <div className='flex items-center gap-1 rounded-md border p-1'>
                  <Button
                    variant={worldSource === 'existing' ? 'secondary' : 'ghost'}
                    size='sm'
                    disabled={freeWorlds.length === 0}
                    aria-pressed={worldSource === 'existing'}
                    onClick={() => setWorldSource('existing')}
                  >
                    {freeWorlds.length === 0 ? 'None free' : `Existing (${freeWorlds.length})`}
                  </Button>
                  <Button
                    variant={worldSource === 'new' ? 'secondary' : 'ghost'}
                    size='sm'
                    aria-pressed={worldSource === 'new'}
                    onClick={() => setWorldSource('new')}
                  >
                    New world
                  </Button>
                </div>
              </Field>

              {worldSource === 'existing' ? (
                <Field>
                  <Select
                    value={selectedWorldId}
                    items={freeWorlds.map((world) => ({
                      value: world.id,
                      label: `${world.name} · ${world.folder}`,
                    }))}
                    onValueChange={(value) => value !== null && setSelectedWorldId(value)}
                  >
                    <SelectTrigger className='w-full'>
                      <SelectValue placeholder='Pick a free world' />
                    </SelectTrigger>
                    <SelectContent>
                      {freeWorlds.map((world) => (
                        <SelectItem key={world.id} value={world.id}>
                          {world.name} · {world.folder} · {formatBytes(world.sizeBytes, 1)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {chosenWorld ? (
                    <FieldDescription>
                      {chosenWorld.generated
                        ? 'Already generated — its terrain stays as it is.'
                        : 'Pristine — generation settings still apply at first start.'}
                    </FieldDescription>
                  ) : null}
                </Field>
              ) : (
                <div className='space-y-4'>
                  <Field>
                    <FieldLabel htmlFor='launch-world-name'>World name</FieldLabel>
                    <Input
                      id='launch-world-name'
                      value={worldDraft.name}
                      placeholder='Survival Season 2'
                      onChange={(event) =>
                        setWorldDraft((previous) => ({ ...previous, name: event.target.value }))
                      }
                    />
                    <FieldDescription>The folder on disk is derived from this name.</FieldDescription>
                  </Field>
                  <div className='grid grid-cols-1 gap-4 sm:grid-cols-3'>
                    <Field>
                      <FieldLabel>Level type</FieldLabel>
                      <Select
                        value={worldDraft.levelType}
                        items={LEVEL_TYPES}
                        onValueChange={(value) =>
                          setWorldDraft((previous) => ({ ...previous, levelType: value as LevelType }))
                        }
                      >
                        <SelectTrigger className='w-full'>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {LEVEL_TYPES.map((entry) => (
                            <SelectItem key={entry.value} value={entry.value}>
                              {entry.label}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <FieldLabel>Difficulty</FieldLabel>
                      <Select
                        value={worldDraft.difficulty}
                        items={DIFFICULTIES}
                        onValueChange={(value) =>
                          setWorldDraft((previous) => ({ ...previous, difficulty: value as Difficulty }))
                        }
                      >
                        <SelectTrigger className='w-full'>
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
                    </Field>
                    <Field>
                      <FieldLabel>Gamemode</FieldLabel>
                      <Select
                        value={worldDraft.gamemode}
                        items={GAMEMODES}
                        onValueChange={(value) =>
                          setWorldDraft((previous) => ({ ...previous, gamemode: value as GameMode }))
                        }
                      >
                        <SelectTrigger className='w-full'>
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
                    </Field>
                  </div>
                </div>
              )}
            </FieldGroup>

            <Separator />

            <FieldGroup>
              <div className='flex items-start gap-2 rounded-md border bg-muted/40 p-3 text-sm'>
                <Sparkles className='mt-0.5 size-4 shrink-0 text-main' />
                <div className='min-w-0'>
                  <p className='font-medium'>Recommended setup</p>
                  <p className='text-xs text-muted-foreground'>
                    Paper runs most plugins and performs well out of the box. Change the software below
                    if this world is built for something else.
                  </p>
                </div>
              </div>

              <Field>
                <FieldLabel htmlFor='launch-server-name'>Server name</FieldLabel>
                <Input
                  id='launch-server-name'
                  value={serverName}
                  placeholder={suggestedName || 'Survival (main)'}
                  onChange={(event) => setServerName(event.target.value)}
                />
              </Field>

              <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
                <Field>
                  <FieldLabel>Software</FieldLabel>
                  <Select
                    value={softwareId}
                    items={options.map((entry) => ({ value: entry.id, label: entry.label }))}
                    onValueChange={(value) => {
                      const id = value as ServerSoftwareId;
                      const option = options.find((entry) => entry.id === id);
                      setSoftwareId(id);
                      setVersion(option?.versions[0]?.version ?? version);
                    }}
                  >
                    <SelectTrigger className='w-full'>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {options.map((entry) => (
                        <SelectItem key={entry.id} value={entry.id}>
                          {entry.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {selectedSoftware ? (
                    <FieldDescription>{selectedSoftware.blurb}</FieldDescription>
                  ) : null}
                </Field>

                <Field>
                  <FieldLabel>Minecraft version</FieldLabel>
                  <Select
                    value={version}
                    items={versions}
                    onValueChange={(value) => value !== null && setVersion(value)}
                  >
                    <SelectTrigger className='w-full'>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {versions.map((entry) => (
                        <SelectItem key={entry.value} value={entry.value}>
                          {entry.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              </div>

              {selectedSoftware?.kind === 'mods' ? (
                <div className='flex items-start gap-2 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-3 text-sm'>
                  <TriangleAlert className='mt-0.5 size-4 shrink-0 text-yellow-500' />
                  <div>
                    <p className='font-medium'>Modded loader</p>
                    <p className='text-xs text-muted-foreground'>
                      {selectedSoftware.label} loads mods. Add them from the configuration page before
                      relying on this world.
                    </p>
                  </div>
                </div>
              ) : null}
            </FieldGroup>

            <div className='rounded-md border'>
              <button
                type='button'
                className='flex w-full items-center justify-between gap-2 px-4 py-3 text-left text-sm font-medium'
                aria-expanded={advancedOpen}
                onClick={() => setAdvancedOpen((previous) => !previous)}
              >
                <span>Advanced</span>
                <ChevronDown
                  aria-hidden='true'
                  className={`size-4 text-muted-foreground transition-transform ${advancedOpen ? 'rotate-180' : ''}`}
                />
              </button>
              {advancedOpen ? (
                <div className='space-y-4 border-t p-4'>
                  <div className='grid grid-cols-1 gap-4 sm:grid-cols-3'>
                    <Field>
                      <FieldLabel htmlFor='launch-memory'>Max heap (MB)</FieldLabel>
                      <Input
                        id='launch-memory'
                        type='number'
                        value={maxMemory}
                        className='tabular-nums'
                        onChange={(event) => setMaxMemory(Number(event.target.value))}
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor='launch-mc-port'>Minecraft port</FieldLabel>
                      <Input
                        id='launch-mc-port'
                        type='number'
                        value={minecraftPort}
                        className='tabular-nums'
                        onChange={(event) => setMinecraftPort(Number(event.target.value))}
                      />
                    </Field>
                    <Field>
                      <FieldLabel htmlFor='launch-rcon-port'>RCON port</FieldLabel>
                      <Input
                        id='launch-rcon-port'
                        type='number'
                        value={rconPort}
                        className='tabular-nums'
                        onChange={(event) => setRconPort(Number(event.target.value))}
                      />
                    </Field>
                  </div>
                  <FieldDescription>
                    A new configuration is created for this server with these limits. Each server needs
                    its own ports.
                  </FieldDescription>
                </div>
              ) : null}
            </div>

            {error ? (
              <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm'>
                <TriangleAlert className='mt-0.5 size-4 shrink-0 text-destructive' />
                <p>{error}</p>
              </div>
            ) : null}
          </div>
        ) : phase === 'launching' ? (
          <div className='flex flex-col items-center gap-3 py-10'>
            <Loader2 aria-hidden='true' className='size-8 animate-spin text-main' />
            <p className='font-medium'>Launching {serverName.trim() || suggestedName}…</p>
            <p className='text-sm text-muted-foreground'>
              Creating the configuration, mounting the world and starting the container.
            </p>
          </div>
        ) : (
          <div className='space-y-4 py-2'>
            <div className='flex items-start gap-3 rounded-md border border-main/40 bg-main/10 p-4'>
              <Check className='mt-0.5 size-5 shrink-0 text-main' />
              <div className='min-w-0'>
                <p className='font-medium'>Server launched</p>
                <p className='text-sm text-muted-foreground'>
                  {created?.name} is starting. Join at <code className='text-xs'>{created?.address}</code>.
                </p>
              </div>
            </div>
            <div className='divide-y rounded-md border'>
              <div className='flex items-center justify-between gap-3 px-4 py-2.5 text-sm'>
                <span className='flex items-center gap-2 text-muted-foreground'>
                  <Layers className='size-4' /> World
                </span>
                <span className='font-medium'>{chosenWorld?.name ?? worldDraft.name}</span>
              </div>
              <div className='flex items-center justify-between gap-3 px-4 py-2.5 text-sm'>
                <span className='flex items-center gap-2 text-muted-foreground'>
                  <Server className='size-4' /> Software
                </span>
                <Badge variant='secondary'>
                  {selectedSoftware?.label ?? softwareId} {version}
                </Badge>
              </div>
            </div>
          </div>
        )}

        <DialogFooter>
          {phase === 'done' ? (
            <BevelButton
              className='w-full px-5 py-4 text-sm sm:w-auto'
              onClick={() => {
                if (created) onLaunched(created.id);
                onOpenChange(false);
              }}
            >
              <Play />
              Open server
            </BevelButton>
          ) : (
            <>
              <Button variant='outline' disabled={phase === 'launching'} onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <BevelButton
                className='px-5 py-4 text-sm'
                disabled={!canLaunch || busy || phase === 'launching'}
                onClick={launch}
              >
                <Play />
                {busy ? 'Launching…' : 'Launch server'}
              </BevelButton>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
