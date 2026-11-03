import { defineConfig, devices } from '@playwright/test';

/**
 * Projects:
 *   demo  Vite dev server in demo mode. No backend.
 *   fake  The real backend (simulated runtime, no Docker/network) serving
 *         the built frontend. Run `npm run build:live` in frontend/ first;
 *         `just e2e` does it for you.
 */
const FAKE_PORT = 7980;
const DEMO_PORT = 4173;

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: { trace: 'retain-on-failure', ...devices['Desktop Chrome'] },
  projects: [
    {
      name: 'demo',
      testMatch: /demo\.spec\.ts/,
      use: { baseURL: `http://localhost:${DEMO_PORT}` },
    },
    {
      name: 'fake',
      testMatch: /fake\.spec\.ts/,
      use: { baseURL: `http://127.0.0.1:${FAKE_PORT}` },
    },
  ],
  webServer: [
    {
      command: `npm --prefix ../frontend run dev -- --port ${DEMO_PORT} --strictPort`,
      url: `http://localhost:${DEMO_PORT}`,
      env: { VITE_LODESTONE_MODE: 'demo' },
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
    },
    {
      command: [
        'rm -rf ../.lodestone-e2e/fake &&',
        'cargo run --quiet --manifest-path ../backend/Cargo.toml --bin lodestone --',
        `--bind 127.0.0.1:${FAKE_PORT} --data-dir ../.lodestone-e2e/fake`,
        '--runtime fake --providers fake --accept-eula --fake-speed 0.1',
        '--static-dir ../frontend/dist',
      ].join(' '),
      url: `http://127.0.0.1:${FAKE_PORT}/api/v1/health`,
      reuseExistingServer: false,
      timeout: 300_000,
    },
  ],
});
