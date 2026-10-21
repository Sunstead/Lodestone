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
import { Button } from '@sunstead/ui/components/button';
import {
  useCreateInstance,
  useCreateProfile,
  useCreateWorld,
  useInstances,
  useProfiles,
  useSoftware,
  useWorlds,
} from '#/api/queries';
import type {
  CreateInstanceRequest,
  CreateProfileRequest,
  CreateWorldRequest,
  Difficulty,
  GameMode,
  LevelType,
  ServerSoftwareId,
} from '#/api/types';
import { BevelButton } from '#/components/server-hero';
import { formatBytes } from '#/lib/format';
import {
  Box,
  Check,
  Layers,
  Plus,
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

type Mode = 'existing' | 'create';

/**
 * Two-mode creation flow. An instance always needs one profile and one
 * world; either can be an asset you already have, or made inline here.
 */
export function CreateInstanceDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (instanceId: string) => void;
}) {
  const worlds = useWorlds();
  const profiles = useProfiles();
  const instances = useInstances();
  const software = useSoftware();
  const createInstance = useCreateInstance();
  const createWorld = useCreateWorld();
  const createProfile = useCreateProfile();

  const [name, setName] = useState('');
  const [worldMode, setWorldMode] = useState<Mode>('existing');
  const [profileMode, setProfileMode] = useState<Mode>('existing');

  const freeWorlds = useMemo(
    () => (worlds.data ?? []).filter((world) => world.attachedInstanceId === null),
    [worlds.data],
  );
  const freeProfiles = useMemo(
    () => (profiles.data ?? []).filter((profile) => profile.instanceCount === 0),
    [profiles.data],
  );

  const [worldId, setWorldId] = useState('');
  const [profileId, setProfileId] = useState('');
  const [minecraftPort, setMinecraftPort] = useState(25565);
  const [rconPort, setRconPort] = useState(25575);
  const [queryPort, setQueryPort] = useState<number | null>(null);

  // Inline world draft.
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

  // Inline profile draft.
  const [profileDraft, setProfileDraft] = useState<ProfileInlineDraft>({
    name: '',
    softwareId: 'paper',
    version: '1.21.4',
    memory: { minMb: 2048, maxMb: 4096, initialMb: 2048 },
  });

  // Preselect the first free asset of each kind so the happy path is one click.
  useEffect(() => {
    if (!open) return;
    setName('');
    setWorldMode(freeWorlds.length > 0 ? 'existing' : 'create');
    setProfileMode(freeProfiles.length > 0 ? 'existing' : 'create');
    setWorldId(freeWorlds[0]?.id ?? '');
    setProfileId(freeProfiles[0]?.id ?? '');
    const nextPort = 25565 + (instances.data?.length ?? 0) * 2;
    setMinecraftPort(nextPort);
    setRconPort(nextPort + 10);
    setQueryPort(null);
    setWorldDraft((previous) => ({ ...previous, name: '' }));
    setProfileDraft((previous) => ({ ...previous, name: '' }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const options = software.data?.software ?? [];
  const selectedSoftware = options.find((entry) => entry.id === profileDraft.softwareId);
  const versions =
    selectedSoftware?.versions.map((entry) => ({ value: entry.version, label: entry.version })) ?? [];

  const busy = createInstance.isPending || createWorld.isPending || createProfile.isPending;

  const canSubmit =
    name.trim().length > 0 &&
    (worldMode === 'existing' ? worldId !== '' : worldDraft.name.trim().length > 0) &&
    (profileMode === 'existing' ? profileId !== '' : profileDraft.name.trim().length > 0);

  const submit = async () => {
    let finalWorldId = worldId;
    let finalProfileId = profileId;

    if (worldMode === 'create') {
      const created = await createWorld.mutateAsync(worldDraft);
      finalWorldId = created.id;
    }
    if (profileMode === 'create') {
      const body: CreateProfileRequest = {
        ...profileDraft,
        description: 'Created while setting up an instance',
        cpuCores: null,
        javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=200',
        autoStart: true,
        restartOnCrash: true,
        crashThreshold: 3,
      };
      const created = await createProfile.mutateAsync(body);
      finalProfileId = created.id;
    }

    const body: CreateInstanceRequest = {
      name: name.trim(),
      profileId: finalProfileId,
      worldId: finalWorldId,
      minecraftPort,
      rconPort,
      queryPort,
    };

    createInstance.mutate(body, {
      onSuccess: (created) => {
        onOpenChange(false);
        onCreated(created.id);
      },
    });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='sm:max-w-2xl'>
        <DialogHeader>
          <DialogTitle>New instance</DialogTitle>
          <DialogDescription>
            An instance is one profile paired with one world, running in its own container. Deleting
            it later keeps both assets.
          </DialogDescription>
        </DialogHeader>

        <div className='max-h-[62vh] space-y-5 overflow-y-auto pr-1'>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor='instance-name'>Name</FieldLabel>
              <Input
                id='instance-name'
                value={name}
                placeholder='Survival (main)'
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
          </FieldGroup>

          <AssetPicker
            icon={Layers}
            title='World'
            description='Supplies the terrain, gameplay rules and backups.'
            mode={worldMode}
            onModeChange={setWorldMode}
            existingCount={freeWorlds.length}
          >
            {worldMode === 'existing' ? (
              <Select
                value={worldId}
                items={freeWorlds.map((world) => ({
                  value: world.id,
                  label: `${world.name} · ${world.folder}`,
                }))}
                onValueChange={(value) => value !== null && setWorldId(value)}
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
            ) : (
              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor='inline-world-name'>World name</FieldLabel>
                  <Input
                    id='inline-world-name'
                    value={worldDraft.name}
                    placeholder='Survival Season 2'
                    onChange={(event) =>
                      setWorldDraft((previous) => ({ ...previous, name: event.target.value }))
                    }
                  />
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
                      items={[
                        { value: 'peaceful', label: 'Peaceful' },
                        { value: 'easy', label: 'Easy' },
                        { value: 'normal', label: 'Normal' },
                        { value: 'hard', label: 'Hard' },
                      ]}
                      onValueChange={(value) =>
                        setWorldDraft((previous) => ({ ...previous, difficulty: value as Difficulty }))
                      }
                    >
                      <SelectTrigger className='w-full'>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {(['peaceful', 'easy', 'normal', 'hard'] as const).map((value) => (
                          <SelectItem key={value} value={value} className='capitalize'>
                            {value}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                  <Field>
                    <FieldLabel>Gamemode</FieldLabel>
                    <Select
                      value={worldDraft.gamemode}
                      items={[
                        { value: 'survival', label: 'Survival' },
                        { value: 'creative', label: 'Creative' },
                        { value: 'adventure', label: 'Adventure' },
                        { value: 'spectator', label: 'Spectator' },
                      ]}
                      onValueChange={(value) =>
                        setWorldDraft((previous) => ({ ...previous, gamemode: value as GameMode }))
                      }
                    >
                      <SelectTrigger className='w-full'>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {(['survival', 'creative', 'adventure', 'spectator'] as const).map((value) => (
                          <SelectItem key={value} value={value} className='capitalize'>
                            {value}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </Field>
                </div>
              </FieldGroup>
            )}
          </AssetPicker>

          <AssetPicker
            icon={Box}
            title='Profile'
            description='Supplies the software, version, mods and resource limits.'
            mode={profileMode}
            onModeChange={setProfileMode}
            existingCount={freeProfiles.length}
          >
            {profileMode === 'existing' ? (
              <Select
                value={profileId}
                items={freeProfiles.map((profile) => ({
                  value: profile.id,
                  label: `${profile.name} · ${profile.software} ${profile.version}`,
                }))}
                onValueChange={(value) => value !== null && setProfileId(value)}
              >
                <SelectTrigger className='w-full'>
                  <SelectValue placeholder='Pick a free profile' />
                </SelectTrigger>
                <SelectContent>
                  {freeProfiles.map((profile) => (
                    <SelectItem key={profile.id} value={profile.id}>
                      {profile.name} · {profile.software} {profile.version}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <FieldGroup>
                <Field>
                  <FieldLabel htmlFor='inline-profile-name'>Profile name</FieldLabel>
                  <Input
                    id='inline-profile-name'
                    value={profileDraft.name}
                    placeholder='Paper 1.21 performance'
                    onChange={(event) =>
                      setProfileDraft((previous) => ({ ...previous, name: event.target.value }))
                    }
                  />
                </Field>
                <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
                  <Field>
                    <FieldLabel>Software</FieldLabel>
                    <Select
                      value={profileDraft.softwareId}
                      items={options.map((entry) => ({ value: entry.id, label: entry.label }))}
                      onValueChange={(value) => {
                        const id = value as ServerSoftwareId;
                        const option = options.find((entry) => entry.id === id);
                        setProfileDraft((previous) => ({
                          ...previous,
                          softwareId: id,
                          version: option?.versions[0]?.version ?? previous.version,
                        }));
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
                  </Field>
                  <Field>
                    <FieldLabel>Version</FieldLabel>
                    <Select
                      value={profileDraft.version}
                      items={versions}
                      onValueChange={(value) => {
                        if (value === null) return;
                        setProfileDraft((previous) => ({ ...previous, version: value }));
                      }}
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
                <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
                  <Field>
                    <FieldLabel htmlFor='inline-profile-min'>Min heap (MB)</FieldLabel>
                    <Input
                      id='inline-profile-min'
                      type='number'
                      value={profileDraft.memory.minMb}
                      className='tabular-nums'
                      onChange={(event) =>
                        setProfileDraft((previous) => ({
                          ...previous,
                          memory: { ...previous.memory, minMb: Number(event.target.value) },
                        }))
                      }
                    />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor='inline-profile-max'>Max heap (MB)</FieldLabel>
                    <Input
                      id='inline-profile-max'
                      type='number'
                      value={profileDraft.memory.maxMb}
                      className='tabular-nums'
                      onChange={(event) =>
                        setProfileDraft((previous) => ({
                          ...previous,
                          memory: { ...previous.memory, maxMb: Number(event.target.value) },
                        }))
                      }
                    />
                  </Field>
                </div>
              </FieldGroup>
            )}
          </AssetPicker>

          <FieldGroup>
            <Field>
              <FieldLabel>Published ports</FieldLabel>
              <div className='grid grid-cols-1 gap-4 sm:grid-cols-3'>
                <div className='space-y-1.5'>
                  <label htmlFor='instance-mc-port' className='text-xs text-muted-foreground'>
                    Minecraft
                  </label>
                  <Input
                    id='instance-mc-port'
                    type='number'
                    value={minecraftPort}
                    onChange={(event) => setMinecraftPort(Number(event.target.value))}
                    className='tabular-nums'
                  />
                </div>
                <div className='space-y-1.5'>
                  <label htmlFor='instance-rcon-port' className='text-xs text-muted-foreground'>
                    RCON
                  </label>
                  <Input
                    id='instance-rcon-port'
                    type='number'
                    value={rconPort}
                    onChange={(event) => setRconPort(Number(event.target.value))}
                    className='tabular-nums'
                  />
                </div>
                <div className='space-y-1.5'>
                  <label htmlFor='instance-query-port' className='text-xs text-muted-foreground'>
                    Query
                  </label>
                  <Input
                    id='instance-query-port'
                    type='number'
                    value={queryPort ?? ''}
                    placeholder='disabled'
                    onChange={(event) =>
                      setQueryPort(event.target.value === '' ? null : Number(event.target.value))
                    }
                    className='tabular-nums'
                  />
                </div>
              </div>
              <FieldDescription>
                Each container needs its own ports. The defaults step past the instances you already have.
              </FieldDescription>
            </Field>
          </FieldGroup>

          {worldMode === 'existing' && worldId ? (
            <CompatibilityNote
              worldLevelType={freeWorlds.find((world) => world.id === worldId)?.levelType}
              profileVersion={freeProfiles.find((profile) => profile.id === profileId)?.version}
            />
          ) : null}
        </div>

        <DialogFooter>
          <Button variant='outline' onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <BevelButton className='px-5 py-4 text-sm' disabled={!canSubmit || busy} onClick={submit}>
            <Plus />
            {createInstance.isPending ? 'Creating…' : 'Create instance'}
          </BevelButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

type ProfileInlineDraft = Pick<
  CreateProfileRequest,
  'name' | 'softwareId' | 'version' | 'memory'
>;

function AssetPicker({
  icon: Icon,
  title,
  description,
  mode,
  onModeChange,
  existingCount,
  children,
}: {
  icon: typeof Box;
  title: string;
  description: string;
  mode: Mode;
  onModeChange: (mode: Mode) => void;
  existingCount: number;
  children: React.ReactNode;
}) {
  return (
    <div className='space-y-3 rounded-md border p-4'>
      <div className='flex flex-wrap items-center justify-between gap-3'>
        <div className='flex items-start gap-3'>
          <span className='flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted/40'>
            <Icon className='size-4 text-muted-foreground' />
          </span>
          <div className='min-w-0'>
            <p className='font-medium'>{title}</p>
            <p className='text-xs text-muted-foreground'>{description}</p>
          </div>
        </div>
        <div className='flex items-center gap-1 rounded-md border p-1'>
          <Button
            variant={mode === 'existing' ? 'secondary' : 'ghost'}
            size='sm'
            disabled={existingCount === 0}
            aria-pressed={mode === 'existing'}
            onClick={() => onModeChange('existing')}
          >
            {existingCount === 0 ? 'None free' : `Use existing (${existingCount})`}
          </Button>
          <Button
            variant={mode === 'create' ? 'secondary' : 'ghost'}
            size='sm'
            aria-pressed={mode === 'create'}
            onClick={() => onModeChange('create')}
          >
            <Plus className='size-3.5' />
            Create new
          </Button>
        </div>
      </div>
      {children}
    </div>
  );
}

function CompatibilityNote({
  worldLevelType,
  profileVersion,
}: {
  worldLevelType?: string;
  profileVersion?: string;
}) {
  return (
    <div className='flex items-start gap-2 rounded-md border bg-muted/40 p-3 text-sm'>
      <Check className='mt-0.5 size-4 shrink-0 text-main' />
      <div className='min-w-0'>
        <p className='font-medium'>Ready to pair</p>
        <p className='text-xs text-muted-foreground'>
          {worldLevelType ? `${worldLevelType.replace('minecraft:', '')} terrain` : 'world'}
          {profileVersion ? ` · Java ${profileVersion}; Minecraft worlds are version-portable but always back up first.` : ''}
        </p>
      </div>
    </div>
  );
}

export function CompatibilityWarning({ worldLevelType }: { worldLevelType: string }) {
  return (
    <div className='flex items-start gap-2 rounded-md border border-yellow-500/40 bg-yellow-500/10 p-3 text-sm'>
      <TriangleAlert className='mt-0.5 size-4 shrink-0 text-yellow-500' />
      <div>
        <p className='font-medium'>Level type differs</p>
        <p className='text-xs text-muted-foreground'>
          The new world is {worldLevelType.replace('minecraft:', '')}. Chunks already generated stay
          as they are; only fresh terrain follows the new type.
        </p>
      </div>
    </div>
  );
}
