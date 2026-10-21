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
import { Textarea } from '@sunstead/ui/components/textarea';
import {
  useCreateProfile,
  useDeleteProfile,
  useDuplicateProfile,
  useProfiles,
  useSoftware,
} from '#/api/queries';
import type { CreateProfileRequest, Profile, ServerSoftwareId } from '#/api/types';
import { EmptyState, SectionHeading } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatRelative } from '#/lib/format';
import {
  Box,
  Copy,
  Cpu,
  MemoryStick,
  Pencil,
  Plus,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import { useMemo, useState } from 'react';

const EMPTY_DRAFT: CreateProfileRequest = {
  name: '',
  description: '',
  softwareId: 'paper',
  version: '1.21.4',
  memory: { minMb: 2048, maxMb: 4096, initialMb: 2048 },
  cpuCores: null,
  javaArgs: '-XX:+UseG1GC -XX:MaxGCPauseMillis=200',
  autoStart: true,
  restartOnCrash: true,
  crashThreshold: 3,
};

function ProfileDialog({
  open,
  onOpenChange,
  profile,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  profile?: Profile;
  onCreated?: (profile: Profile) => void;
}) {
  const software = useSoftware();
  const create = useCreateProfile();
  const [draft, setDraft] = useState<CreateProfileRequest>(EMPTY_DRAFT);

  const options = software.data?.software ?? [];
  const editing = Boolean(profile);

  const reset = () => {
    setDraft(
      profile
        ? {
            name: profile.name,
            description: profile.description,
            softwareId: profile.softwareId,
            version: profile.version,
            memory: profile.memory,
            cpuCores: profile.cpuCores,
            javaArgs: profile.javaArgs,
            autoStart: profile.autoStart,
            restartOnCrash: profile.restartOnCrash,
            crashThreshold: profile.crashThreshold,
          }
        : EMPTY_DRAFT,
    );
  };

  const selected = options.find((entry) => entry.id === draft.softwareId);
  const versions = selected?.versions.map((entry) => ({ value: entry.version, label: entry.version })) ?? [];

  const pickSoftware = (id: ServerSoftwareId) => {
    const option = options.find((entry) => entry.id === id);
    const version = option?.versions[0]?.version ?? draft.version;
    setDraft((previous) => ({ ...previous, softwareId: id, version }));
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
          <DialogTitle>{editing ? 'New profile from this one' : 'Create profile'}</DialogTitle>
          <DialogDescription>
            A profile carries the software, version, mods, properties and resource limits. Many
            instances can run the same profile.
          </DialogDescription>
        </DialogHeader>

        <div className='max-h-[60vh] space-y-4 overflow-y-auto pr-1'>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor='profile-name'>Name</FieldLabel>
              <Input
                id='profile-name'
                value={draft.name}
                placeholder='Paper 1.21 performance'
                onChange={(event) => setDraft((previous) => ({ ...previous, name: event.target.value }))}
              />
            </Field>

            <Field>
              <FieldLabel htmlFor='profile-description'>Description</FieldLabel>
              <Textarea
                id='profile-description'
                rows={2}
                value={draft.description}
                placeholder='What this profile is tuned for'
                onChange={(event) =>
                  setDraft((previous) => ({ ...previous, description: event.target.value }))
                }
              />
            </Field>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              <Field>
                <FieldLabel>Software</FieldLabel>
                <Select
                  value={draft.softwareId}
                  items={options.map((entry) => ({ value: entry.id, label: entry.label }))}
                  onValueChange={(value) => pickSoftware(value as ServerSoftwareId)}
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
                {selected ? <FieldDescription>{selected.blurb}</FieldDescription> : null}
              </Field>

              <Field>
                <FieldLabel>Version</FieldLabel>
                <Select
                  value={draft.version}
                  items={versions}
                  onValueChange={(value) => {
                    if (value === null) return;
                    setDraft((previous) => ({ ...previous, version: value }));
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
                <FieldLabel htmlFor='profile-min-memory'>Minimum heap (MB)</FieldLabel>
                <Input
                  id='profile-min-memory'
                  type='number'
                  value={draft.memory.minMb}
                  onChange={(event) =>
                    setDraft((previous) => ({
                      ...previous,
                      memory: { ...previous.memory, minMb: Number(event.target.value) },
                    }))
                  }
                  className='tabular-nums'
                />
              </Field>
              <Field>
                <FieldLabel htmlFor='profile-max-memory'>Maximum heap (MB)</FieldLabel>
                <Input
                  id='profile-max-memory'
                  type='number'
                  value={draft.memory.maxMb}
                  onChange={(event) =>
                    setDraft((previous) => ({
                      ...previous,
                      memory: { ...previous.memory, maxMb: Number(event.target.value) },
                    }))
                  }
                  className='tabular-nums'
                />
              </Field>
            </div>

            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              <Field orientation='horizontal'>
                <FieldLabel htmlFor='profile-autostart' className='flex-1'>
                  Start with Lodestone
                </FieldLabel>
                <Switch
                  id='profile-autostart'
                  checked={draft.autoStart}
                  onCheckedChange={(checked) =>
                    setDraft((previous) => ({ ...previous, autoStart: checked }))
                  }
                />
              </Field>
              <Field orientation='horizontal'>
                <FieldLabel htmlFor='profile-restart' className='flex-1'>
                  Restart on crash
                </FieldLabel>
                <Switch
                  id='profile-restart'
                  checked={draft.restartOnCrash}
                  onCheckedChange={(checked) =>
                    setDraft((previous) => ({ ...previous, restartOnCrash: checked }))
                  }
                />
              </Field>
            </div>
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
            {create.isPending ? 'Creating…' : 'Create profile'}
          </BevelButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ProfilesPage({
  onOpenProfile,
}: {
  onOpenProfile: (profileId: string) => void;
}) {
  const profiles = useProfiles();
  const duplicate = useDuplicateProfile();
  const remove = useDeleteProfile();
  const [createOpen, setCreateOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<Profile | null>(null);

  const list = profiles.data ?? [];
  const attached = useMemo(() => list.filter((profile) => profile.instanceCount > 0).length, [list]);

  if (profiles.isLoading) {
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
        title='Profiles'
        icon={Box}
        description={`${list.length} profile${list.length === 1 ? '' : 's'} · ${attached} in use by an instance`}
        action={
          <BevelButton className='px-5 py-5 text-base' onClick={() => setCreateOpen(true)}>
            <Plus />
            Create profile
          </BevelButton>
        }
      />

      {list.length === 0 ? (
        <Card>
          <CardContent className='p-6'>
            <EmptyState
              icon={Box}
              title='No profiles yet'
              description='A profile defines the software, mods and limits. Create one, then pair it with a world to make an instance.'
            />
          </CardContent>
        </Card>
      ) : (
        <Card className='gap-0 overflow-hidden py-0'>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Profile</TableHead>
                <TableHead>Software</TableHead>
                <TableHead className='hidden sm:table-cell'>Resources</TableHead>
                <TableHead className='hidden lg:table-cell'>Supervision</TableHead>
                <TableHead className='text-right'>Instances</TableHead>
                <TableHead className='text-right'>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.map((profile) => (
                <TableRow key={profile.id}>
                  <TableCell>
                    <div className='flex items-center gap-2'>
                      <button
                        type='button'
                        className='truncate text-left font-medium hover:underline'
                        onClick={() => onOpenProfile(profile.id)}
                      >
                        {profile.name}
                      </button>
                      {profile.instanceCount > 0 ? (
                        <Badge variant='secondary'>{profile.instanceCount} attached</Badge>
                      ) : (
                        <Badge className='bg-main/15 text-main'>free</Badge>
                      )}
                    </div>
                    <p className='mt-0.5 truncate text-xs text-muted-foreground'>
                      {profile.description || `Updated ${formatRelative(profile.updatedAt)}`}
                    </p>
                  </TableCell>
                  <TableCell>
                    <div className='flex items-center gap-2'>
                      <span className='text-sm font-medium'>
                        {profile.software} {profile.version}
                      </span>
                    </div>
                    <code className='mt-0.5 block text-[11px] text-muted-foreground'>{profile.image}</code>
                  </TableCell>
                  <TableCell className='hidden sm:table-cell'>
                    <span className='flex items-center gap-1.5 text-sm text-muted-foreground'>
                      <MemoryStick className='size-3.5' />
                      {profile.memory.minMb}–{profile.memory.maxMb} MB
                    </span>
                    <span className='mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground'>
                      <Cpu className='size-3.5' />
                      {profile.cpuCores === null ? 'unbounded CPU' : `${profile.cpuCores} cores`}
                    </span>
                  </TableCell>
                  <TableCell className='hidden lg:table-cell text-sm text-muted-foreground'>
                    {profile.autoStart ? 'autostart' : 'manual start'}
                    {profile.restartOnCrash ? ' · restarts' : ''}
                  </TableCell>
                  <TableCell className='text-right tabular-nums text-muted-foreground'>
                    {profile.instanceCount}
                  </TableCell>
                  <TableCell>
                    <div className='flex items-center justify-end gap-1'>
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Edit ${profile.name}`}
                        onClick={() => onOpenProfile(profile.id)}
                      >
                        <Pencil />
                      </Button>
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Duplicate ${profile.name}`}
                        disabled={duplicate.isPending}
                        onClick={() => duplicate.mutate(profile.id)}
                      >
                        <Copy />
                      </Button>
                      <Button
                        variant='ghost'
                        size='icon-sm'
                        aria-label={`Delete ${profile.name}`}
                        disabled={remove.isPending || profile.instanceCount > 0}
                        onClick={() => setPendingDelete(profile)}
                        className='text-destructive hover:text-destructive'
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}

      <ProfileDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(created) => onOpenProfile(created.id)}
      />

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => (open ? undefined : setPendingDelete(null))}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this profile?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingDelete
                ? `${pendingDelete.name} will be removed, along with the mods, properties and limits it holds. Worlds it ran are kept untouched.`
                : ''}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm'>
            <TriangleAlert className='mt-0.5 size-4 shrink-0 text-destructive' />
            <p>Any world you used with this profile stays on disk and can be attached to another profile.</p>
          </div>
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
              Delete profile
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

export { ProfileDialog };
