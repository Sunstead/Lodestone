import { SidebarTrigger } from '@sunstead/ui/components/resizable-sidebar';
import { ChevronRight } from 'lucide-react';
import type { ReactNode } from 'react';
import { PageRibbon } from './server-navigation';
import type { Page, PageMeta } from './server-navigation';

/**
 * One breadcrumb step. When `onClick` is present the step is a link — that
 * is how you get from an asset back to its directory.
 */
export type Crumb = {
  label: string;
  onClick?: () => void;
};

/**
 * The header is the middle layer between the sidebar and the content:
 *
 *   sidebar (which section)  ->  header (which asset + which page)  ->  content
 *
 * `crumbs` is the section-and-asset breadcrumb. `tabs-only` drops that row
 * for instances, where the hero already names the asset and only the page
 * ribbon is needed. `title-only` drops the ribbon instead, for directory
 * pages that are reached only through the sidebar and have no siblings to
 * tab between.
 *
 * The ribbon always shows exactly one section's pages, so worlds never sit
 * beside instance tabs.
 */
export function LocationHeader(
  props: {
    pages?: readonly PageMeta[];
    activePage?: Page;
    onPageSelect?: (page: Page) => void;
    /** Names the asset these pages belong to, for screen readers. */
    ribbonLabel?: string;
    /** Section-level controls shown at the right of the header. */
    actions?: ReactNode;
  } & (
    | { variant?: 'crumbs'; crumbs: Crumb[] }
    | { variant: 'tabs-only'; crumbs?: never }
    | { variant: 'title-only'; crumbs?: never }
  ),
) {
  const { actions } = props;

  if (props.variant === 'title-only') {
    return (
      <div className='sticky top-0 z-30 flex h-12 min-w-0 items-center gap-2 border-y bg-sidebar/85 px-2 backdrop-blur-md'>
        <SidebarTrigger aria-label='Toggle sidebar' className='shrink-0' />
        <h1 className='min-w-0 truncate text-base font-semibold'>{props.ribbonLabel}</h1>
        {actions ? <div className='ml-auto flex shrink-0 items-center gap-2'>{actions}</div> : null}
      </div>
    );
  }

  const { pages, activePage, onPageSelect, ribbonLabel } = props as {
    pages: readonly PageMeta[];
    activePage: Page;
    onPageSelect: (page: Page) => void;
    ribbonLabel: string;
  };
  const crumbs = props.variant === 'tabs-only' ? null : props.crumbs;

  return (
    <div className='sticky top-0 z-30 border-y bg-sidebar/85 backdrop-blur-md'>
      {crumbs ? (
        <div className='flex h-11 min-w-0 items-center gap-2 px-2'>
          <SidebarTrigger aria-label='Toggle sidebar' className='shrink-0' />
          <nav aria-label='Breadcrumb' className='flex min-w-0 items-center gap-1 text-sm'>
            {crumbs.map((crumb, index) => {
              const last = index === crumbs.length - 1;
              return (
                <span key={`${crumb.label}-${index}`} className='flex min-w-0 items-center gap-1'>
                  {index > 0 ? (
                    <ChevronRight aria-hidden='true' className='size-3.5 shrink-0 text-muted-foreground/60' />
                  ) : null}
                  {crumb.onClick && !last ? (
                    <button
                      type='button'
                      onClick={crumb.onClick}
                      className='truncate text-muted-foreground underline-offset-4 hover:text-foreground hover:underline'
                    >
                      {crumb.label}
                    </button>
                  ) : (
                    <span
                      aria-current={last ? 'page' : undefined}
                      className={last ? 'truncate font-medium' : 'truncate text-muted-foreground'}
                    >
                      {crumb.label}
                    </span>
                  )}
                </span>
              );
            })}
          </nav>
          {actions ? <div className='ml-auto flex shrink-0 items-center gap-2'>{actions}</div> : null}
        </div>
      ) : null}

      <div className={`flex h-12 min-w-0 items-stretch ${crumbs ? 'border-t' : ''}`}>
        {!crumbs ? (
          <SidebarTrigger aria-label='Toggle sidebar' className='mx-2 shrink-0 self-center' />
        ) : null}
        <PageRibbon
          pages={pages}
          activePage={activePage}
          onPageSelect={onPageSelect}
          label={ribbonLabel}
        />
        {!crumbs && actions ? (
          <div className='my-auto mr-2 flex shrink-0 items-center gap-2'>{actions}</div>
        ) : null}
      </div>
    </div>
  );
}
