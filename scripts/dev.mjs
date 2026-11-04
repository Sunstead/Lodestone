#!/usr/bin/env node
/**
 * Runs the backend and the Vite dev server together, with prefixed output.
 *
 *   node scripts/dev.mjs fake   # simulated servers: no Docker, no network
 *   node scripts/dev.mjs real   # Docker + real downloads (needs Docker running)
 *
 * Data lives in ./.lodestone-dev (fake) or ./.lodestone-dev-real (real) so
 * the two never mix. Open http://localhost:3000.
 */

import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const kind = process.argv[2] ?? 'fake';
if (!['fake', 'real'].includes(kind)) {
  console.error('usage: dev.mjs fake|real');
  process.exit(2);
}

const backendArgs =
  kind === 'fake'
    ? ['--runtime', 'fake', '--providers', 'fake', '--accept-eula', '--data-dir', join(root, '.lodestone-dev')]
    : ['--runtime', 'docker', '--providers', 'real', '--data-dir', join(root, '.lodestone-dev-real')];
// Sign in through Authentik when LODESTONE_OIDC_ISSUER is set (with
// LODESTONE_PUBLIC_URL=http://localhost:3000); otherwise everyone is `dev`.
if (!process.env.LODESTONE_OIDC_ISSUER) backendArgs.push('--dev-user', process.env.LODESTONE_DEV_USER ?? 'dev');

const colors = { api: '\x1b[35m', web: '\x1b[36m' };
const children = [];

function run(name, command, args, cwd, env = {}) {
  const child = spawn(command, args, { cwd, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  const prefix = `${colors[name]}${name}\x1b[0m │ `;
  for (const stream of [child.stdout, child.stderr]) {
    let buffer = '';
    stream.on('data', (chunk) => {
      buffer += chunk;
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) process.stdout.write(prefix + line + '\n');
    });
  }
  child.on('exit', (code) => {
    process.stdout.write(`${prefix}exited (${code})\n`);
    shutdown(code ?? 0);
  });
  children.push(child);
}

let stopping = false;
function shutdown(code) {
  if (stopping) return;
  stopping = true;
  for (const child of children) child.kill('SIGINT');
  setTimeout(() => process.exit(code), 500);
}
process.on('SIGINT', () => shutdown(0));
process.on('SIGTERM', () => shutdown(0));

run('api', 'cargo', ['run', '--bin', 'lodestone', '--', ...backendArgs], join(root, 'backend'), {
  RUST_LOG: process.env.RUST_LOG ?? 'lodestone=info,tower_http=warn',
});
// `live`: with a backend running, never fall back to demo data silently.
run('web', 'npm', ['run', 'dev'], join(root, 'frontend'), { VITE_LODESTONE_MODE: 'live' });
