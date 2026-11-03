import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from '#/api/query-client';

export function ApiProvider({ children }: { children: React.ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}
