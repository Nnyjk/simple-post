#!/usr/bin/env node
// scripts/test-web-basepath.mjs — verify commit 5's reverse-proxy base path.
//
//   node scripts/test-web-basepath.mjs
//
// Asserts:
//   - with --public-base-path /sp:
//       GET /sp/api/projects        → 200 (route reachable under prefix)
//       GET /api/projects            → 404 (route NOT mounted at bare path)
//       GET /sp/api/healthz         → 200 (probe under prefix works)
//       GET /sp/healthz             → 200 (legacy probe under prefix works)
//       GET /sp/mcp/healthz         → 200 (mcp probe under prefix works)
//       GET /sp/                    → 200 (SPA fallback under prefix works,
//                                          returns text/html)
//       GET /sp/<unknown>           → 200 + text/html (SPA fallback catches
//                                          client-side routes; same shape as
//                                          the bare /<unknown> case)
//
// Mirrors test-web-auth.mjs (same spawn / wait / kill pattern, commit 4).
// The script uses --public-base-path (CLI flag) instead of the
// SIMPLE_POST_PUBLIC_BASE_PATH env var to exercise both code paths.

import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import process from 'node:process';

const REPO = join(import.meta.dirname, '..');
const EXE_DEBUG = join(
  REPO,
  'src-tauri',
  'target',
  'debug',
  process.platform === 'win32' ? 'simple-post.exe' : 'simple-post',
);
const EXE_RELEASE = join(
  REPO,
  'src-tauri',
  'target',
  'release',
  process.platform === 'win32' ? 'simple-post.exe' : 'simple-post',
);

let exe;
for (const candidate of [EXE_DEBUG, EXE_RELEASE]) {
  if (existsSync(candidate)) {
    exe = candidate;
    break;
  }
}
if (!exe) {
  console.error(`[test:web-basepath] simple-post binary not found.`);
  console.error(`[test:web-basepath] run \`cargo build\` first.`);
  process.exit(2);
}

const PREFIX = '/sp';
const BASE = 'http://127.0.0.1:4173';

let failures = 0;
function assert(cond, msg) {
  if (!cond) {
    console.error(`✗ ${msg}`);
    failures++;
  } else {
    console.log(`✓ ${msg}`);
  }
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function waitForHttp(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    // Probe the prefix; if the binary isn't up yet this 404s. Once it
    // responds 200/404 we know the listener is bound.
    try {
      await fetch(`${BASE}${PREFIX}/api/healthz`);
      return;
    } catch {
      /* keep polling */
    }
    await sleep(100);
  }
  throw new Error(`simple-post serve-web did not come up at ${BASE}${PREFIX}`);
}

async function killGracefully(proc, label) {
  if (!proc || proc.exitCode !== null || proc.signalCode !== null) return;
  proc.kill('SIGTERM');
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null || proc.signalCode !== null) {
      console.log(
        `[test:web-basepath] ${label} exited code=${proc.exitCode} sig=${proc.signalCode}`,
      );
      return;
    }
    await sleep(50);
  }
  proc.kill('SIGKILL');
}

const scratch = mkdtempSync(join(tmpdir(), 'web-basepath-'));
console.log(`[test:web-basepath] scratch = ${scratch}`);
const proc = spawn(
  exe,
  ['serve-web', '--bind=127.0.0.1', '--port=4173', '--public-base-path', PREFIX],
  {
    env: { ...process.env, SIMPLE_POST_DATA_DIR: scratch, RUST_LOG: 'warn' },
    stdio: ['ignore', 'pipe', 'pipe'],
  },
);
// Drain child stderr so a chatty server can't fill the pipe buffer
// and block; server logs are not asserted by this test.
proc.stderr.resume();

try {
  await waitForHttp();
  console.log(`[test:web-basepath] server up with base path ${PREFIX}`);

  // 1 — /sp/api/projects returns 200 (auth open in this run, no password)
  const okProjects = await fetch(`${BASE}${PREFIX}/api/projects`);
  assert(
    okProjects.status === 200,
    `${PREFIX}/api/projects returns 200 (got ${okProjects.status})`,
  );

  // 2 — /api/projects (bare) returns 404 — the router is NOT mounted at root
  const bareProjects = await fetch(`${BASE}/api/projects`);
  assert(
    bareProjects.status === 404,
    `bare /api/projects returns 404 under prefix mode (got ${bareProjects.status})`,
  );

  // 3 — healthz works under prefix
  const okH = await fetch(`${BASE}${PREFIX}/api/healthz`);
  assert(okH.status === 200, `${PREFIX}/api/healthz returns 200 (got ${okH.status})`);

  // 4 — bare /healthz returns 404 under prefix mode
  const bareH = await fetch(`${BASE}/healthz`);
  assert(bareH.status === 404, `bare /healthz returns 404 under prefix mode (got ${bareH.status})`);

  // 5 — /sp/mcp/healthz returns 200
  const okMcpH = await fetch(`${BASE}${PREFIX}/mcp/healthz`);
  assert(okMcpH.status === 200, `${PREFIX}/mcp/healthz returns 200 (got ${okMcpH.status})`);

  // 6 — SPA fallback under prefix: /sp/ returns text/html
  const okSpa = await fetch(`${BASE}${PREFIX}/`);
  assert(okSpa.status === 200, `${PREFIX}/ returns 200 (got ${okSpa.status})`);
  const ctype = okSpa.headers.get('content-type') ?? '';
  assert(
    ctype.toLowerCase().includes('text/html'),
    `${PREFIX}/ returns text/html (got ${JSON.stringify(ctype)})`,
  );

  // 7 — SPA fallback under prefix: /sp/<unknown-route> returns text/html
  //     so client-side routes reload cleanly behind nginx/Caddy
  const okUnknown = await fetch(`${BASE}${PREFIX}/some/spa/route`);
  assert(
    okUnknown.status === 200,
    `${PREFIX}/some/spa/route (SPA fallback) returns 200 (got ${okUnknown.status})`,
  );
  const ctype2 = okUnknown.headers.get('content-type') ?? '';
  assert(
    ctype2.toLowerCase().includes('text/html'),
    `${PREFIX}/some/spa/route returns text/html (got ${JSON.stringify(ctype2)})`,
  );
} finally {
  await killGracefully(proc, 'basepath');
  rmSync(scratch, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`\n[test:web-basepath] FAILED with ${failures} assertion failure(s)`);
  process.exit(1);
}
console.log(`\n[test:web-basepath] all assertions passed`);
