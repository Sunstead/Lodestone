import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import { Card, CardContent } from '@sunstead/ui/components/card';
import { Skeleton } from '@sunstead/ui/components/skeleton';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@sunstead/ui/components/table';
import { useBackups, useWorlds } from '#/api/queries';
import type { World } from '#/api/types';
import { EmptyState, ScrollTable, SectionHeading } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatBytes, formatWhen } from '#/lib/format';
import { Container, Layers, Loader2 } from 'lucide-react';
import { useMemo } from 'react';

/**
 * Backups are owned by worlds, so this page is a roll-up across every
 * world rather than a separate store. Each row links into the world that
 * holds the archives.
 */
export function AllBackupsPage({
  onOpenWorld,
  onOpenWorlds,
}: {
  onOpenWorld: (worldId: string) => void;
  onOpenWorlds: () => void;
}) {
  const worlds = useWorlds();
  const list = worlds.data ?? [];

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
        title='Backups'
        icon={Container}
        description='Every world keeps its own archives. Open a world to take, restore or schedule snapshots.'
        action={
          <BevelButton className='px-5 py-5 text-base' onClick={onOpenWorlds}>
            <Layers />
            Browse worlds
          </BevelButton>
        }
      />

      {list.length === 0 ? (
        <Card>
          <CardContent className='p-6'>
            <EmptyState
              icon={Container}
              title='No worlds to back up'
              description='Create a world first — its snapshots live beside it and survive any server you attach it to.'
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
                <TableHead className='hidden sm:table-cell'>Folder</TableHead>
                <TableHead className='text-right'>On disk</TableHead>
                <TableHead className='text-right'>Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.map((world) => (
                <BackupRow key={world.id} world={world} onOpenWorld={onOpenWorld} />
              ))}
            </TableBody>
          </Table>
          </ScrollTable>
        </Card>
      )}
    </div>
  );
}

function BackupRow({ world, onOpenWorld }: { world: World; onOpenWorld: (id: string) => void }) {
  const backups = useBackups(world.id);

  const stats = useMemo(() => {
    const entries = backups.data?.backups ?? [];
    const latest = entries[0];
    return {
      count: entries.length,
      running: entries.some((backup) => backup.status === 'running'),
      latest,
      used: backups.data?.storage.usedBytes ?? 0,
    };
  }, [backups.data]);

  return (
    <TableRow>
      <TableCell>
        <div className='flex items-center gap-2'>
          <button
            type='button'
            className='truncate text-left font-medium hover:underline'
            onClick={() => onOpenWorld(world.id)}
          >
            {world.name}
          </button>
          {stats.running ? (
            <Badge className='gap-1'>
              <Loader2 className='size-3 animate-spin' />
              archiving
            </Badge>
          ) : stats.count > 0 ? (
            <Badge variant='secondary'>{stats.count} snapshots</Badge>
          ) : (
            <Badge variant='secondary'>none yet</Badge>
          )}
        </div>
        <p className='mt-0.5 text-xs text-muted-foreground'>
          {stats.latest ? `Latest ${formatWhen(stats.latest.createdAt)}` : 'No snapshot taken yet'}
        </p>
      </TableCell>
      <TableCell className='hidden sm:table-cell'>
        <code className='text-xs text-muted-foreground'>{world.folder}</code>
      </TableCell>
      <TableCell className='text-right tabular-nums text-muted-foreground'>
        <span className='block'>{formatBytes(world.sizeBytes, 1)}</span>
        <span className='text-xs'>{formatBytes(stats.used, 1)} archived</span>
      </TableCell>
      <TableCell>
        <div className='flex justify-end'>
          <Button variant='ghost' size='sm' onClick={() => onOpenWorld(world.id)}>
            <Container />
            Manage backups
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}
