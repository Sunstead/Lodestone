import { ScrollArea } from '@sunstead/ui/components/scroll-area';
import { cn } from '@sunstead/ui/utils';
import type { LucideIcon } from 'lucide-react';
import type { ReactNode } from 'react';

/**
 * Small heading used above dense control clusters inside a page.
 * Keeps every tab visually consistent without new card chrome.
 */
export function SectionHeading({
  title,
  description,
  icon: Icon,
  action,
  className,
}: {
  title: string;
  description?: ReactNode;
  icon?: LucideIcon;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('flex flex-wrap items-end justify-between gap-3', className)}>
      <div className='min-w-0'>
        <h2 className='flex items-center gap-2 text-sm font-semibold tracking-wide text-foreground uppercase'>
          {Icon ? <Icon aria-hidden='true' className='size-4 text-muted-foreground' /> : null}
          {title}
        </h2>
        {description ? <p className='mt-1 text-sm text-muted-foreground'>{description}</p> : null}
      </div>
      {action ? <div className='flex shrink-0 items-center gap-2'>{action}</div> : null}
    </div>
  );
}

/** Dense label/value rows used across dashboard, settings and console sidebars. */
export function KeyValue({
  label,
  value,
  className,
}: {
  label: ReactNode;
  value: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('flex items-center justify-between gap-4 py-2 text-sm', className)}>
      <span className='text-muted-foreground'>{label}</span>
      <span className='text-right font-medium tabular-nums'>{value}</span>
    </div>
  );
}

/** Reusable empty state so every tab says the same thing in the same way. */
export function EmptyState({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon?: LucideIcon;
  title: string;
  description?: string;
  children?: ReactNode;
}) {
  return (
    <div className='flex flex-col items-center justify-center gap-2 rounded-md border border-dashed px-6 py-12 text-center'>
      {Icon ? <Icon aria-hidden='true' className='size-8 text-muted-foreground' /> : null}
      <p className='font-medium'>{title}</p>
      {description ? <p className='max-w-md text-sm text-muted-foreground'>{description}</p> : null}
      {children ? <div className='mt-2 flex flex-wrap items-center justify-center gap-2'>{children}</div> : null}
    </div>
  );
}

/**
 * Wraps a table in a height-capped scroll area so long lists scroll instead
 * of pushing the page down. The table header stays put while the body moves.
 *
 * The height is definite rather than a max: a Base UI scroll area only clips
 * its viewport when the root has a resolved height.
 */
export function ScrollTable({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <ScrollArea className={cn('h-[calc(100dvh-16rem)] [&>[data-slot=scroll-area-viewport]>div]:block!', className)}>
      <div className='[&_[data-slot=table-header]]:sticky [&_[data-slot=table-header]]:top-0 [&_[data-slot=table-header]]:z-10 [&_[data-slot=table-header]]:bg-card'>
        {children}
      </div>
    </ScrollArea>
  );
}

/** Inline stats strip used by console, backups and mods headers. */
export function StatStrip({
  items,
  className,
}: {
  items: { label: string; value: ReactNode; hint?: string }[];
  className?: string;
}) {
  return (
    <dl className={cn('grid grid-cols-2 gap-px overflow-hidden rounded-md border bg-border sm:grid-cols-4', className)}>
      {items.map((item) => (
        <div key={item.label} className='bg-card px-4 py-3'>
          <dt className='text-xs font-medium tracking-wide text-muted-foreground uppercase'>{item.label}</dt>
          <dd className='mt-1 text-lg font-semibold tabular-nums'>{item.value}</dd>
          {item.hint ? <p className='text-xs text-muted-foreground'>{item.hint}</p> : null}
        </div>
      ))}
    </dl>
  );
}
