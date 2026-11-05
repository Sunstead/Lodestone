import { Badge } from '@sunstead/ui/components/badge';
import { Button } from '@sunstead/ui/components/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@sunstead/ui/components/card';
import { Field, FieldDescription, FieldLabel } from '@sunstead/ui/components/field';
import { Input } from '@sunstead/ui/components/input';
import { useSettings, useUpdateInstance } from '#/api/queries';
import type { DnsStatus, Instance } from '#/api/types';
import { KeyValue } from '#/components/page-parts';
import { Check, Copy, Globe } from 'lucide-react';
import { useEffect, useState } from 'react';

const STATUS: Record<DnsStatus, { label: string; variant: 'default' | 'secondary' | 'outline' | 'destructive' }> = {
  synced: { label: 'live', variant: 'default' },
  pending: { label: 'updating DNS', variant: 'secondary' },
  manual: { label: 'add the record', variant: 'outline' },
  error: { label: 'DNS error', variant: 'destructive' },
  unavailable: { label: 'not available', variant: 'outline' },
};

function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      type='button'
      variant='ghost'
      size='icon-sm'
      aria-label={label}
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? <Check /> : <Copy />}
    </Button>
  );
}

/** What players type to join, and the server's address name (an SRV record). */
export function ServerAddressCard({ instance, publicAddress }: { instance: Instance; publicAddress: string }) {
  const update = useUpdateInstance(instance.id);
  const [draft, setDraft] = useState(instance.hostname ?? '');

  useEffect(() => setDraft(instance.hostname ?? ''), [instance.hostname]);

  const dirty = draft.trim().toLowerCase() !== (instance.hostname ?? '');
  const dns = instance.dns;
  const suffix = publicAddress ? `.${publicAddress}` : '.<public address>';

  return (
    <Card>
      <CardHeader className='pb-3'>
        <CardTitle className='flex items-center gap-2 text-base'>
          <Globe className='size-4' /> Address
        </CardTitle>
        <CardDescription>
          What players type to join. An address name lets them join without a port.
        </CardDescription>
      </CardHeader>
      <CardContent className='space-y-4'>
        <div className='divide-y'>
          <KeyValue
            label='Join at'
            value={
              <span className='flex items-center gap-1'>
                <code className='text-sm' data-testid='server-address'>
                  {instance.address}
                </code>
                <CopyButton text={instance.address} label='Copy the address' />
              </span>
            }
          />
          {instance.address !== instance.directAddress ? (
            <KeyValue
              label='Or, with the port'
              value={<code className='text-xs text-muted-foreground'>{instance.directAddress}</code>}
            />
          ) : null}
        </div>

        <form
          className='space-y-2'
          onSubmit={(event) => {
            event.preventDefault();
            update.mutate({ hostname: draft.trim() === '' ? null : draft.trim() });
          }}
        >
          <label htmlFor={`hostname-${instance.id}`} className='text-sm font-medium'>
            Address name
          </label>
          <div className='flex flex-wrap items-center gap-2'>
            <div className='flex min-w-0 flex-1 items-center gap-1'>
              <Input
                id={`hostname-${instance.id}`}
                value={draft}
                placeholder='e.g. survival'
                onChange={(event) => setDraft(event.target.value)}
                className='min-w-0 max-w-48 font-mono'
              />
              <span className='truncate font-mono text-sm text-muted-foreground'>{suffix}</span>
            </div>
            <Button type='submit' size='sm' disabled={!dirty || update.isPending}>
              {draft.trim() === '' && instance.hostname ? 'Remove' : 'Save'}
            </Button>
          </div>
          {update.error ? (
            <p role='alert' className='text-sm text-destructive'>
              {update.error.message}
            </p>
          ) : null}
        </form>

        {dns ? (
          <div className='space-y-2 rounded-md border p-3 text-sm'>
            <div className='flex flex-wrap items-center gap-2'>
              <span className='font-medium'>DNS</span>
              <Badge variant={STATUS[dns.status].variant} data-testid='dns-status'>
                {STATUS[dns.status].label}
              </Badge>
            </div>
            {dns.message ? <p className='text-muted-foreground'>{dns.message}</p> : null}
            {dns.record ? (
              <div className='grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-xs'>
                <span className='text-muted-foreground'>SRV</span>
                <code className='break-all'>{dns.record}</code>
                <span className='text-muted-foreground'>Value</span>
                <code className='break-all'>{dns.value}</code>
              </div>
            ) : null}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

/** The address name when creating a server: `survival` → `survival.play.example.com`. */
export function AddressNameField({
  id,
  value,
  onChange,
}: {
  id: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const settings = useSettings();
  const publicAddress = settings.data?.publicAddress ?? '';
  return (
    <Field>
      <FieldLabel htmlFor={id}>Address name</FieldLabel>
      <div className='flex min-w-0 items-center gap-1'>
        <Input
          id={id}
          value={value}
          placeholder='optional, e.g. survival'
          onChange={(event) => onChange(event.target.value)}
          className='min-w-0 max-w-56 font-mono'
        />
        <span className='truncate font-mono text-sm text-muted-foreground'>
          .{publicAddress || '<public address>'}
        </span>
      </div>
      <FieldDescription>
        Players join at this name, with no port. Leave it empty to use the address and port.
      </FieldDescription>
    </Field>
  );
}
