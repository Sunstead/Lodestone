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
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@sunstead/ui/components/tooltip';
import { cn } from '@sunstead/ui/utils';
import { useProfileProperties, useSaveProfileProperties } from '#/api/queries';
import type { PropertyField } from '#/api/types';
import { BevelButton } from '#/components/server-hero';
import { SectionHeading } from '#/components/page-parts';
import { formatRelative } from '#/lib/format';
import {
  Check,
  Download,
  Eye,
  EyeOff,
  FileText,
  Info,
  Lock,
  RotateCcw,
  Save,
  Search,
  Undo2,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';

/** Fieldset labels, keyed by the `group` a profile-scoped key declares. */
const GROUPS = [
  { id: 'network', label: 'Network', hint: 'Ports and motd' },
  { id: 'access', label: 'Access', hint: 'Whitelist and idling' },
  { id: 'performance', label: 'Performance', hint: 'Distance and tick load' },
  { id: 'rcon', label: 'RCON', hint: 'Remote console' },
  { id: 'advanced', label: 'Advanced', hint: 'Everything else' },
] as const;

type GroupId = (typeof GROUPS)[number]['id'];

function FieldRow({
  field,
  value,
  dirty,
  onChange,
}: {
  field: PropertyField;
  value: string;
  dirty: boolean;
  onChange: (value: string) => void;
}) {
  const [revealed, setRevealed] = useState(false);

  return (
    <div
      className={cn(
        'flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center',
        dirty && 'bg-main/5',
      )}
    >
      <div className='min-w-0 sm:w-72 sm:shrink-0'>
        <div className='flex flex-wrap items-center gap-2'>
          <label htmlFor={`prop-${field.key}`} className='font-medium'>
            {field.label}
          </label>
          {dirty ? <Badge variant='secondary'>edited</Badge> : null}
          {field.appliesLive ? (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger
                  render={
                    <span className='flex items-center gap-1 text-xs text-muted-foreground'>
                      <Info aria-hidden='true' className='size-3.5' />
                      applies live
                    </span>
                  }
                />
                <TooltipContent>No restart needed — Minecraft re-reads this key at runtime.</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          ) : null}
        </div>
        <p className='mt-1 text-xs leading-relaxed text-muted-foreground'>{field.description}</p>
        <code className='mt-1 inline-block text-[11px] text-muted-foreground/70'>{field.key}</code>
      </div>

      <div className='min-w-0 flex-1 sm:max-w-md'>
        {field.control === 'switch' ? (
          <div className='flex items-center gap-3'>
            <Switch
              id={`prop-${field.key}`}
              checked={value === 'true'}
              onCheckedChange={(checked) => onChange(String(checked))}
            />
            <span className='text-sm font-medium tabular-nums'>{value === 'true' ? 'true' : 'false'}</span>
          </div>
        ) : field.control === 'select' ? (
          <Select
            value={value}
            items={(field.options ?? []).map((option) => ({ value: option.value, label: option.label }))}
            onValueChange={(next) => onChange(String(next))}
          >
            <SelectTrigger id={`prop-${field.key}`} className='w-full'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(field.options ?? []).map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : field.control === 'textarea' ? (
          <Textarea
            id={`prop-${field.key}`}
            value={value}
            onChange={(event) => onChange(event.target.value)}
            placeholder={field.placeholder}
            rows={3}
          />
        ) : field.control === 'password' ? (
          <div className='flex items-center gap-2'>
            <Input
              id={`prop-${field.key}`}
              type={revealed ? 'text' : 'password'}
              value={value}
              className='font-mono'
              onChange={(event) => onChange(event.target.value)}
            />
            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              aria-label={revealed ? 'Hide value' : 'Show value'}
              onClick={() => setRevealed((previous) => !previous)}
              className='shrink-0'
            >
              {revealed ? <EyeOff /> : <Eye />}
            </Button>
          </div>
        ) : field.control === 'slider' && field.min !== undefined && field.max !== undefined ? (
          <div className='space-y-1.5'>
            <div className='flex items-center gap-3'>
              <input
                id={`prop-${field.key}`}
                type='range'
                min={field.min}
                max={field.max}
                step={field.step ?? 1}
                value={Number(value) || field.min}
                onChange={(event) => onChange(event.target.value)}
                className='h-1.5 w-full cursor-pointer appearance-none rounded-full bg-muted accent-[var(--color-main)]'
              />
              <span className='w-16 shrink-0 text-right text-sm font-semibold tabular-nums'>
                {value}
                {field.unit ? <span className='ml-1 text-xs font-normal text-muted-foreground'>{field.unit}</span> : null}
              </span>
            </div>
          </div>
        ) : (
          <div className='flex items-center gap-2'>
            <Input
              id={`prop-${field.key}`}
              type={field.control === 'number' ? 'number' : 'text'}
              value={value}
              min={field.min}
              max={field.max}
              placeholder={field.placeholder}
              onChange={(event) => onChange(event.target.value)}
              className='tabular-nums'
            />
            {field.unit ? <span className='shrink-0 text-xs text-muted-foreground'>{field.unit}</span> : null}
          </div>
        )}
      </div>

      <div className='flex items-center gap-1 sm:w-24 sm:shrink-0 sm:justify-end'>
        {value !== field.defaultValue ? (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    type='button'
                    variant='ghost'
                    size='icon-sm'
                    aria-label={`Reset ${field.label} to default`}
                    onClick={() => onChange(field.defaultValue)}
                  >
                    <RotateCcw />
                  </Button>
                }
              />
              <TooltipContent>Reset to default ({field.defaultValue || 'blank'})</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          <span className='text-xs text-muted-foreground/50'>default</span>
        )}
      </div>
    </div>
  );
}

/** A key that belongs to the world or the container: visible, never editable here. */
function DerivedRow({ field }: { field: PropertyField }) {
  return (
    <div className='flex flex-col gap-2 px-4 py-3.5 sm:flex-row sm:items-center'>
      <div className='min-w-0 sm:w-72 sm:shrink-0'>
        <div className='flex flex-wrap items-center gap-2'>
          <span className='font-medium'>{field.label}</span>
          <Badge variant='outline' className='gap-1'>
            <Lock className='size-3' />
            {field.scope}
          </Badge>
        </div>
        <p className='mt-1 text-xs leading-relaxed text-muted-foreground'>
          {field.derivedFrom ? `Mirrors this configuration's own key: ${field.derivedFrom}.` : field.description}
        </p>
        <code className='mt-1 inline-block text-[11px] text-muted-foreground/70'>{field.key}</code>
      </div>
      <div className='min-w-0 flex-1 sm:max-w-md'>
        <p className='truncate font-mono text-sm text-muted-foreground'>{field.value || '—'}</p>
      </div>
      <div className='sm:w-24 sm:shrink-0' />
    </div>
  );
}

export function PropertiesPage({ profileId }: { profileId: string }) {
  const query = useProfileProperties(profileId);
  const save = useSaveProfileProperties(profileId);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [group, setGroup] = useState<GroupId>('network');
  const [needle, setNeedle] = useState('');
  const [showOther, setShowOther] = useState(false);

  useEffect(() => {
    setDraft({});
    setNeedle('');
  }, [profileId]);

  const fields = query.data?.fields ?? [];
  const valueOf = (field: PropertyField) => draft[field.key] ?? field.value;
  const changes = useMemo(() => {
    const result: Record<string, string> = {};
    for (const field of fields) {
      const value = draft[field.key];
      if (value !== undefined && value !== field.value) result[field.key] = value;
    }
    return result;
  }, [draft, fields]);

  const dirtyCount = Object.keys(changes).length;

  const visible = useMemo(() => {
    const search = needle.trim().toLowerCase();
    return fields.filter((field) => {
      if (search) {
        return (
          field.key.toLowerCase().includes(search) ||
          field.label.toLowerCase().includes(search) ||
          field.description.toLowerCase().includes(search)
        );
      }
      return field.group === group;
    });
  }, [fields, group, needle]);

  const groupCounts = useMemo(() => {
    const counts = {} as Record<string, number>;
    for (const field of fields) counts[field.group] = (counts[field.group] ?? 0) + 1;
    return counts;
  }, [fields]);

  const dirtyGroups = useMemo(() => {
    const set = new Set<string>();
    for (const field of fields) {
      if (changes[field.key] !== undefined) set.add(field.group);
    }
    return set;
  }, [changes, fields]);

  const currentGroup = GROUPS.find((entry) => entry.id === group)!;

  const commit = () => {
    if (!dirtyCount) return;
    save.mutate(changes, { onSuccess: () => setDraft({}) });
  };

  const download = () => {
    const text = fields.map((field) => `${field.key}=${valueOf(field)}`).join('\n');
    const blob = new Blob([text], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'server.properties';
    anchor.click();
    URL.revokeObjectURL(url);
  };

  if (query.isLoading) {
    return (
      <div className='mx-auto w-full max-w-[1600px] space-y-4 p-4 md:p-6'>
        <Skeleton className='h-10 w-64' />
        <Skeleton className='h-[32rem] w-full' />
      </div>
    );
  }

  return (
    <div className='mx-auto flex w-full min-w-0 max-w-[1600px] flex-col gap-4 p-4 md:p-6'>
      <SectionHeading
        title='Configuration properties'
        icon={FileText}
        description='Only the keys a configuration owns are editable here. World rules live on the world asset, and published ports live on the server.'
        action={
          <>
            <span className='text-xs text-muted-foreground'>
              Saved {formatRelative(query.data?.updatedAt)}
            </span>
            <Button variant='outline' size='sm' onClick={download}>
              <Download />
              Export
            </Button>
            {dirtyCount > 0 ? (
              <Button variant='ghost' size='sm' onClick={() => setDraft({})}>
                <Undo2 />
                Discard {dirtyCount}
              </Button>
            ) : null}
            <BevelButton
              size='sm'
              className='px-4 py-4 text-sm'
              disabled={!dirtyCount || save.isPending}
              onClick={commit}
            >
              <Save />
              {save.isPending ? 'Writing…' : dirtyCount ? `Save ${dirtyCount} change${dirtyCount > 1 ? 's' : ''}` : 'Saved'}
            </BevelButton>
          </>
        }
      />

      {save.isSuccess && !dirtyCount ? (
        <p className='flex items-center gap-2 rounded-md border border-main/40 bg-main/10 px-3 py-2 text-sm'>
          <Check className='size-4 text-main' />
          server.properties written to the configuration. Keys marked “applies live” took effect immediately; the rest apply on the next restart.
        </p>
      ) : null}

      <div className='grid grid-cols-1 gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]'>
        <aside className='space-y-4'>
          <nav aria-label='Property groups' className='rounded-md border p-1'>
            {GROUPS.map((entry) => {
              const active = !needle && entry.id === group;
              return (
                <button
                  key={entry.id}
                  type='button'
                  aria-current={active ? 'true' : undefined}
                  onClick={() => {
                    setGroup(entry.id);
                    setNeedle('');
                  }}
                  className={cn(
                    'flex w-full items-center justify-between gap-2 rounded-sm px-3 py-2 text-left text-sm transition-colors hover:bg-muted',
                    active && 'bg-sidebar-accent font-medium',
                  )}
                >
                  <span className='min-w-0'>
                    <span className='block truncate'>{entry.label}</span>
                    <span className='block truncate text-xs text-muted-foreground'>{entry.hint}</span>
                  </span>
                  <span className='flex shrink-0 items-center gap-1.5'>
                    {dirtyGroups.has(entry.id) ? <span className='size-1.5 rounded-full bg-main' /> : null}
                    <span className='text-xs text-muted-foreground tabular-nums'>{groupCounts[entry.id] ?? 0}</span>
                  </span>
                </button>
              );
            })}
          </nav>

          <Card className='gap-2 py-4'>
            <CardHeader className='px-4'>
              <CardTitle className='text-sm'>File</CardTitle>
            </CardHeader>
            <CardContent className='space-y-2 px-4 text-xs text-muted-foreground'>
              <p className='break-all font-mono'>{query.data?.path}</p>
              <Separator />
              <p>{fields.length} configuration keys tracked by Lodestone.</p>
              <p>
                Keys owned by a world or a server are listed but locked — edit them on the asset that owns them.
              </p>
            </CardContent>
          </Card>
        </aside>

        <div className='min-w-0 space-y-3'>
          <div className='relative'>
            <Search aria-hidden='true' className='pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground' />
            <Input
              value={needle}
              onChange={(event) => setNeedle(event.target.value)}
              placeholder='Search all properties by name or key…'
              className='pl-8'
              aria-label='Search properties'
            />
          </div>

          <Card className='gap-0 overflow-hidden py-0'>
            <CardHeader className='border-b bg-muted/30 px-4 py-3'>
              <CardTitle className='text-base'>{needle ? `Search results (${visible.length})` : currentGroup.label}</CardTitle>
              <CardDescription>{needle ? 'Matching every group' : currentGroup.hint}</CardDescription>
            </CardHeader>
            <CardContent className='divide-y p-0'>
              {visible.length === 0 ? (
                <p className='p-6 text-sm text-muted-foreground'>No properties match that search.</p>
              ) : (
                visible.map((field) => (
                  <FieldRow
                    key={field.key}
                    field={field}
                    value={valueOf(field)}
                    dirty={changes[field.key] !== undefined}
                    onChange={(value) => setDraft((previous) => ({ ...previous, [field.key]: value }))}
                  />
                ))
              )}
            </CardContent>
          </Card>

          <div className='space-y-2' data-testid='profile-properties-derived'>
            <button
              type='button'
              aria-expanded={showOther}
              onClick={() => setShowOther((value) => !value)}
              className='text-xs font-medium text-muted-foreground underline-offset-4 hover:underline'
            >
              {showOther ? 'Hide' : 'Show'} what this configuration does not own
            </button>
            {showOther ? (
              <Card className='gap-0 overflow-hidden py-0'>
                <CardHeader className='border-b bg-muted/30 px-4 py-3'>
                  <CardTitle className='text-base'>Owned elsewhere</CardTitle>
                  <CardDescription>
                    These keys stay in server.properties, but Lodestone writes them from the world or the server.
                  </CardDescription>
                </CardHeader>
                <CardContent className='divide-y p-0'>
                  <DerivedRow
                    field={{
                      key: 'level-name',
                      label: 'World folder',
                      description: '',
                      scope: 'world',
                      group: '',
                      control: 'text',
                      value: '',
                      defaultValue: '',
                      readOnly: true,
                      derivedFrom: 'world.folder',
                    }}
                  />
                  <DerivedRow
                    field={{
                      key: 'difficulty / gamemode / pvp',
                      label: 'World gameplay rules',
                      description: 'Set per world so two servers can share a configuration but play differently.',
                      scope: 'world',
                      group: '',
                      control: 'text',
                      value: 'edit on the world page',
                      defaultValue: '',
                      readOnly: true,
                    }}
                  />
                  <DerivedRow
                    field={{
                      key: 'server-port / server-ip',
                      label: 'Published ports',
                      description: 'Chosen per server so several containers can share one configuration.',
                      scope: 'instance',
                      group: '',
                      control: 'text',
                      value: 'see the server dashboard',
                      defaultValue: '',
                      readOnly: true,
                    }}
                  />
                </CardContent>
              </Card>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
