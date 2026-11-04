import { defineConfig, devices } from '@playwright/test';

/**
 * Projects:
 *   demo  Vite dev server in demo mode. No backend.
 *   fake  The real backend (simulated runtime, no Docker/network) serving
 *         the built frontend. Run `npm run build:live` in frontend/ first;
 *         `just e2e` does it for you.
 *   oidc  The same, signing in through a stand-in OIDC provider
 *         (mock-oidc.mjs) the way it would through Authentik.
 */
const FAKE_PORT = 7980;
const OIDC_APP_PORT = 7981;
const OIDC_PROVIDER_PORT = 7991;
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
    {
      name: 'oidc',
      testMatch: /oidc\.spec\.ts/,
      use: { baseURL: `http://127.0.0.1:${OIDC_APP_PORT}` },
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
        '--runtime fake --providers fake --accept-eula --dev-user dev --fake-speed 0.1',
        '--static-dir ../frontend/dist',
      ].join(' '),
      url: `http://127.0.0.1:${FAKE_PORT}/api/v1/health`,
      reuseExistingServer: false,
      timeout: 300_000,
    },
    {
      command: `node mock-oidc.mjs ${OIDC_PROVIDER_PORT}`,
      url: `http://127.0.0.1:${OIDC_PROVIDER_PORT}/health`,
      reuseExistingServer: false,
    },
    {
      command: [
        'rm -rf ../.lodestone-e2e/oidc &&',
        'cargo run --quiet --manifest-path ../backend/Cargo.toml --bin lodestone --',
        `--bind 127.0.0.1:${OIDC_APP_PORT} --data-dir ../.lodestone-e2e/oidc`,
        '--runtime fake --providers fake --fake-speed 0.1 --static-dir ../frontend/dist',
        `--oidc-issuer http://127.0.0.1:${OIDC_PROVIDER_PORT}/application/o/lodestone/`,
        `--public-url http://127.0.0.1:${OIDC_APP_PORT} --oidc-admin-groups homelab-admins`,
      ].join(' '),
      url: `http://127.0.0.1:${OIDC_APP_PORT}/api/v1/health`,
      reuseExistingServer: false,
      timeout: 300_000,
    },
  ],
});
