import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@sunstead/ui/components/select';
import { Separator } from '@sunstead/ui/components/separator';
import { Switch } from '@sunstead/ui/components/switch';
import { THEMES, themesOf } from '@sunstead/ui/themes';
import { useTheme } from '@sunstead/ui/use-theme';
import { Input } from '@sunstead/ui/components/input';
import { detectMode, getMode, isBackendReachable, onModeChange, signOut } from '#/api/client';
import { useMe, useSettings, useSystem, useUpdateMe, useUpdateSettings, useUsers } from '#/api/queries';
import type { ApiMode } from '#/api/types';
import { formatBytes } from '#/lib/format';
import { KeyValue, SectionHeading } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { Globe, LogOut, Palette, RefreshCw, Scale, Server, ShieldCheck, UserRound, Users } from 'lucide-react';
import { formatRelative } from '#/lib/format';
import { useEffect, useState } from 'react';

/**
 * App-wide preferences. World and configuration settings live on their own
 * assets — this page only holds things that apply to Lodestone itself.
 */
export function AppSettingsPage() {
  const { themeId, setThemeId, followSystem, setFollowSystem } = useTheme();
  const [mode, setMode] = useState<ApiMode>(getMode());
  const [reachable, setReachable] = useState(isBackendReachable());
  const settings = useSettings();
  const updateSettings = useUpdateSettings();
  const system = useSystem();
  const me = useMe();
  const updateMe = useUpdateMe();
  const admin = me.data?.user.admin ?? false;
  const users = useUsers(admin);
  const [address, setAddress] = useState('');

  useEffect(() => {
    const unsubscribe = onModeChange((next) => {
      setMode(next);
      setReachable(isBackendReachable());
    });
    void detectMode();
    return unsubscribe;
  }, []);

  useEffect(() => {
    if (settings.data) setAddress(settings.data.publicAddress);
  }, [settings.data]);

  const darkThemes = themesOf('dark');
  const lightThemes = themesOf('light');
  const active = THEMES.find((theme) => theme.id === themeId);

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <SectionHeading
        title='Settings'
        icon={Server}
        description='Preferences for Lodestone itself. Worlds and configurations keep their own settings.'
      />

      <Card>
        <CardHeader>
          <CardTitle className='flex items-center gap-2'>
            <UserRound className='size-4' /> Account
          </CardTitle>
          <CardDescription>
            {me.data?.auth.mode === 'dev'
              ? 'Development mode: there is no sign-in, and everyone is this admin.'
              : 'Signed in through your identity provider. Your worlds, configurations and servers are yours alone.'}
          </CardDescription>
        </CardHeader>
        <CardContent className='flex flex-wrap items-center justify-between gap-3'>
          <div className='min-w-0'>
            <p className='flex items-center gap-2 font-medium'>
              {me.data?.user.displayName ?? '—'}
              {admin ? <Badge variant='secondary'>Admin</Badge> : null}
            </p>
            <p className='text-sm text-muted-foreground'>@{me.data?.user.username ?? '—'}</p>
          </div>
          {me.data?.auth.mode === 'oidc' ? (
            <Button variant='outline' onClick={() => void signOut()}>
              <LogOut /> Sign out
            </Button>
          ) : null}
        </CardContent>
      </Card>

      <div className='grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_22rem]'>
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Palette className='size-4' /> Appearance
            </CardTitle>
            <CardDescription>Pick the theme Lodestone draws with.</CardDescription>
          </CardHeader>
          <CardContent className='space-y-5'>
            <div className='flex items-center justify-between gap-4'>
              <div className='min-w-0'>
                <p className='font-medium'>Follow system</p>
                <p className='text-xs text-muted-foreground'>
                  Match the operating system's light or dark preference.
                </p>
              </div>
              <Switch checked={followSystem} onCheckedChange={setFollowSystem} />
            </div>

            <Separator />

            <div className='space-y-1.5'>
              <label htmlFor='theme-select' className='text-sm font-medium'>
                Theme
              </label>
              <Select
                value={themeId}
                items={THEMES.map((theme) => ({
                  value: theme.id,
                  label: `${theme.name} · ${theme.scheme}`,
                }))}
                onValueChange={(value) => value !== null && setThemeId(value)}
              >
                <SelectTrigger id='theme-select' className='w-full sm:max-w-sm'>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value='__dark-header' disabled>
                    Dark
                  </SelectItem>
                  {darkThemes.map((theme) => (
                    <SelectItem key={theme.id} value={theme.id}>
                      {theme.name}
                    </SelectItem>
                  ))}
                  <SelectItem value='__light-header' disabled>
                    Light
                  </SelectItem>
                  {lightThemes.map((theme) => (
                    <SelectItem key={theme.id} value={theme.id}>
                      {theme.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {active ? (
                <p className='text-xs text-muted-foreground'>{active.description}</p>
              ) : null}
            </div>
          </CardContent>
        </Card>

        <div className='space-y-4'>
          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <ShieldCheck className='size-4' /> Backend
              </CardTitle>
              <CardDescription>Where Lodestone reads its data from.</CardDescription>
            </CardHeader>
            <CardContent className='space-y-3'>
              <div className='flex items-center justify-between gap-3'>
                <span className='text-sm text-muted-foreground'>Mode</span>
                <Badge variant={mode === 'live' && reachable !== false ? 'default' : 'secondary'}>
                  {mode === 'demo' ? 'Demo data' : reachable === false ? 'Offline' : 'Live API'}
                </Badge>
              </div>
              <p className='text-sm text-muted-foreground'>
                {mode === 'demo'
                  ? 'Showing in-memory demo data. Nothing you do here runs a real server.'
                  : reachable === false
                    ? 'The Lodestone backend is not answering. Start it, then recheck.'
                    : 'Connected to the Lodestone backend. Changes are persisted.'}
              </p>
              <BevelButton
                tone='gray'
                className='w-full px-4 py-4 text-sm'
                onClick={() => void detectMode(true)}
              >
                <RefreshCw />
                Recheck connection
              </BevelButton>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className='flex items-center gap-2'>
                <Server className='size-4' /> Runtime
              </CardTitle>
              <CardDescription>What Lodestone manages your servers with.</CardDescription>
            </CardHeader>
            <CardContent className='divide-y'>
              <KeyValue
                label='Container runtime'
                value={
                  system.data
                    ? `${system.data.runtime.kind}${system.data.runtime.version ? ` ${system.data.runtime.version}` : ''}${system.data.runtime.available ? '' : ' (unavailable)'}`
                    : '—'
                }
              />
              <KeyValue label='Downloads from' value={system.data?.providers === 'real' ? 'Official sources' : (system.data?.providers ?? '—')} />
              <KeyValue label='Data directory' value={<code className='text-xs break-all'>{system.data?.dataDir ?? '—'}</code>} />
              <KeyValue label='Data on disk' value={system.data ? formatBytes(system.data.dataDirBytes, 1) : '—'} />
              <KeyValue label='Backup format' value='tar + zstd' />
              {system.data?.runtime.message ? (
                <p className='pt-2 text-xs text-muted-foreground'>{system.data.runtime.message}</p>
              ) : null}
            </CardContent>
          </Card>
        </div>
      </div>

      <div className='grid grid-cols-1 gap-4 xl:grid-cols-2'>
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Scale className='size-4' /> Minecraft EULA
            </CardTitle>
            <CardDescription>
              Lodestone downloads server software from Mojang and others when a server first starts. Your servers
              only run after you accept the{' '}
              <a className='underline' href='https://aka.ms/MinecraftEULA' target='_blank' rel='noreferrer'>
                Minecraft End User License Agreement
              </a>
              . Everyone accepts it for their own servers.
            </CardDescription>
          </CardHeader>
          <CardContent className='flex items-center justify-between gap-3'>
            <label htmlFor='eula' className='text-sm'>
              I have read and accept the Minecraft EULA
            </label>
            <Switch
              id='eula'
              checked={me.data?.eulaAccepted ?? false}
              disabled={!me.data || updateMe.isPending}
              onCheckedChange={(checked: boolean) => updateMe.mutate({ eulaAccepted: checked })}
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Globe className='size-4' /> Public address
            </CardTitle>
            <CardDescription>
              The host name players type to connect. Shown on every server with its port. Leave blank for
              localhost.{admin ? '' : ' Only admins can change it.'}
            </CardDescription>
          </CardHeader>
          <CardContent className='flex gap-2'>
            <Input
              aria-label='Public address'
              placeholder='play.example.com'
              value={address}
              disabled={!admin}
              onChange={(event) => setAddress(event.target.value)}
            />
            <Button
              variant='outline'
              disabled={!admin || !settings.data || address === settings.data.publicAddress || updateSettings.isPending}
              onClick={() => updateSettings.mutate({ publicAddress: address })}
            >
              Save
            </Button>
          </CardContent>
          {updateSettings.error ? (
            <p role='alert' className='px-6 pb-4 text-sm text-destructive'>
              {updateSettings.error.message}
            </p>
          ) : null}
        </Card>
      </div>

      {admin ? (
        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <Users className='size-4' /> People
            </CardTitle>
            <CardDescription>
              Everyone who has signed in. Access and admin rights come from groups in your identity provider.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {users.error ? <p className='text-sm text-destructive'>{users.error.message}</p> : null}
            <ul className='divide-y'>
              {(users.data ?? []).map((row) => (
                <li key={row.user.id} className='flex flex-wrap items-center gap-x-4 gap-y-1 py-2.5 text-sm'>
                  <div className='min-w-40 flex-1'>
                    <p className='flex items-center gap-2 font-medium'>
                      {row.user.displayName}
                      {row.user.admin ? <Badge variant='secondary'>Admin</Badge> : null}
                    </p>
                    <p className='text-xs text-muted-foreground'>
                      @{row.user.username} · last seen {formatRelative(row.lastSeenAt)}
                    </p>
                  </div>
                  <span className='text-muted-foreground tabular-nums'>
                    {row.instances} servers · {row.worlds} worlds · {row.profiles} configurations
                  </span>
                  {!row.eulaAccepted ? <Badge variant='outline'>EULA not accepted</Badge> : null}
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>
      ) : null}

      <Card>
        <CardHeader>
          <CardTitle className='text-base'>Data safety</CardTitle>
          <CardDescription>How Lodestone keeps your worlds and configurations safe.</CardDescription>
        </CardHeader>
        <CardContent className='space-y-3 text-sm text-muted-foreground'>
          <p>
            Nothing Minecraft-related ships with Lodestone. Server jars and Java come from their publishers
            the first time a server needs them, are checked against the published hashes, and are cached.
          </p>
          <p>
            Worlds, configurations and servers are separate assets. Deleting a server never deletes the
            world it ran or the configuration it used.
          </p>
          <p>
            A world is mounted by at most one server at a time. Moving it to another server stops the
            container, remounts the data and starts again — the world itself is untouched.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
