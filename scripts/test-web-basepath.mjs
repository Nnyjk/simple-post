#!/usr/bin/env node
// Web base path 集成测试：验证前缀路由、探针和 SPA fallback。
// 子进程输出由统一生命周期工具消费，退出与临时目录清理均有时间上限。

import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import process from 'node:process';
import { requireWorkspaceBinary } from './e2e/artifact-resolver.mjs';
import { cleanupScratch, spawnTracked } from './e2e/process-lifecycle.mjs';

let exe;
try {
  const artifact = await requireWorkspaceBinary('simple-post', {
    profiles: ['debug', 'release'],
  });
  exe = artifact.path;
} catch (error) {
  console.error(`[test:web-basepath] E2E 产物预检失败：${error.message}`);
  process.exit(2);
}

const PREFIX = '/sp';
const BASE = 'http://127.0.0.1:4173';
let failures = 0;

function assert(condition, message) {
  if (condition) {
    console.log(`✓ ${message}`);
  } else {
    console.error(`✗ ${message}`);
    failures++;
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForHttp(tracked, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (tracked.spawnError) {
      throw new Error(`server spawn failed: ${tracked.spawnError.message}`);
    }
    try {
      const response = await fetch(`${BASE}${PREFIX}/api/healthz`);
      if (response.ok) return;
    } catch {
      // 服务尚未监听，继续轮询。
    }
    await sleep(100);
  }
  throw new Error(`simple-post serve-web did not come up at ${BASE}${PREFIX}`);
}

async function stopTracked(tracked, label) {
  const exit = await tracked.terminate({ gracefulMs: 2_000, forceMs: 2_000 });
  if (exit.timedOut) {
    failures++;
    console.error(`✗ ${label} did not exit within cleanup deadline`);
  }
  if (exit.spawnError) {
    failures++;
    console.error(`✗ ${label} spawn error: ${exit.spawnError.message}`);
  }
  if (exit.exitResult) {
    console.log(
      `[test:web-basepath] ${label} exited code=${exit.exitResult.code} sig=${exit.exitResult.signal}`,
    );
  }
}

function cleanScratch(directory) {
  if (!cleanupScratch(directory)) {
    failures++;
    console.error(`✗ failed to clean basepath temporary directory ${directory}`);
  }
}

const scratch = mkdtempSync(join(tmpdir(), 'web-basepath-'));
console.log(`[test:web-basepath] scratch = ${scratch}`);
const proc = spawnTracked(
  exe,
  ['serve-web', '--bind=127.0.0.1', '--port=4173', '--public-base-path', PREFIX],
  {
    env: { ...process.env, SIMPLE_POST_DATA_DIR: scratch, RUST_LOG: 'warn' },
    stdio: ['ignore', 'pipe', 'pipe'],
  },
  { label: 'basepath' },
);

try {
  await waitForHttp(proc);
  console.log(`[test:web-basepath] server up with base path ${PREFIX}`);

  const projects = await fetch(`${BASE}${PREFIX}/api/projects`);
  assert(projects.status === 200, `${PREFIX}/api/projects returns 200 (got ${projects.status})`);

  const bareProjects = await fetch(`${BASE}/api/projects`);
  assert(
    bareProjects.status === 404,
    `bare /api/projects returns 404 (got ${bareProjects.status})`,
  );

  const health = await fetch(`${BASE}${PREFIX}/api/healthz`);
  assert(health.status === 200, `${PREFIX}/api/healthz returns 200 (got ${health.status})`);

  const bareHealth = await fetch(`${BASE}/healthz`);
  assert(bareHealth.status === 404, `bare /healthz returns 404 (got ${bareHealth.status})`);

  const mcpHealth = await fetch(`${BASE}${PREFIX}/mcp/healthz`);
  assert(mcpHealth.status === 200, `${PREFIX}/mcp/healthz returns 200 (got ${mcpHealth.status})`);

  const spa = await fetch(`${BASE}${PREFIX}/`);
  assert(spa.status === 200, `${PREFIX}/ returns 200 (got ${spa.status})`);
  const spaType = spa.headers.get('content-type') ?? '';
  assert(
    spaType.toLowerCase().includes('text/html'),
    `${PREFIX}/ returns text/html (got ${JSON.stringify(spaType)})`,
  );

  const unknown = await fetch(`${BASE}${PREFIX}/some/spa/route`);
  assert(unknown.status === 200, `${PREFIX}/some/spa/route returns 200 (got ${unknown.status})`);
  const unknownType = unknown.headers.get('content-type') ?? '';
  assert(
    unknownType.toLowerCase().includes('text/html'),
    `${PREFIX}/some/spa/route returns text/html (got ${JSON.stringify(unknownType)})`,
  );
} catch (error) {
  failures++;
  console.error(`✗ test failed: ${error.message}`);
} finally {
  await stopTracked(proc, 'basepath');
  cleanScratch(scratch);
}

if (failures > 0) {
  console.error(`\n[test:web-basepath] FAILED with ${failures} assertion failure(s)`);
  process.exit(1);
}
console.log('\n[test:web-basepath] all assertions passed');
