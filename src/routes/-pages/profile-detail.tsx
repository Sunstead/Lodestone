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
import { Textarea } from '@sunstead/ui/components/textarea';
import { useInstances, useProfile, useSoftware, useUpdateProfile } from '#/api/queries';
import type { CreateProfileRequest, Profile, ServerSoftwareId } from '#/api/types';
import { EmptyState, KeyValue, SectionHeading, StatStrip } from '#/components/page-parts';
import { ModsPanel } from '#/routes/-pages/mods-panel';
import { PropertiesPage } from '#/routes/-pages/properties';
import { BevelButton } from '#/components/server-hero';
import { ServerStateDot } from '#/components/server-state';
import { formatRelative } from '#/lib/format';
import {
  Box,
  Cpu,
  MemoryStick,
  Play,
  Puzzle,
  Save,
  Server,
  Settings2,
  Undo2,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';

/**
 * Only the fields a profile owns are edited here. Generation and gameplay
 * rules live on the world, and published ports on the instance.
 */
type ProfileDraft = Pick<
  Profile,
  | 'name'
  | 'description'
  | 'softwareId'
  | 'version'
  | 'memory'
  | 'cpuCores'
  | 'javaArgs'
  | 'autoStart'
  | 'restartOnCrash'
  | 'crashThreshold'
  | 'ports'
>;

function draftOf(profile: Profile): ProfileDraft {
  return {
    name: profile.name,
    description: profile.description,
    softwareId: profile.softwareId,
    version: profile.version,
    memory: { ...profile.memory },
    cpuCores: profile.cpuCores,
    javaArgs: profile.javaArgs,
    autoStart: profile.autoStart,
    restartOnCrash: profile.restartOnCrash,
    crashThreshold: profile.crashThreshold,
    ports: { ...profile.ports },
  };
}

const CPU_OPTIONS = [
  { value: 'unbounded', label: 'Unbounded' },
  { value: '1', label: '1 core' },
  { value: '2', label: '2 cores' },
  { value: '4', label: '4 cores' },
  { value: '8', label: '8 cores' },
];

export function ProfileDetailPage({
  profileId,
  page,
  onOpenInstance,
  onLaunch,
}: {
  profileId: string;
  page: string;
  onOpenInstance?: (instanceId: string) => void;
  onLaunch?: () => void;
}) {
  const profile = useProfile(profileId);
  const software = useSoftware();
  const update = useUpdateProfile(profileId);
  const [draft, setDraft] = useState<ProfileDraft | null>(null);

  useEffect(() => {
    setDraft(null);
  }, [profileId]);

  const data = profile.data;
  const base = useMemo(() => (data ? draftOf(data) : null), [data]);
  const current = draft ?? base;

  const changes = useMemo(() => {
    if (!current || !base) return {};
    const result: Partial<CreateProfileRequest> & { ports?: Profile['ports'] } = {};
    for (const key of Object.keys(current) as (keyof ProfileDraft)[]) {
      if (JSON.stringify(current[key]) !== JSON.stringify(base[key])) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        (result as any)[key] = current[key];
      }
    }
    return result;
  }, [current, base]);

  const dirtyCount = Object.keys(changes).length;

  const patch = (next: Partial<ProfileDraft>) => {
    setDraft((previous) => ({ ...(previous ?? base!), ...next }));
  };

  if (profile.isLoading || !data || !current) {
    return (
      <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
        <Skeleton className='h-28 w-full' />
        <Skeleton className='h-96 w-full' />
      </div>
    );
  }

  // Properties and mods are full pages of their own and carry their own chrome.
  if (page === 'properties') {
    return <PropertiesPage profileId={data.id} />;
  }

  if (page === 'mods') {
    return (
      <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
        <SectionHeading
          title='Mods & Plugins'
          icon={Puzzle}
          description={`Anything installed here is shared by every server running ${data.software} ${data.version}.`}
        />
        <ModsPanel profileId={data.id} />
      </div>
    );
  }

  const options = software.data?.software ?? [];
  const selected = options.find((entry) => entry.id === current.softwareId);
  const versions = selected?.versions.map((entry) => ({ value: entry.version, label: entry.version })) ?? [];
  const kindLabel = selected?.kind === 'mods' ? 'Mods' : selected?.kind === 'plugins' ? 'Plugins' : 'Vanilla';

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <SectionHeading
        title='Configuration'
        icon={Box}
        description={
          <span className='flex flex-wrap items-center gap-2'>
            <span>
              {data.software} {data.version}
            </span>
            <Badge variant='outline'>{kindLabel}</Badge>
            {data.instanceCount > 0 ? (
              <Badge variant='secondary'>{data.instanceCount} in use</Badge>
            ) : (
              <Badge className='bg-main/15 text-main'>free</Badge>
            )}
          </span>
        }
        action={
          <>
            {dirtyCount > 0 ? (
              <Button variant='ghost' size='sm' onClick={() => setDraft(null)}>
                <Undo2 />
                Discard {dirtyCount}
              </Button>
            ) : null}
            {onLaunch ? (
              <Button variant='outline' size='sm' onClick={onLaunch}>
                <Play />
                Launch a world with this
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
        }
      />

      <StatStrip
        items={[
          {
            label: 'Heap',
            value: `${current.memory.minMb}–${current.memory.maxMb} MB`,
            hint: `initial ${current.memory.initialMb} MB`,
          },
          {
            label: 'CPU',
            value: current.cpuCores === null ? 'Unbounded' : `${current.cpuCores} cores`,
            hint: 'container limit',
          },
          {
            label: 'Servers',
            value: String(data.instanceCount),
            hint: data.instanceCount > 0 ? 'sharing this configuration' : 'available',
          },
          {
            label: 'Updated',
            value: formatRelative(data.updatedAt),
          },
        ]}
      />

      <div className='grid grid-cols-1 gap-4 xl:grid-cols-2'>
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Settings2 className='size-4' /> Software
            </CardTitle>
            <CardDescription>What runs inside every server using this configuration.</CardDescription>
          </CardHeader>
          <CardContent className='space-y-4'>
            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              <div className='space-y-1.5'>
                <label htmlFor='profile-software' className='text-sm font-medium'>
                  Software
                </label>
                <Select
                  value={current.softwareId}
                  items={options.map((entry) => ({ value: entry.id, label: entry.label }))}
                  onValueChange={(value) => {
                    if (value === null) return;
                    const id = value as ServerSoftwareId;
                    const option = options.find((entry) => entry.id === id);
                    patch({ softwareId: id, version: option?.versions[0]?.version ?? current.version });
                  }}
                >
                  <SelectTrigger id='profile-software' className='w-full'>
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
              </div>
              <div className='space-y-1.5'>
                <label htmlFor='profile-version' className='text-sm font-medium'>
                  Version
                </label>
                <Select
                  value={current.version}
                  items={versions}
                  onValueChange={(value) => {
                    if (value !== null) patch({ version: value });
                  }}
                >
                  <SelectTrigger id='profile-version' className='w-full'>
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
              </div>
            </div>

            <KeyValue label='Container image' value={<code className='text-xs'>{data.image}</code>} />

            <div className='space-y-1.5'>
              <label htmlFor='profile-description' className='text-sm font-medium'>
                Description
              </label>
              <Textarea
                id='profile-description'
                rows={2}
                value={current.description}
                onChange={(event) => patch({ description: event.target.value })}
              />
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <MemoryStick className='size-4' /> Resources
            </CardTitle>
            <CardDescription>Heap and CPU limits the container is created with.</CardDescription>
          </CardHeader>
          <CardContent className='space-y-4'>
            <div className='grid grid-cols-1 gap-4 sm:grid-cols-3'>
              <div className='space-y-1.5'>
                <label htmlFor='profile-min' className='text-sm font-medium'>
                  Min heap
                </label>
                <div className='flex items-center gap-2'>
                  <Input
                    id='profile-min'
                    type='number'
                    value={current.memory.minMb}
                    onChange={(event) =>
                      patch({ memory: { ...current.memory, minMb: Number(event.target.value) } })
                    }
                    className='tabular-nums'
                  />
                  <span className='text-xs text-muted-foreground'>MB</span>
                </div>
              </div>
              <div className='space-y-1.5'>
                <label htmlFor='profile-max' className='text-sm font-medium'>
                  Max heap
                </label>
                <div className='flex items-center gap-2'>
                  <Input
                    id='profile-max'
                    type='number'
                    value={current.memory.maxMb}
                    onChange={(event) =>
                      patch({ memory: { ...current.memory, maxMb: Number(event.target.value) } })
                    }
                    className='tabular-nums'
                  />
                  <span className='text-xs text-muted-foreground'>MB</span>
                </div>
              </div>
              <div className='space-y-1.5'>
                <label htmlFor='profile-initial' className='text-sm font-medium'>
                  Initial (-Xms)
                </label>
                <div className='flex items-center gap-2'>
                  <Input
                    id='profile-initial'
                    type='number'
                    value={current.memory.initialMb}
                    onChange={(event) =>
                      patch({ memory: { ...current.memory, initialMb: Number(event.target.value) } })
                    }
                    className='tabular-nums'
                  />
                  <span className='text-xs text-muted-foreground'>MB</span>
                </div>
              </div>
            </div>

            <div className='space-y-1.5'>
              <label htmlFor='profile-cpu' className='text-sm font-medium'>
                CPU limit
              </label>
              <Select
                value={current.cpuCores === null ? 'unbounded' : String(current.cpuCores)}
                items={CPU_OPTIONS}
                onValueChange={(value) => {
                  if (value === null) return;
                  patch({ cpuCores: value === 'unbounded' ? null : Number(value) });
                }}
              >
                <SelectTrigger id='profile-cpu' className='w-full'>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {CPU_OPTIONS.map((entry) => (
                    <SelectItem key={entry.value} value={entry.value}>
                      {entry.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className='flex items-center gap-1.5 text-xs text-muted-foreground'>
                <Cpu className='size-3.5' />
                Docker `--cpus` applied to every container built from this configuration.
              </p>
            </div>

            <div className='space-y-1.5'>
              <label htmlFor='profile-java' className='text-sm font-medium'>
                Java arguments
              </label>
              <Input
                id='profile-java'
                value={current.javaArgs}
                className='font-mono text-xs'
                onChange={(event) => patch({ javaArgs: event.target.value })}
              />
              <p className='text-xs text-muted-foreground'>
                Prepended to the JVM command; the heap flags above are appended automatically.
              </p>
            </div>

            <Separator />

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              <div className='space-y-1.5'>
                <label htmlFor='profile-rcon' className='text-sm font-medium'>
                  Default RCON port
                </label>
                <Input
                  id='profile-rcon'
                  type='number'
                  value={current.ports.rcon}
                  onChange={(event) =>
                    patch({ ports: { ...current.ports, rcon: Number(event.target.value) } })
                  }
                  className='tabular-nums'
                />
              </div>
              <div className='space-y-1.5'>
                <label htmlFor='profile-query' className='text-sm font-medium'>
                  Default query port
                </label>
                <Input
                  id='profile-query'
                  type='number'
                  value={current.ports.query ?? ''}
                  placeholder='disabled'
                  onChange={(event) =>
                    patch({
                      ports: {
                        ...current.ports,
                        query: event.target.value === '' ? null : Number(event.target.value),
                      },
                    })
                  }
                  className='tabular-nums'
                />
              </div>
            </div>
            <p className='text-xs text-muted-foreground'>
              The Minecraft port is chosen per server so several containers can share one configuration.
            </p>
          </CardContent>
        </Card>

        <Card className='xl:col-span-2'>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Puzzle className='size-4' /> Supervision
            </CardTitle>
            <CardDescription>How Lodestone keeps every server honest.</CardDescription>
          </CardHeader>
          <CardContent className='grid grid-cols-1 gap-4 sm:grid-cols-3'>
            <div className='flex items-center justify-between gap-4 rounded-md border p-3'>
              <div className='min-w-0'>
                <p className='text-sm font-medium'>Autostart</p>
                <p className='text-xs text-muted-foreground'>Start with Lodestone</p>
              </div>
              <Switch
                checked={current.autoStart}
                aria-label='Autostart'
                onCheckedChange={(checked) => patch({ autoStart: checked })}
              />
            </div>
            <div className='flex items-center justify-between gap-4 rounded-md border p-3'>
              <div className='min-w-0'>
                <p className='text-sm font-medium'>Restart on crash</p>
                <p className='text-xs text-muted-foreground'>Recover a failed server</p>
              </div>
              <Switch
                checked={current.restartOnCrash}
                aria-label='Restart on crash'
                onCheckedChange={(checked) => patch({ restartOnCrash: checked })}
              />
            </div>
            <div className='space-y-1.5 rounded-md border p-3'>
              <label htmlFor='profile-crash-threshold' className='text-sm font-medium'>
                Crash threshold
              </label>
              <Input
                id='profile-crash-threshold'
                type='number'
                min={1}
                value={current.crashThreshold}
                onChange={(event) => patch({ crashThreshold: Number(event.target.value) })}
                className='tabular-nums'
              />
              <p className='text-xs text-muted-foreground'>Give up after this many crashes in an hour.</p>
            </div>
          </CardContent>
        </Card>

        {onOpenInstance ? (
          <AssociatedServersCard profileId={data.id} onOpenInstance={onOpenInstance} onLaunch={onLaunch} />
        ) : null}
      </div>
    </div>
  );
}

/**
 * Servers currently built on this configuration. Because a configuration is
 * reusable, this is often more than one — the card is how you get from a
 * configuration back to the servers that run it.
 */
function AssociatedServersCard({
  profileId,
  onOpenInstance,
  onLaunch,
}: {
  profileId: string;
  onOpenInstance: (instanceId: string) => void;
  onLaunch?: () => void;
}) {
  const instances = useInstances();
  const servers = (instances.data ?? []).filter((instance) => instance.profileId === profileId);

  return (
    <Card className='xl:col-span-2'>
      <CardHeader className='flex-row items-center justify-between gap-3'>
        <div className='min-w-0'>
          <CardTitle className='flex items-center gap-2'>
            <Server className='size-4' /> Servers using this configuration
          </CardTitle>
          <CardDescription>Deleting a server leaves this configuration intact.</CardDescription>
        </div>
        {onLaunch ? (
          <Button variant='outline' size='sm' onClick={onLaunch}>
            <Play />
            Launch a world with this
          </Button>
        ) : null}
      </CardHeader>
      <CardContent>
        {servers.length === 0 ? (
          <EmptyState
            icon={Server}
            title='No servers yet'
            description='Launch a world with this configuration to create one.'
          />
        ) : (
          <div className='flex flex-wrap gap-2'>
            {servers.map((instance) => (
              <button
                key={instance.id}
                type='button'
                className='flex items-center gap-2 rounded-md border px-3 py-2 text-sm hover:bg-muted/60'
                onClick={() => onOpenInstance(instance.id)}
              >
                <ServerStateDot state={instance.state} className='size-2.5' />
                <span className='font-medium'>{instance.name}</span>
                <span className='text-xs text-muted-foreground'>{instance.world.name}</span>
              </button>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
