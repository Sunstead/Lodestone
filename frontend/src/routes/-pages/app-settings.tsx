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
import { detectMode, getMode, onModeChange } from '#/api/client';
import type { ApiMode } from '#/api/types';
import { KeyValue, SectionHeading } from '#/components/page-parts';
import { BevelButton } from '#/components/server-hero';
import { Palette, RefreshCw, Server, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';

/**
 * App-wide preferences. World and configuration settings live on their own
 * assets — this page only holds things that apply to Lodestone itself.
 */
export function AppSettingsPage() {
  const { themeId, setThemeId, followSystem, setFollowSystem } = useTheme();
  const [mode, setMode] = useState<ApiMode>(getMode());

  useEffect(() => {
    const unsubscribe = onModeChange(setMode);
    void detectMode();
    return unsubscribe;
  }, []);

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
                <Badge variant={mode === 'live' ? 'default' : 'secondary'}>
                  {mode === 'live' ? 'Live API' : 'Demo data'}
                </Badge>
              </div>
              <p className='text-sm text-muted-foreground'>
                {mode === 'live'
                  ? 'Connected to the Rust backend. Changes are persisted.'
                  : 'The Rust backend is unreachable, so everything is served by the in-memory demo store.'}
              </p>
              <BevelButton
                tone='gray'
                className='w-full px-4 py-4 text-sm'
                onClick={() => void detectMode()}
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
              <KeyValue label='Container runtime' value='Docker Engine' />
              <KeyValue label='Metadata store' value='SQLite' />
              <KeyValue label='World isolation' value='One world, one server' />
              <KeyValue label='Backup format' value='gzipped tar' />
            </CardContent>
          </Card>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className='text-base'>Data safety</CardTitle>
          <CardDescription>How Lodestone keeps your worlds and configurations safe.</CardDescription>
        </CardHeader>
        <CardContent className='space-y-3 text-sm text-muted-foreground'>
          <p>
            Worlds, configurations and servers are separate assets. Deleting a server never deletes the
            world it ran or the configuration it used.
          </p>
          <p>
            A world is mounted by at most one server at a time. Moving it to another server stops the
            container, remounts the data and starts again — the world itself is untouched.
          </p>
          <div className='flex flex-wrap gap-2 pt-1'>
            <Button variant='ghost' size='sm' disabled>
              Export data
            </Button>
            <Button variant='ghost' size='sm' disabled>
              Import data
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
