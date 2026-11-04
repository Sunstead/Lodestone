#!/usr/bin/env node
/**
 * Starts a throwaway Lodestone backend, waits for /health, runs a command
 * against it, then stops the backend and deletes its data.
 *
 *   node scripts/with-backend.mjs [--real] [--port 7979] [--static] -- <command...>
 *
 * Default: simulated runtime and providers (no Docker, no network), EULA
 * pre-accepted. `--real` uses Docker and real downloads, keeping the
 * download cache in .lodestone-e2e/ so reruns are fast.
 *
 * The command sees LODESTONE_API (e.g. http://127.0.0.1:7979/api/v1) and
 * LODESTONE_URL (the origin).
 */

import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const split = args.indexOf('--');
const flags = split === -1 ? args : args.slice(0, split);
const command = split === -1 ? [] : args.slice(split + 1);
const real = flags.includes('--real');
const serveStatic = flags.includes('--static');
const port = Number(flags[flags.indexOf('--port') + 1]) || 7979;

if (command.length === 0) {
  console.error('usage: with-backend.mjs [--real] [--port N] [--static] -- <command...>');
  process.exit(2);
}

const build = spawnSync('cargo', ['build', '--quiet', '--bin', 'lodestone'], { cwd: join(root, 'backend'), stdio: 'inherit' });
if (build.status !== 0) process.exit(build.status ?? 1);

let dataDir;
if (real) {
  dataDir = join(root, '.lodestone-e2e', `run-${Date.now()}`);
  mkdirSync(dataDir, { recursive: true });
} else {
  dataDir = mkdtempSync(join(tmpdir(), 'lodestone-test-'));
}

const backendArgs = [
  '--data-dir', dataDir,
  '--bind', `127.0.0.1:${port}`,
  '--runtime', real ? 'docker' : 'fake',
  '--providers', real ? 'real' : 'fake',
  '--accept-eula',
  '--dev-user',
  'dev',
  '--stop-timeout', '20',
  '--fake-speed', '0.05',
];
if (serveStatic) backendArgs.push('--static-dir', join(root, 'frontend', 'dist'));

const backend = spawn(join(root, 'backend', 'target', 'debug', 'lodestone'), backendArgs, {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'lodestone=warn' },
});

const origin = `http://127.0.0.1:${port}`;
const cleanup = () => {
  backend.kill('SIGINT');
  if (!real) rmSync(dataDir, { recursive: true, force: true });
};
process.on('SIGINT', () => {
  cleanup();
  process.exit(130);
});

const deadline = Date.now() + 30_000;
for (;;) {
  try {
    const response = await fetch(`${origin}/api/v1/health`);
    if (response.ok) break;
  } catch {
    /* not up yet */
  }
  if (backend.exitCode !== null || Date.now() > deadline) {
    console.error('backend did not come up');
    cleanup();
    process.exit(1);
  }
  await new Promise((r) => setTimeout(r, 200));
}

const child = spawn(command[0], command.slice(1), {
  stdio: 'inherit',
  env: { ...process.env, LODESTONE_API: `${origin}/api/v1`, LODESTONE_URL: origin, LODESTONE_CONTRACT_API: `${origin}/api/v1` },
});
child.on('exit', (code) => {
  cleanup();
  process.exit(code ?? 1);
});
