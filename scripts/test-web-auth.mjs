#!/usr/bin/env node
// Web 鉴权集成测试：验证开放模式、密码模式、健康检查和 MCP 探针。
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
  console.error(`[test:web-auth] E2E 产物预检失败：${error.message}`);
  process.exit(2);
}

const PASSWORD = 'shh-this-is-secret-9c8a';
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
      const response = await fetch(`${BASE}/api/healthz`);
      if (response.ok) return;
    } catch {
      // 服务尚未监听，继续轮询。
    }
    await sleep(100);
  }
  throw new Error(`simple-post serve-web did not come up at ${BASE}`);
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
      `[test:web-auth] ${label} exited code=${exit.exitResult.code} sig=${exit.exitResult.signal}`,
    );
  }
}

function cleanScratch(directory, label) {
  if (!cleanupScratch(directory)) {
    failures++;
    console.error(`✗ failed to clean ${label} temporary directory ${directory}`);
  }
}

async function runPhase(label, scratch, tracked, testBody) {
  try {
    await testBody();
  } catch (error) {
    failures++;
    console.error(`✗ ${label} failed: ${error.message}`);
  } finally {
    await stopTracked(tracked, label);
    cleanScratch(scratch, label);
  }
}

// ----- 阶段一：无密码 ---------------------------------------------------
const scratch1 = mkdtempSync(join(tmpdir(), 'web-auth-open-'));
console.log(`[test:web-auth] phase 1 scratch = ${scratch1}`);
const proc1 = spawnTracked(
  exe,
  ['serve-web', '--bind=127.0.0.1', '--port=4173'],
  {
    env: { ...process.env, SIMPLE_POST_DATA_DIR: scratch1, RUST_LOG: 'warn' },
    stdio: ['ignore', 'pipe', 'pipe'],
  },
  { label: 'phase 1' },
);
await runPhase('phase 1', scratch1, proc1, async () => {
  await waitForHttp(proc1);
  console.log('[test:web-auth] phase 1: server up (no password)');

  const projects = await fetch(`${BASE}/api/projects`);
  assert(
    projects.status === 200,
    `phase 1: GET /api/projects returns 200 (got ${projects.status})`,
  );

  const health = await fetch(`${BASE}/api/healthz`);
  assert(health.status === 200, `phase 1: /api/healthz is 200 (got ${health.status})`);
  const body = await health.json();
  assert(
    body?.status === 'ok',
    `phase 1: /api/healthz body.status === "ok" (got ${JSON.stringify(body)})`,
  );
});

// ----- 阶段二：设置密码 -------------------------------------------------
const scratch2 = mkdtempSync(join(tmpdir(), 'web-auth-locked-'));
console.log(`[test:web-auth] phase 2 scratch = ${scratch2}`);
const proc2 = spawnTracked(
  exe,
  ['serve-web', '--bind=127.0.0.1', '--port=4173'],
  {
    env: {
      ...process.env,
      SIMPLE_POST_DATA_DIR: scratch2,
      SIMPLE_POST_WEB_PASSWORD: PASSWORD,
      RUST_LOG: 'warn',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  },
  { label: 'phase 2' },
);
await runPhase('phase 2', scratch2, proc2, async () => {
  await waitForHttp(proc2);
  console.log('[test:web-auth] phase 2: server up (password set)');

  const noToken = await fetch(`${BASE}/api/projects`);
  assert(noToken.status === 401, `phase 2: no token returns 401 (got ${noToken.status})`);
  const authHeader = noToken.headers.get('www-authenticate') ?? '';
  assert(
    authHeader.toLowerCase().includes('bearer'),
    `phase 2: 401 carries Bearer challenge (got ${JSON.stringify(authHeader)})`,
  );

  const wrongToken = await fetch(`${BASE}/api/projects`, {
    headers: { authorization: 'Bearer wrong-password' },
  });
  assert(wrongToken.status === 401, `phase 2: wrong token returns 401 (got ${wrongToken.status})`);

  const correctToken = await fetch(`${BASE}/api/projects`, {
    headers: { authorization: `Bearer ${PASSWORD}` },
  });
  assert(
    correctToken.status === 200,
    `phase 2: correct token returns 200 (got ${correctToken.status})`,
  );

  const health = await fetch(`${BASE}/api/healthz`);
  assert(health.status === 200, `phase 2: /api/healthz stays 200 (got ${health.status})`);

  const spa = await fetch(`${BASE}/`);
  assert(spa.status === 200, `phase 2: GET / stays 200 (got ${spa.status})`);
  const contentType = spa.headers.get('content-type') ?? '';
  assert(
    contentType.toLowerCase().includes('text/html'),
    `phase 2: GET / returns text/html (got ${JSON.stringify(contentType)})`,
  );

  const mcpHealth = await fetch(`${BASE}/mcp/healthz`);
  assert(mcpHealth.status === 200, `phase 2: /mcp/healthz stays 200 (got ${mcpHealth.status})`);
});

if (failures > 0) {
  console.error(`\n[test:web-auth] FAILED with ${failures} assertion failure(s)`);
  process.exit(1);
}
console.log('\n[test:web-auth] all assertions passed');
