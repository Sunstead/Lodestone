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
import { Input } from '@sunstead/ui/components/input';
import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sunstead/ui/components/tabs';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@sunstead/ui/components/table';
import { usePlayerAction, usePlayers } from '#/api/queries';
import type { Player } from '#/api/types';
import { EmptyState, SectionHeading, StatStrip } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { formatDuration, formatPlaytime, formatPing, formatRelative, pingTone } from '#/lib/format';
import {
  Ban,
  CircleSlash,
  Crown,
  Search,
  ShieldCheck,
  ShieldOff,
  UserGroup,
  UserPlus,
  UserX,
} from 'lucide-react';
import { useMemo, useState } from 'react';

const DIMENSION_LABEL: Record<NonNullable<Player['dimension']>, string> = {
  overworld: 'Overworld',
  nether: 'Nether',
  end: 'The End',
};

function PlayerRow({
  player,
  onAction,
  pending,
}: {
  player: Player;
  onAction: (action: string, reason?: string) => void;
  pending: boolean;
}) {
  const [confirm, setConfirm] = useState<'ban' | 'kick' | null>(null);

  return (
    <TableRow>
      <TableCell>
        <div className='flex items-center gap-2'>
          <Avatar className='size-8 rounded-md'>
            <AvatarFallback className='rounded-md text-xs'>
              {player.name.slice(0, 2).toUpperCase()}
            </AvatarFallback>
          </Avatar>
          <div className='min-w-0'>
            <p className='flex items-center gap-1.5 truncate font-medium'>
              {player.name}
              {player.op ? <Crown aria-label='Operator' className='size-3.5 text-yellow-500' /> : null}
            </p>
            <p className='truncate font-mono text-[11px] text-muted-foreground'>{player.uuid}</p>
          </div>
        </div>
      </TableCell>

      <TableCell>
        <Badge variant={player.online ? 'default' : 'secondary'} className='gap-1.5'>
          <span className={player.online ? 'size-1.5 rounded-full bg-green-400' : 'size-1.5 rounded-full bg-muted-foreground'} />
          {player.online ? 'Online' : 'Offline'}
        </Badge>
      </TableCell>

      <TableCell className='hidden text-muted-foreground md:table-cell'>
        {player.dimension ? DIMENSION_LABEL[player.dimension] : '—'}
      </TableCell>

      <TableCell className={`hidden text-right tabular-nums sm:table-cell ${pingTone(player.pingMs)}`}>
        {formatPing(player.pingMs)}
      </TableCell>

      <TableCell className='text-right tabular-nums text-muted-foreground'>
        {formatPlaytime(player.playtimeSeconds)}
      </TableCell>

      <TableCell className='hidden text-right text-muted-foreground lg:table-cell'>
        {formatRelative(player.lastSeen)}
      </TableCell>

      <TableCell>
        <div className='flex items-center justify-end gap-1'>
          <Button
            variant='ghost'
            size='icon-sm'
            aria-label={player.op ? `Remove operator from ${player.name}` : `Make ${player.name} an operator`}
            disabled={pending}
            onClick={() => onAction(player.op ? 'deop' : 'op')}
          >
            {player.op ? <ShieldOff /> : <ShieldCheck />}
          </Button>
          <Button
            variant='ghost'
            size='icon-sm'
            aria-label={player.whitelisted ? `Remove ${player.name} from the whitelist` : `Add ${player.name} to the whitelist`}
            disabled={pending}
            onClick={() => onAction(player.whitelisted ? 'whitelist-remove' : 'whitelist-add')}
          >
            {player.whitelisted ? <UserX /> : <UserPlus />}
          </Button>
          <Button
            variant='ghost'
            size='icon-sm'
            aria-label={`Kick ${player.name}`}
            disabled={pending || !player.online}
            onClick={() => setConfirm('kick')}
          >
            <CircleSlash />
          </Button>
          <Button
            variant='ghost'
            size='icon-sm'
            aria-label={`Ban ${player.name}`}
            disabled={pending}
            onClick={() => setConfirm('ban')}
            className='text-destructive hover:text-destructive'
          >
            <Ban />
          </Button>
        </div>
      </TableCell>

      <Dialog
        state={confirm}
        player={player}
        onClose={() => setConfirm(null)}
        onConfirm={(reason) => {
          onAction(confirm === 'ban' ? 'ban' : 'kick', reason);
          setConfirm(null);
        }}
      />
    </TableRow>
  );
}

function Dialog({
  state,
  player,
  onClose,
  onConfirm,
}: {
  state: 'ban' | 'kick' | null;
  player: Player;
  onClose: () => void;
  onConfirm: (reason: string) => void;
}) {
  const [reason, setReason] = useState('');

  return (
    <AlertDialog open={state !== null} onOpenChange={(open) => (open ? undefined : onClose())}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {state === 'ban' ? `Ban ${player.name}?` : `Kick ${player.name}?`}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {state === 'ban'
              ? 'Adds the player to banned-players.json. They cannot rejoin until you pardon them.'
              : 'Removes the player from the server. They can rejoin immediately.'}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className='space-y-2'>
          <label htmlFor='reason' className='text-sm font-medium'>
            Reason
          </label>
          <Input
            id='reason'
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            placeholder={state === 'ban' ? 'Griefing spawn' : 'AFK farming'}
          />
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel render={<Button variant='outline' />}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            render={
              <BevelButton
                tone={state === 'ban' ? 'danger' : 'gray'}
                className='px-4 py-4 text-sm'
                onClick={() => {
                  onConfirm(reason);
                  setReason('');
                }}
              />
            }
          >
            {state === 'ban' ? 'Ban player' : 'Kick player'}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function PlayersPage({ instanceId }: { instanceId: string }) {
  const query = usePlayers(instanceId);
  const action = usePlayerAction(instanceId);
  const [filter, setFilter] = useState('');
  const [tab, setTab] = useState('all');

  const players = query.data?.players ?? [];
  const bans = query.data?.bans ?? [];
  const online = players.filter((player) => player.online);
  const ops = players.filter((player) => player.op);
  const whitelisted = players.filter((player) => player.whitelisted);

  const visible = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return players
      .filter((player) => {
        if (tab === 'online') return player.online;
        if (tab === 'ops') return player.op;
        if (tab === 'whitelist') return player.whitelisted;
        return true;
      })
      .filter((player) => (needle ? player.name.toLowerCase().includes(needle) : true))
      .sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
  }, [players, tab, filter]);

  const run = (name: string, playerAction: string, reason?: string) =>
    action.mutate({ action: playerAction, name, reason });

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <StatStrip
        items={[
          { label: 'Online now', value: `${online.length}`, hint: `of ${query.data?.players.length ?? 0} known players` },
          { label: 'Operators', value: `${ops.length}`, hint: ops.map((player) => player.name).join(', ') || 'none' },
          {
            label: 'Whitelist',
            value: query.data?.whitelistEnabled ? 'On' : 'Off',
            hint: query.data?.whitelistEnforced ? 'enforced on join' : 'not enforced',
          },
          { label: 'Bans', value: `${bans.length}`, hint: bans[0]?.name ?? 'nobody banned' },
        ]}
      />

      <SectionHeading
        title='Players'
        icon={UserGroup}
        description='Whitelist, operators and bans. Actions are sent over RCON so changes show up in the console immediately.'
        action={
          <Button
            variant='outline'
            size='sm'
            disabled={action.isPending}
            onClick={() => run('', query.data?.whitelistEnabled ? 'whitelist-remove' : 'whitelist-add')}
            className=''
          >
            Toggle whitelist in console
          </Button>
        }
      />

      <Tabs value={tab} onValueChange={setTab}>
        <div className='flex flex-wrap items-center gap-3'>
          <TabsList>
            <TabsTrigger value='all'>All ({players.length})</TabsTrigger>
            <TabsTrigger value='online'>Online ({online.length})</TabsTrigger>
            <TabsTrigger value='whitelist'>Whitelist ({whitelisted.length})</TabsTrigger>
            <TabsTrigger value='ops'>Ops ({ops.length})</TabsTrigger>
          </TabsList>
          <div className='relative min-w-48 flex-1'>
            <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
            <Input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder='Find a player…'
              className='pl-8'
              aria-label='Filter players'
            />
          </div>
        </div>

        <TabsContent value={tab} className='mt-3'>
          <Card className='gap-0 overflow-hidden py-0'>
            {visible.length === 0 ? (
              <CardContent className='p-6'>
                <EmptyState
                  icon={UserGroup}
                  title={tab === 'online' ? 'Nobody is online' : 'No players here yet'}
                  description={
                    tab === 'whitelist'
                      ? 'Add names to the whitelist to control who may join.'
                      : 'Player records appear after their first join.'
                  }
                />
              </CardContent>
            ) : (
              <ScrollArea className='h-[32rem] [&>[data-slot=scroll-area-viewport]>div]:block!'>
                <Table>
                  <TableHeader className='sticky top-0 z-10 bg-card'>
                    <TableRow>
                      <TableHead>Player</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead className='hidden md:table-cell'>Dimension</TableHead>
                      <TableHead className='hidden text-right sm:table-cell'>Ping</TableHead>
                      <TableHead className='text-right'>Session</TableHead>
                      <TableHead className='hidden text-right lg:table-cell'>Last seen</TableHead>
                      <TableHead className='text-right'>Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {visible.map((player) => (
                      <PlayerRow
                        key={player.uuid}
                        player={player}
                        pending={action.isPending}
                        onAction={(playerAction, reason) => run(player.name, playerAction, reason)}
                      />
                    ))}
                  </TableBody>
                </Table>
              </ScrollArea>
            )}
          </Card>
        </TabsContent>
      </Tabs>

      <Card>
        <CardHeader>
          <CardTitle className='flex items-center gap-2'>
            <Ban className='size-4' /> Bans
          </CardTitle>
          <CardDescription>
            Mirrors banned-players.json and banned-ips.json for the world this server has mounted.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {bans.length === 0 ? (
            <EmptyState icon={Ban} title='No bans' description='Nobody has been banned from this server.' />
          ) : (
            <ul className='divide-y'>
              {bans.map((ban) => (
                <li key={`${ban.name}-${ban.createdAt}`} className='flex flex-wrap items-center gap-3 py-3'>
                  <div className='min-w-0 flex-1'>
                    <p className='font-medium'>{ban.name}</p>
                    <p className='text-xs text-muted-foreground'>
                      {ban.reason} · by {ban.bannedBy} · {formatRelative(ban.createdAt)}
                    </p>
                  </div>
                  <span className='text-xs text-muted-foreground'>
                    {ban.expiresAt ? `expires ${formatDuration((Date.parse(ban.expiresAt) - Date.now()) / 1000)}` : 'permanent'}
                  </span>
                  <Button
                    variant='outline'
                    size='sm'
                    disabled={action.isPending}
                    onClick={() => run(ban.name, 'pardon')}
                  >
                    Pardon
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
