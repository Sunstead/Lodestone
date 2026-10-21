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
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Field, FieldDescription, FieldLabel } from '@sunstead/ui/components/field';
import { Input } from '@sunstead/ui/components/input';
import { Progress } from '@sunstead/ui/components/progress';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@sunstead/ui/components/select';
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
  useBackups,
  useCreateBackup,
  useDeleteBackup,
  usePatchSchedule,
  useRestoreBackup,
} from '#/api/queries';
import type { Backup } from '#/api/types';
import { EmptyState, KeyValue, ScrollTable, StatStrip } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatRelative, formatWhen, percent } from '#/lib/format';
import {
  ArchiveRestore,
  CalendarClock,
  Clock,
  Container,
  Database,
  HardDriveDownload,
  History,
  Loader2,
  Play,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import { useState } from 'react';

const INTERVALS = [
  { value: '30', label: 'Every 30 minutes' },
  { value: '60', label: 'Every hour' },
  { value: '180', label: 'Every 3 hours' },
  { value: '360', label: 'Every 6 hours' },
  { value: '720', label: 'Every 12 hours' },
  { value: '1440', label: 'Daily' },
  { value: '10080', label: 'Weekly' },
];

const STATUS_BADGE: Record<Backup['status'], { label: string; variant: 'default' | 'secondary' | 'destructive' }> = {
  complete: { label: 'Complete', variant: 'secondary' },
  running: { label: 'Running', variant: 'default' },
  failed: { label: 'Failed', variant: 'destructive' },
};

/**
 * Backups belong to a world, not to a container. Deleting every server
 * that ran a world leaves these archives untouched.
 */
export function BackupsPanel({ worldId, worldName }: { worldId: string; worldName: string }) {
  const query = useBackups(worldId);
  const create = useCreateBackup(worldId);
  const remove = useDeleteBackup(worldId);
  const restore = useRestoreBackup(worldId);
  const schedule = usePatchSchedule(worldId);
  const [pendingRestore, setPendingRestore] = useState<Backup | null>(null);

  const backups = query.data?.backups ?? [];
  const settings = query.data?.schedule;
  const storage = query.data?.storage;
  const running = backups.find((backup) => backup.status === 'running');

  return (
    <div className='flex w-full min-w-0 flex-col gap-4'>
      <StatStrip
        items={[
          {
            label: 'Snapshots',
            value: String(backups.length),
            hint: `${backups.filter((backup) => backup.kind === 'manual').length} manual`,
          },
          {
            label: 'Latest',
            value: backups[0] ? formatRelative(backups[0].createdAt) : '—',
            hint: backups[0] ? formatBytes(backups[0].sizeBytes) : undefined,
          },
          {
            label: 'Next run',
            value: settings?.enabled ? formatRelative(settings.nextRunAt) : 'Paused',
            hint: settings?.enabled ? `every ${Math.round((settings.intervalMinutes / 60) * 10) / 10}h` : 'schedule disabled',
          },
          {
            label: 'Storage used',
            value: storage ? formatBytes(storage.usedBytes, 1) : '—',
            hint: storage ? `of ${formatBytes(storage.quotaBytes, 0)} quota` : undefined,
          },
        ]}
      />

      <div className='grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]'>
        <Card className='gap-0 overflow-hidden py-0'>
          <CardHeader className='flex-row items-center justify-between gap-3 border-b bg-muted/30 px-4 py-3'>
            <div className='min-w-0'>
              <CardTitle className='flex items-center gap-2 text-base'>
                <History className='size-4' /> Snapshots of {worldName}
              </CardTitle>
              <CardDescription>
                Each snapshot archives this world's region data and stays with the world, not the server.
              </CardDescription>
            </div>
            {running ? (
              <span className='flex shrink-0 items-center gap-2 text-xs text-muted-foreground'>
                <Loader2 className='size-3.5 animate-spin' />
                Archiving {formatWhen(running.createdAt)}…
              </span>
            ) : null}
            <BevelButton
              className='shrink-0 px-5 py-5 text-base'
              disabled={create.isPending || Boolean(running)}
              onClick={() => create.mutate()}
            >
              <Database />
              {create.isPending ? 'Starting…' : 'Back up now'}
            </BevelButton>
          </CardHeader>
          {backups.length === 0 ? (
            <CardContent className='p-6'>
              <EmptyState
                icon={Container}
                title='No backups yet'
                description='Take a manual snapshot before your next big change, or turn on the schedule on the right.'
              />
            </CardContent>
          ) : (
            <ScrollTable className='h-[30rem]'>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Kind</TableHead>
                  <TableHead className='hidden sm:table-cell'>Created</TableHead>
                  <TableHead className='text-right'>Size</TableHead>
                  <TableHead className='text-right'>Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {backups.map((backup) => {
                  const status = STATUS_BADGE[backup.status];
                  return (
                    <TableRow key={backup.id}>
                      <TableCell>
                        <div className='flex items-center gap-2'>
                          <span className='font-medium'>{backup.name}</span>
                          <Badge variant={status.variant} className='gap-1'>
                            {backup.status === 'running' ? <Loader2 className='size-3 animate-spin' /> : null}
                            {backup.status === 'failed' ? <TriangleAlert className='size-3' /> : null}
                            {status.label}
                          </Badge>
                        </div>
                        <p className='mt-0.5 font-mono text-[11px] text-muted-foreground'>
                          {backup.id}
                          {backup.instanceId ? ` · taken by ${backup.instanceId}` : ' · manual archive'}
                        </p>
                      </TableCell>
                      <TableCell className='text-muted-foreground capitalize'>{backup.kind}</TableCell>
                      <TableCell className='hidden text-muted-foreground sm:table-cell'>
                        {formatWhen(backup.createdAt)}
                      </TableCell>
                      <TableCell className='text-right tabular-nums text-muted-foreground'>
                        {formatBytes(backup.sizeBytes)}
                      </TableCell>
                      <TableCell>
                        <div className='flex items-center justify-end gap-1'>
                          <Button
                            variant='ghost'
                            size='sm'
                            disabled={backup.status !== 'complete' || restore.isPending}
                            onClick={() => setPendingRestore(backup)}
                          >
                            <ArchiveRestore />
                            Restore
                          </Button>
                          <Button
                            variant='ghost'
                            size='icon-sm'
                            aria-label={`Delete backup ${backup.id}`}
                            disabled={backup.status === 'running' || remove.isPending}
                            onClick={() => remove.mutate(backup.id)}
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
          )}
        </Card>

        <div className='space-y-4'>
          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <CalendarClock className='size-4' /> Schedule
              </CardTitle>
              <CardDescription>Lodestone takes these snapshots on the world's behalf.</CardDescription>
            </CardHeader>
            <CardContent className='space-y-4'>
              <Field orientation='horizontal'>
                <FieldLabel htmlFor='schedule-enabled' className='flex-1'>
                  Automatic backups
                </FieldLabel>
                <Switch
                  id='schedule-enabled'
                  checked={settings?.enabled ?? false}
                  disabled={schedule.isPending}
                  onCheckedChange={(checked) => schedule.mutate({ enabled: checked })}
                />
              </Field>

              <Field>
                <FieldLabel>Interval</FieldLabel>
                <Select
                  value={String(settings?.intervalMinutes ?? 360)}
                  items={INTERVALS}
                  onValueChange={(value) => schedule.mutate({ intervalMinutes: Number(value) })}
                >
                  <SelectTrigger id='interval' className='w-full' disabled={!settings?.enabled || schedule.isPending}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {INTERVALS.map((interval) => (
                      <SelectItem key={interval.value} value={interval.value}>
                        {interval.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <FieldDescription>Measured from the end of the previous run.</FieldDescription>
              </Field>

              <Field>
                <FieldLabel htmlFor='retention'>Keep</FieldLabel>
                <Input
                  id='retention'
                  type='number'
                  min={1}
                  max={365}
                  value={settings?.retention ?? 14}
                  disabled={!settings?.enabled || schedule.isPending}
                  onChange={(event) => schedule.mutate({ retention: Number(event.target.value) })}
                />
                <FieldDescription>Oldest snapshots are pruned once this many exist.</FieldDescription>
              </Field>

              <div className='divide-y border-t pt-2'>
                <KeyValue label='Last run' value={formatWhen(settings?.lastRunAt)} />
                <KeyValue label='Next run' value={settings?.enabled ? formatWhen(settings?.nextRunAt) : 'paused'} />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <HardDriveDownload className='size-4' /> Storage
              </CardTitle>
              <CardDescription>Kept outside any container so a rebuild never touches it.</CardDescription>
            </CardHeader>
            <CardContent className='space-y-3'>
              <Progress value={storage ? percent(storage.usedBytes, storage.quotaBytes) : 0} aria-label='Backup storage used' />
              <p className='text-sm text-muted-foreground'>
                {storage ? `${formatBytes(storage.usedBytes, 1)} of ${formatBytes(storage.quotaBytes, 0)} used` : '—'}
              </p>
              <code className='block break-all rounded-md border bg-muted/40 p-2 text-xs'>{storage?.path}</code>
              <div className='flex items-center gap-2 text-xs text-muted-foreground'>
                <Clock className='size-3.5' />
                Archives are gzipped tar files of this world's folders.
              </div>
            </CardContent>
          </Card>
        </div>
      </div>

      <AlertDialog open={pendingRestore !== null} onOpenChange={(open) => (open ? undefined : setPendingRestore(null))}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Restore this snapshot?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingRestore
                ? `${pendingRestore.name} from ${formatWhen(pendingRestore.createdAt)} will overwrite ${worldName}. If a server has it mounted, the container stops, the archive is unpacked, and the server starts again.`
                : ''}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm'>
            <TriangleAlert className='mt-0.5 size-4 shrink-0 text-destructive' />
            <p>Everything built since this snapshot will be lost. Take a fresh backup first if you are unsure.</p>
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel render={<Button variant='outline' />}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              render={
                <BevelButton
                  tone='danger'
                  className='px-4 py-4 text-sm'
                  onClick={() => {
                    if (pendingRestore) restore.mutate(pendingRestore.id);
                    setPendingRestore(null);
                  }}
                />
              }
            >
              <Play />
              Restore
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
