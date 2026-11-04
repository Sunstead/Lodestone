#!/usr/bin/env node
/**
 * Spins up a real Minecraft server through Lodestone, end to end:
 * backend → Docker → jar from the publisher → running server → RCON.
 *
 *   node scripts/smoke.mjs                       # newest Vanilla release, then stop
 *   node scripts/smoke.mjs --keep                # leave it running so you can join
 *   node scripts/smoke.mjs --software paper --version 1.21.4
 *   node scripts/smoke.mjs --accept-eula         # non-interactive (CI)
 *
 * Data lives in .lodestone-smoke/ (the download cache is kept between runs).
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { createInterface } from 'node:readline/promises';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const option = (name, fallback) => {
  const index = argv.indexOf(`--${name}`);
  return index === -1 ? fallback : argv[index + 1];
};

const software = option('software', 'vanilla');
const keep = flag('keep');
const port = Number(option('port', 7899));
const api = `http://127.0.0.1:${port}/api/v1`;
const step = (message) => console.log(`\x1b[36m▸\x1b[0m ${message}`);

if (!flag('accept-eula')) {
  console.log('Running a Minecraft server requires accepting the Minecraft EULA: https://aka.ms/MinecraftEULA');
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  const answer = await rl.question('Do you accept it? [y/N] ');
  rl.close();
  if (!/^y(es)?$/i.test(answer.trim())) {
    console.log('Not accepted; nothing was started.');
    process.exit(1);
  }
}

// Fresh database each run, but keep the download cache.
const dataDir = join(root, '.lodestone-smoke');
mkdirSync(dataDir, { recursive: true });
for (const entry of readdirSync(dataDir)) {
  if (entry !== 'cache') rmSync(join(dataDir, entry), { recursive: true, force: true });
}

step('Building the backend');
const build = spawnSync('cargo', ['build', '--quiet', '--bin', 'lodestone'], { cwd: join(root, 'backend'), stdio: 'inherit' });
if (build.status !== 0) process.exit(build.status ?? 1);

step('Starting Lodestone (Docker runtime, real downloads)');
const backend = spawn(
  join(root, 'backend', 'target', 'debug', 'lodestone'),
  ['--data-dir', dataDir, '--bind', `127.0.0.1:${port}`, '--runtime', 'docker', '--providers', 'real', '--accept-eula', '--dev-user', 'smoke'],
  { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'lodestone=warn' } },
);

let instanceId = null;
async function call(method, path, body) {
  const response = await fetch(`${api}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  const value = text ? JSON.parse(text) : null;
  if (!response.ok) throw new Error(value?.error?.message ?? `${response.status}`);
  return value;
}

async function finish(code) {
  if (instanceId) {
    step('Stopping the server');
    try {
      await call('POST', `/instances/${instanceId}/actions`, { action: 'stop' });
      for (let i = 0; i < 120; i++) {
        const instance = await call('GET', `/instances/${instanceId}`);
        if (instance.state === 'stopped' || instance.state === 'error') break;
        await new Promise((r) => setTimeout(r, 500));
      }
    } catch (error) {
      console.error(`  could not stop cleanly: ${error.message}`);
    }
  }
  backend.kill('SIGINT');
  process.exit(code);
}
process.on('SIGINT', () => void finish(0));

try {
  for (let i = 0; ; i++) {
    try {
      await call('GET', '/health');
      break;
    } catch {
      if (i > 100) throw new Error('backend did not start');
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  const system = await call('GET', '/system');
  if (!system.runtime.available) throw new Error(system.runtime.message ?? 'Docker is not available');
  step(`Docker ${system.runtime.version} is reachable`);

  const catalog = await call('GET', '/profiles/software');
  const entry = catalog.software.find((option) => option.id === software);
  if (!entry) throw new Error(`${software} is not available (publisher unreachable?)`);
  const version = option('version', entry.versions[0].version);
  step(`Creating a ${entry.label} ${version} server (Java ${entry.versions.find((v) => v.version === version)?.javaMajor ?? '?'})`);

  const instance = await call('POST', '/instances', {
    name: 'smoke',
    newWorld: {
      name: 'Smoke Test',
      levelType: 'minecraft:normal',
      seed: null,
      generateStructures: true,
      difficulty: 'normal',
      gamemode: 'survival',
      hardcore: false,
      maxWorldSize: 29999984,
      allowNether: true,
    },
    newProfile: {
      name: `${entry.label} ${version}`,
      description: 'Created by scripts/smoke.mjs',
      softwareId: software,
      version,
      memory: { minMb: 1024, maxMb: 2048, initialMb: 1024 },
      cpuCores: null,
      javaArgs: '-XX:+UseG1GC',
      autoStart: false,
      restartOnCrash: false,
      crashThreshold: 1,
    },
    start: true,
  });
  instanceId = instance.id;

  let lastMessage = '';
  const deadline = Date.now() + 10 * 60_000;
  for (;;) {
    const current = await call('GET', `/instances/${instanceId}`);
    if (current.state === 'running') break;
    if (current.state === 'error') throw new Error(current.lastError ?? 'server failed to start');
    const [job] = await call('GET', '/jobs');
    const message = current.state === 'preparing' && job ? `${job.message}${job.progress != null ? ` (${Math.round(job.progress * 100)}%)` : ''}` : current.state;
    if (message !== lastMessage) {
      step(message);
      lastMessage = message;
    }
    if (Date.now() > deadline) throw new Error('timed out waiting for the server');
    await new Promise((r) => setTimeout(r, 500));
  }

  const lines = await call('POST', `/instances/${instanceId}/console`, { command: 'list' });
  step(`RCON: ${lines.slice(1).map((line) => line.message).join(' ')}`);
  await new Promise((r) => setTimeout(r, 3000)); // first container stats sample
  const status = await call('GET', `/instances/${instanceId}/status`);
  step(`Running in ${status.container.image}, ${Math.round(status.memory.usedBytes / 1024 / 1024)} MB in use`);
  console.log(`\n\x1b[32m✔ ${entry.label} ${version} is running.\x1b[0m Join at \x1b[1m${instance.address}\x1b[0m\n`);

  if (keep) {
    console.log('Leaving it running. Press Ctrl-C to stop the server and Lodestone.');
    console.log(`(Want the UI too? In another terminal: cd frontend && LODESTONE_API=http://127.0.0.1:${port} npm run dev)`);
    await new Promise(() => {});
  }
  await finish(0);
} catch (error) {
  console.error(`\x1b[31m✘ ${error.message}\x1b[0m`);
  await finish(1);
}
