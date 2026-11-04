import { QueryClientProvider } from '@tanstack/react-query';
import { Outlet, createRootRoute } from '@tanstack/react-router'

import { TanStackRouterDevtoolsPanel } from '@tanstack/react-router-devtools'
import { TanStackDevtools } from '@tanstack/react-devtools'

import { queryClient } from '#/api/query-client'
import '@fontsource-variable/oxanium';
import '../styles.css'

export const Route = createRootRoute({
  component: RootComponent,
})

function RootComponent() {
  return (
    <QueryClientProvider client={queryClient}>
      <Outlet />
      <TanStackDevtools
        config={{
          position: 'bottom-right',
        }}
        plugins={[
          {
            name: 'TanStack Router',
            render: <TanStackRouterDevtoolsPanel />,
          },
        ]}
      />
    </QueryClientProvider>
  )
}
