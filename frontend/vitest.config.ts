import { defineConfig } from 'vitest/config';

// Kept apart from vite.config.ts so tests do not load the router/devtools plugins.
export default defineConfig({
  resolve: { tsconfigPaths: true },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    // The contract suite waits on (simulated) server start-ups.
    testTimeout: 30_000,
  },
});
