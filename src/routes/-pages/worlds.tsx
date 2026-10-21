import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@sunstead/ui/components/alert-dialog';
import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
} from '@sunstead/ui/components/card';
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
import { Skeleton } from '@sunstead/ui/components/skeleton';
import { Switch } from '@sunstead/ui/components/switch';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@sunstead/ui/components/table';
import {
  useCreateWorld,
  useDeleteWorld,
  useDuplicateWorld,
  useInstances,
  useWorlds,
} from '#/api/queries';
import type { CreateWorldRequest, Difficulty, GameMode, Instance, LevelType, World } from '#/api/types';
import { EmptyState, ScrollTable, SectionHeading } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatRelative } from '#/lib/format';
import {
  Boxes,
  Copy,
  Layers,
  Lock,
  Pencil,
  Play,
  Plus,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import { useMemo, useState } from 'react';

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

const EMPTY_DRAFT: CreateWorldRequest = {
  name: '',
  levelType: 'minecraft:normal',
  seed: null,
  generateStructures: true,
  difficulty: 'normal',
  gamemode: 'survival',
  hardcore: false,
  maxWorldSize: 29_999_984,
  allowNether: true,
};

function levelTypeLabel(value: LevelType): string {
  return LEVEL_TYPES.find((entry) => entry.value === value)?.label ?? value;
}

/**
 * Create/edit dialog for a world. Generation-only fields lock once the
 * world has generated, because changing them does nothing on disk.
 */
export function WorldDialog({
  open,
  onOpenChange,
  world,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  world?: World;
  onCreated?: (world: World) => void;
}) {
  const create = useCreateWorld();
  const [draft, setDraft] = useState<CreateWorldRequest>(EMPTY_DRAFT);

  const editing = Boolean(world);
  const locked = world?.generated ?? false;

  const reset = () => {
    setDraft(
      world
        ? {
            name: world.name,
            levelType: world.levelType,
            seed: world.seed,
            generateStructures: world.generateStructures,
            difficulty: world.difficulty,
            gamemode: world.gamemode,
            hardcore: world.hardcore,
            maxWorldSize: world.maxWorldSize,
            allowNether: world.allowNether,
          }
        : EMPTY_DRAFT,
    );
  };

  const submit = () => {
    if (!draft.name.trim()) return;
    create.mutate(draft, {
      onSuccess: (created) => {
        onOpenChange(false);
        onCreated?.(created);
      },
    });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) reset();
        onOpenChange(next);
      }}
    >
      <DialogContent className='sm:max-w-lg'>
        <DialogHeader>
          <DialogTitle>{editing ? 'New world from this one' : 'Create world'}</DialogTitle>
          <DialogDescription>
            A world is a standalone asset. It can be mounted by one server at a time and keeps its
            region data, gameplay rules and backups when no server is running.
          </DialogDescription>
        </DialogHeader>

        <div className='max-h-[60vh] space-y-4 overflow-y-auto pr-1'>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor='world-name'>Name</FieldLabel>
              <Input
                id='world-name'
                value={draft.name}
                placeholder='Survival Season 2'
                onChange={(event) => setDraft((previous) => ({ ...previous, name: event.target.value }))}
              />
              <FieldDescription>The folder on disk is derived from this name.</FieldDescription>
            </Field>

            <Field>
              <FieldLabel>Level type</FieldLabel>
              <Select
                value={draft.levelType}
                items={LEVEL_TYPES}
                disabled={locked}
                onValueChange={(value) =>
                  setDraft((previous) => ({ ...previous, levelType: value as LevelType }))
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
              {locked ? (
                <FieldDescription className='flex items-center gap-1.5'>
                  <Lock className='size-3' />
                  Fixed — this world has already generated terrain.
                </FieldDescription>
              ) : null}
            </Field>

            <Field>
              <FieldLabel htmlFor='world-seed'>Seed</FieldLabel>
              <Input
                id='world-seed'
                value={draft.seed ?? ''}
                placeholder='Leave blank for a random seed'
                disabled={locked}
                onChange={(event) =>
                  setDraft((previous) => ({ ...previous, seed: event.target.value || null }))
                }
              />
            </Field>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              <Field>
                <FieldLabel>Difficulty</FieldLabel>
                <Select
                  value={draft.difficulty}
                  items={DIFFICULTIES}
                  onValueChange={(value) =>
                    setDraft((previous) => ({ ...previous, difficulty: value as Difficulty }))
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
                <FieldLabel>Default gamemode</FieldLabel>
                <Select
                  value={draft.gamemode}
                  items={GAMEMODES}
                  onValueChange={(value) =>
                    setDraft((previous) => ({ ...previous, gamemode: value as GameMode }))
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

            <Field orientation='horizontal'>
              <FieldLabel htmlFor='world-structures' className='flex-1'>
                Generate structures
              </FieldLabel>
              <Switch
                id='world-structures'
                checked={draft.generateStructures}
                disabled={locked}
                onCheckedChange={(checked) =>
                  setDraft((previous) => ({ ...previous, generateStructures: checked }))
                }
              />
            </Field>

            <Field orientation='horizontal'>
              <FieldLabel htmlFor='world-nether' className='flex-1'>
                Allow nether
              </FieldLabel>
              <Switch
                id='world-nether'
                checked={draft.allowNether}
                onCheckedChange={(checked) =>
                  setDraft((previous) => ({ ...previous, allowNether: checked }))
                }
              />
            </Field>

            <Field orientation='horizontal'>
              <FieldLabel htmlFor='world-hardcore' className='flex-1'>
                Hardcore
              </FieldLabel>
              <Switch
                id='world-hardcore'
                checked={draft.hardcore}
                onCheckedChange={(checked) =>
                  setDraft((previous) => ({ ...previous, hardcore: checked }))
                }
              />
            </Field>

            <Field>
              <FieldLabel htmlFor='world-size'>Max world size</FieldLabel>
              <Input
                id='world-size'
                type='number'
                value={draft.maxWorldSize}
                onChange={(event) =>
                  setDraft((previous) => ({ ...previous, maxWorldSize: Number(event.target.value) }))
                }
              />
              <FieldDescription>Border radius in blocks. Vanilla default is 29,999,984.</FieldDescription>
            </Field>
          </FieldGroup>
        </div>

        <DialogFooter>
          <Button variant='outline' onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <BevelButton
            className='px-5 py-4 text-sm'
            disabled={!draft.name.trim() || create.isPending}
            onClick={submit}
          >
            <Plus />
            {create.isPending ? 'Creating…' : 'Create world'}
          </BevelButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function WorldsPage({
  onOpenWorld,
  onOpenInstance,
  onLaunchWorld,
}: {
  onOpenWorld: (worldId: string) => void;
  onOpenInstance?: (instanceId: string) => void;
  onLaunchWorld?: (worldId: string) => void;
}) {
  const worlds = useWorlds();
  const instances = useInstances();
  const duplicate = useDuplicateWorld();
  const remove = useDeleteWorld();
  const [createOpen, setCreateOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<World | null>(null);

  const list = worlds.data ?? [];
  const serverById = useMemo(() => {
    const map = new Map<string, Instance>();
    for (const instance of instances.data ?? []) map.set(instance.id, instance);
    return map;
  }, [instances.data]);

  const totals = useMemo(
    () => ({
      attached: list.filter((world) => world.attachedInstanceId !== null).length,
      bytes: list.reduce((total, world) => total + world.sizeBytes, 0),
    }),
    [list],
  );

  if (worlds.isLoading) {
    return (
      <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
        <Skeleton className='h-10 w-64' />
        <Skeleton className='h-96 w-full' />
      </div>
    );
  }

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <SectionHeading
        title='Worlds'
        icon={Layers}
        description={`${list.length} world${list.length === 1 ? '' : 's'} · ${totals.attached} mounted · ${formatBytes(totals.bytes, 1)} on disk`}
        action={
          <BevelButton className='px-5 py-5 text-base' onClick={() => setCreateOpen(true)}>
            <Plus />
            Create world
          </BevelButton>
        }
      />

      {list.length === 0 ? (
        <Card>
          <CardContent className='p-6'>
            <EmptyState
              icon={Layers}
              title='No worlds yet'
              description='A world holds the terrain, gameplay rules and backups. Create one, then launch it to run a server.'
            />
          </CardContent>
        </Card>
      ) : (
        <Card className='gap-0 overflow-hidden py-0'>
          <ScrollTable>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>World</TableHead>
                <TableHead>Folder</TableHead>
                <TableHead className='hidden sm:table-cell'>Server</TableHead>
                <TableHead className='hidden lg:table-cell'>Gameplay</TableHead>
                <TableHead className='text-right'>Size</TableHead>
                <TableHead className='text-right'>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.map((world) => {
                const server = world.attachedInstanceId
                  ? serverById.get(world.attachedInstanceId)
                  : undefined;
                return (
                <TableRow key={world.id}>
                  <TableCell>
                    <div className='flex items-center gap-2'>
                      <button
                        type='button'
                        className='truncate text-left font-medium hover:underline'
                        onClick={() => onOpenWorld(world.id)}
                      >
                        {world.name}
                      </button>
                      {world.attachedInstanceId ? (
                        <Badge variant='secondary' className='gap-1'>
                          <Boxes className='size-3' />
                          mounted
                        </Badge>
                      ) : (
                        <Badge className='bg-main/15 text-main'>free</Badge>
                      )}
                      {world.hardcore ? <Badge variant='destructive'>hardcore</Badge> : null}
                    </div>
                    <p className='mt-0.5 text-xs text-muted-foreground'>
                      Updated {formatRelative(world.updatedAt)}
                    </p>
                  </TableCell>
                  <TableCell>
                    <code className='text-xs text-muted-foreground'>{world.folder}</code>
                  </TableCell>
                  <TableCell className='hidden sm:table-cell'>
                    {server ? (
                      onOpenInstance ? (
                        <button
                          type='button'
                          className='truncate text-left text-sm font-medium hover:underline'
                          onClick={() => onOpenInstance(server.id)}
                        >
                          {server.name}
                        </button>
                      ) : (
                        <span className='text-sm'>{server.name}</span>
                      )
                    ) : onLaunchWorld ? (
                      <Button
                        variant='outline'
                        size='sm'
                        onClick={() => onLaunchWorld(world.id)}
                      >
                        <Play />
                        Launch
                      </Button>
                    ) : (
                      <span className='text-sm text-muted-foreground'>not running</span>
                    )}
                  </TableCell>
                  <TableCell className='hidden lg:table-cell text-sm text-muted-foreground capitalize'>
                    {world.gamemode} · {world.difficulty}
                    {world.pvp ? ' · pvp' : ''}
                  </TableCell>
                  <TableCell className='text-right tabular-nums text-muted-foreground'>
                    {formatBytes(world.sizeBytes, 1)}
                  </TableCell>
                  <TableCell>
                    <div className='flex items-center justify-end gap-1'>
                      {onLaunchWorld && !world.attachedInstanceId ? (
                        <Button
                          variant='ghost'
                          size='sm'
                          onClick={() => onLaunchWorld(world.id)}
                        >
                          <Play />
                          Launch
                        </Button>
                      ) : null}
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Open ${world.name}`}
                        onClick={() => onOpenWorld(world.id)}
                      >
                        <Pencil />
                      </Button>
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Duplicate ${world.name}`}
                        disabled={duplicate.isPending}
                        onClick={() => duplicate.mutate(world.id)}
                      >
                        <Copy />
                      </Button>
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Delete ${world.name}`}
                        disabled={remove.isPending || world.attachedInstanceId !== null}
                        title={
                          world.attachedInstanceId
                            ? 'Detach this world from its server first'
                            : undefined
                        }
                        onClick={() => setPendingDelete(world)}
                        className='text-destructive hover:text-destructive'
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
                );
              })}
            </TableBody>
          </Table>
          </ScrollTable>
        </Card>
      )}

      <WorldDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(created) => onOpenWorld(created.id)}
      />

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => (open ? undefined : setPendingDelete(null))}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this world?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingDelete
                ? `${pendingDelete.name} and its ${formatBytes(pendingDelete.sizeBytes, 1)} of region data will be removed from disk. Its backups are deleted with it.`
                : ''}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {pendingDelete?.attachedInstanceId ? (
            <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm'>
              <TriangleAlert className='mt-0.5 size-4 shrink-0 text-destructive' />
              <p>A server currently has this world mounted. Stop and detach it first.</p>
            </div>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel render={<Button variant='outline' />}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              render={
                <BevelButton
                  tone='danger'
                  className='px-4 py-4 text-sm'
                  onClick={() => {
                    if (pendingDelete) remove.mutate(pendingDelete.id);
                    setPendingDelete(null);
                  }}
                />
              }
            >
              <Trash2 />
              Delete world
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

export { GAMEMODES, DIFFICULTIES, LEVEL_TYPES, levelTypeLabel };
