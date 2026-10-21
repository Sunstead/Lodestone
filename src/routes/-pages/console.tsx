import { Button } from '@sunstead/ui/components/button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@sunstead/ui/components/card';
import { Checkbox } from '@sunstead/ui/components/checkbox';
import { Input } from '@sunstead/ui/components/input';
import { Separator } from '@sunstead/ui/components/separator';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@sunstead/ui/components/tooltip';
import { cn } from '@sunstead/ui/utils';
import { useConsoleTail, useInstanceStatus, useSendCommand } from '#/api/queries';
import { subscribeConsole } from '#/api/client';
import type { ConsoleLine, LogLevel } from '#/api/types';
import { KeyValue, SectionHeading, StatStrip } from '#/components/page-parts';
import { ServerStateDot, serverStateLabel } from '#/components/server-state';
import { formatBytes, formatClock, formatDuration, formatNumber } from '#/lib/format';
import {
  ArrowDownToLine,
  Ban,
  CircleCheck,
  CornerDownLeft,
  Eraser,
  Pause,
  Play,
  Search,
  Terminal,
  Trash2,
  TriangleAlert,
} from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

const MAX_LINES = 1500;

const LEVEL_META: Record<LogLevel, { label: string; className: string; tone: string }> = {
  DEBUG: { label: 'DEBUG', className: 'text-sky-400', tone: 'debug' },
  INFO: { label: 'INFO ', className: 'text-muted-foreground', tone: 'info' },
  WARN: { label: 'WARN ', className: 'text-yellow-500', tone: 'warn' },
  ERROR: { label: 'ERROR', className: 'text-red-500', tone: 'error' },
};

/** Split a log line so timestamps and levels line up in columns. */
function parseLine(message: string): { text: string; level: LogLevel | null } {
  const match = /^\[[^\]]*?\/(DEBUG|INFO|WARN|ERROR)\]:\s?(.*)$/.exec(message);
  if (match) return { text: match[2], level: match[1] as LogLevel };
  const bare = /^\[(DEBUG|INFO|WARN|ERROR)\]\s?(.*)$/.exec(message);
  if (bare) return { text: bare[2], level: bare[1] as LogLevel };
  return { text: message, level: null };
}

const SUGGESTIONS = [
  { command: 'list', hint: 'Show online players' },
  { command: 'save-all', hint: 'Flush chunks and players to disk' },
  { command: 'time set day', hint: 'Jump to daytime' },
  { command: 'weather clear', hint: 'Clear the sky' },
  { command: 'whitelist add <player>', hint: 'Add someone to the whitelist' },
  { command: 'tps', hint: 'Paper: tick rate over 1m/5m/15m' },
];

export function ConsolePage({ instanceId }: { instanceId: string }) {
  const tail = useConsoleTail(instanceId);
  const status = useInstanceStatus(instanceId);
  const send = useSendCommand(instanceId);

  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [connected, setConnected] = useState(false);
  const [paused, setPaused] = useState(false);
  const [command, setCommand] = useState('');
  const [query, setQuery] = useState('');
  const [levels, setLevels] = useState<Record<LogLevel, boolean>>({
    DEBUG: false,
    INFO: true,
    WARN: true,
    ERROR: true,
  });
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const historyRef = useRef<string[]>([]);
  const historyIndex = useRef(-1);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const bufferRef = useRef<ConsoleLine[]>([]);

  // Seed from the tail endpoint, then keep appending as the seed changes.
  useEffect(() => {
    if (tail.data) setLines(tail.data);
  }, [tail.data]);

  useEffect(() => {
    bufferRef.current = [];
    setLines([]);
    const unsubscribe = subscribeConsole(instanceId, {
      onLine: (line) => {
        if (paused) {
          bufferRef.current.push(line);
          return;
        }
        setLines((previous) => (previous.at(-1)?.id === line.id ? previous : [...previous, line].slice(-MAX_LINES)));
      },
      onStatus: () => undefined,
      onError: () => setConnected(false),
    });
    setConnected(true);
    return () => {
      unsubscribe();
      setConnected(false);
    };
  }, [instanceId, paused]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return lines.filter((line) => {
      const parsed = parseLine(line.message);
      const level = parsed.level ?? line.level;
      if (!levels[level]) return false;
      if (!needle) return true;
      return line.message.toLowerCase().includes(needle);
    });
  }, [lines, levels, query]);

  const counts = useMemo(() => {
    const total = { DEBUG: 0, INFO: 0, WARN: 0, ERROR: 0 } as Record<LogLevel, number>;
    for (const line of lines) {
      const parsed = parseLine(line.message);
      total[parsed.level ?? line.level] += 1;
    }
    return total;
  }, [lines]);

  const scrollToBottom = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    viewport.scrollTop = viewport.scrollHeight;
  }, []);

  useEffect(() => {
    if (atBottom && !paused) scrollToBottom();
  }, [filtered.length, atBottom, paused, scrollToBottom]);

  const handleScroll = (event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    setAtBottom(el.scrollHeight - el.scrollTop - el.clientHeight < 24);
  };

  const submit = (raw: string) => {
    const value = raw.trim().replace(/^\//, '');
    if (!value) return;
    historyRef.current = [value, ...historyRef.current.filter((entry) => entry !== value)].slice(0, 50);
    historyIndex.current = -1;
    send.mutate(value);
    setCommand('');
    setShowSuggestions(false);
    setAtBottom(true);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      submit(command);
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      const next = Math.min(historyRef.current.length - 1, historyIndex.current + 1);
      if (next >= 0) {
        historyIndex.current = next;
        setCommand(historyRef.current[next]);
      }
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      const next = historyIndex.current - 1;
      historyIndex.current = next;
      setCommand(next >= 0 ? historyRef.current[next] : '');
      return;
    }
    if (event.key === 'Tab') {
      event.preventDefault();
      setShowSuggestions((value) => !value);
    }
  };

  const suggestions = useMemo(() => {
    const needle = command.trim().replace(/^\//, '').toLowerCase();
    if (!needle) return SUGGESTIONS;
    return SUGGESTIONS.filter((entry) => entry.command.startsWith(needle.split(' ')[0]));
  }, [command]);

  const applySuggestions = (entry: (typeof SUGGESTIONS)[number]) => {
    if (entry.command.includes('<')) {
      setCommand(`${entry.command.split('<')[0]}`);
      setShowSuggestions(false);
      return;
    }
    submit(entry.command);
  };

  const download = () => {
    const blob = new Blob([lines.map((line) => `${formatClock(line.ts)} ${line.message}`).join('\n')], {
      type: 'text/plain',
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${instanceId}-latest.log`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  const run = status.data?.state === 'running';

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <StatStrip
        items={[
          {
            label: 'Container',
            value: <span className='flex items-center gap-2 text-base'><ServerStateDot state={status.data?.state ?? 'stopped'} />{serverStateLabel(status.data?.state ?? 'stopped')}</span>,
            hint: status.data?.container.name,
          },
          { label: 'Uptime', value: status.data?.uptimeSeconds != null ? formatDuration(status.data.uptimeSeconds) : '—' },
          { label: 'JVM heap', value: status.data ? formatBytes(status.data.memory.usedBytes, 1) : '—', hint: status.data ? `${formatBytes(status.data.memory.limitBytes, 0)} limit` : undefined },
          { label: 'Lines buffered', value: formatNumber(lines.length), hint: `${formatNumber(counts.WARN)} warn · ${counts.ERROR} error` },
        ]}
      />

      <SectionHeading
        title='Live console'
        icon={Terminal}
        description='Streamed over SSE from the container log plus RCON responses. Commands are sent through RCON.'
        action={
          <>
            <span className={cn('flex items-center gap-1.5 text-xs font-medium', connected ? 'text-green-500' : 'text-muted-foreground')}>
              <span className={cn('size-2 rounded-full', connected ? 'animate-pulse bg-green-500' : 'bg-muted-foreground')} />
              {connected ? 'Stream connected' : 'Stream idle'}
            </span>
            <Button variant='outline' size='sm' onClick={() => setPaused((value) => !value)}>
              {paused ? <Play /> : <Pause />}
              {paused ? `Resume${bufferRef.current.length ? ` (${bufferRef.current.length})` : ''}` : 'Pause'}
            </Button>
            <Button
              variant='outline'
              size='sm'
              disabled={paused || !bufferRef.current.length}
              onClick={() => {
                setLines((previous) => [...previous, ...bufferRef.current].slice(-MAX_LINES));
                bufferRef.current = [];
                setAtBottom(true);
              }}
            >
              <ArrowDownToLine />
              Flush
            </Button>
            <Button variant='outline' size='sm' onClick={download}>
              <Trash2 className='rotate-180' />
              Download log
            </Button>
          </>
        }
      />

      <Card className='gap-0 overflow-hidden py-0'>
        <div className='flex flex-wrap items-center gap-3 border-b px-4 py-3'>
          <div className='relative min-w-56 flex-1'>
            <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder='Filter lines…'
              className='pl-8'
              aria-label='Filter console output'
            />
          </div>
          <div className='flex flex-wrap items-center gap-4'>
            {(Object.keys(LEVEL_META) as LogLevel[]).map((level) => (
              <label key={level} className='flex items-center gap-2 text-xs font-medium'>
                <Checkbox
                  checked={levels[level]}
                  onCheckedChange={(checked) => setLevels((previous) => ({ ...previous, [level]: checked === true }))}
                />
                <span className={LEVEL_META[level].className}>{level}</span>
                <span className='text-muted-foreground tabular-nums'>{counts[level]}</span>
              </label>
            ))}
          </div>
          <Button variant='ghost' size='sm' onClick={() => setLines([])}>
            <Eraser />
            Clear view
          </Button>
        </div>

        <div
          ref={viewportRef}
          data-slot='scroll-area-viewport'
          onScroll={handleScroll}
          className='relative h-[26rem] overflow-y-auto bg-background/85'
          role='log'
          aria-live='polite'
          aria-label='Server console output'
        >
          {filtered.length === 0 ? (
            <p className='p-6 font-mono text-xs text-muted-foreground'>
              {lines.length === 0 ? 'Waiting for output…' : 'No lines match the current filters.'}
            </p>
          ) : (
            <ul className='p-3 font-mono text-xs leading-relaxed text-foreground/90'>
              {filtered.map((line) => {
                const parsed = parseLine(line.message);
                const level = parsed.level ?? line.level;
                return (
                  <li key={line.id} className='flex gap-3 hover:bg-foreground/5'>
                    <span className='shrink-0 text-muted-foreground/70 tabular-nums'>{formatClock(line.ts)}</span>
                    <span className={cn('w-11 shrink-0 font-semibold', LEVEL_META[level].className)}>{level}</span>
                    {line.source === 'rcon' ? (
                      <span aria-label='Sent over RCON' className='w-3 shrink-0 text-main'>
                        ›
                      </span>
                    ) : (
                      <span className='w-3 shrink-0' />
                    )}
                    <span className='min-w-0 wrap-break-word whitespace-pre-wrap'>{parsed.text}</span>
                  </li>
                );
              })}
            </ul>
          )}
          {!atBottom ? (
            <Button
              size='sm'
              variant='secondary'
              onClick={scrollToBottom}
              className='sticky bottom-3 left-1/2 -translate-x-1/2 shadow-lg'
            >
              <ArrowDownToLine />
              Jump to latest
            </Button>
          ) : null}
        </div>

        <div className='border-t'>
          {showSuggestions && suggestions.length > 0 ? (
            <ul className='grid gap-1 border-b p-2 sm:grid-cols-2 lg:grid-cols-3'>
              {suggestions.map((entry) => (
                <li key={entry.command}>
                  <button
                    type='button'
                    onClick={() => applySuggestions(entry)}
                    className='flex w-full items-baseline justify-between gap-2 rounded-sm px-2 py-1.5 text-left font-mono text-xs hover:bg-muted'
                  >
                    <span className='font-semibold'>/{entry.command}</span>
                    <span className='text-muted-foreground'>{entry.hint}</span>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          <form
            className='flex items-center gap-2 p-3'
            onSubmit={(event) => {
              event.preventDefault();
              submit(command);
            }}
          >
            <span aria-hidden='true' className='font-mono text-lg text-main'>
              /
            </span>
            <Input
              value={command}
              onChange={(event) => setCommand(event.target.value)}
              onKeyDown={onKeyDown}
              onFocus={() => setShowSuggestions(true)}
              onBlur={() => setTimeout(() => setShowSuggestions(false), 120)}
              placeholder={run ? 'Type a command, ↑ for history, Tab to browse' : 'Server is not running'}
              disabled={!run}
              className='flex-1 font-mono'
              aria-label='Console command'
            />
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger
                  render={
                    <Button type='submit' disabled={!run || !command.trim()} className='bevel bevel-main bg-(--bevel-base) text-accent-foreground hover:bg-(--bevel-base)'>
                      <CornerDownLeft />
                      Send
                    </Button>
                  }
                />
                <TooltipContent>Runs through RCON and prints the response here</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </form>
        </div>
      </Card>

      <div className='grid grid-cols-1 gap-4 lg:grid-cols-3'>
        <Card className='lg:col-span-2'>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <CircleCheck className='size-4' /> Stream health
            </CardTitle>
            <CardDescription>What the SSE connection is carrying right now</CardDescription>
          </CardHeader>
          <CardContent className='divide-y'>
            <KeyValue label='Connection' value={connected ? 'SSE open' : 'Idle'} />
            <KeyValue label='Endpoint' value={<code className='text-xs'>/api/v1/instances/{instanceId}/console/stream</code>} />
            <KeyValue label='Latest sequence' value={lines.at(-1)?.id ?? '—'} />
            <KeyValue label='Log file' value='/server/logs/latest.log' />
            <KeyValue label='Buffered lines' value={`${formatNumber(lines.length)} / ${formatNumber(MAX_LINES)}`} />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className='flex items-center gap-2'>
              <TriangleAlert className='size-4' /> Quick actions
            </CardTitle>
            <CardDescription>Common commands, sent over RCON</CardDescription>
          </CardHeader>
          <CardContent className='space-y-2'>
            <Button variant='outline' className='w-full justify-start font-mono text-xs' disabled={!run} onClick={() => submit('save-all')}>
              /save-all
            </Button>
            <Button variant='outline' className='w-full justify-start font-mono text-xs' disabled={!run} onClick={() => submit('list')}>
              /list
            </Button>
            <Button variant='outline' className='w-full justify-start font-mono text-xs' disabled={!run} onClick={() => submit('whitelist on')}>
              /whitelist on
            </Button>
            <Button variant='outline' className='w-full justify-start font-mono text-xs' disabled={!run} onClick={() => submit('weather clear')}>
              /weather clear
            </Button>
            <Separator />
            <div className='flex items-center justify-between gap-2 pt-2 text-sm'>
              <span className='flex items-center gap-2'>
                <Ban className='size-4 text-destructive' />
                Ban a player
              </span>
              <Button variant='ghost' size='sm' disabled={!run} onClick={() => submit('ban ')}>
                Use /ban
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
