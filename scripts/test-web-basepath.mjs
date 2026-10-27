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

  // The unprefixed alias must answer with the *probe*, not with the SPA
  // shell. `auth::is_public_path` has listed `/healthz` since the beginning,
  // but no route was ever registered for it, so it fell through to the SPA
  // fallback and answered `200 text/html`. Any probe that only checks for a
  // 200 calls that healthy — a probe that cannot fail is worse than none.
  const legacyHealth = await fetch(`${BASE}${PREFIX}/healthz`);
  assert(legacyHealth.status === 200, `${PREFIX}/healthz returns 200 (got ${legacyHealth.status})`);
  const legacyBody = await legacyHealth.text();
  assert(
    legacyBody.includes('"status":"ok"'),
    `${PREFIX}/healthz returns the health JSON, not the SPA shell (got ${JSON.stringify(
      legacyBody.slice(0, 60),
    )})`,
  );

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
  const shell = await spa.text();

  const unknown = await fetch(`${BASE}${PREFIX}/some/spa/route`);
  assert(unknown.status === 200, `${PREFIX}/some/spa/route returns 200 (got ${unknown.status})`);
  const unknownType = unknown.headers.get('content-type') ?? '';
  assert(
    unknownType.toLowerCase().includes('text/html'),
    `${PREFIX}/some/spa/route returns text/html (got ${JSON.stringify(unknownType)})`,
  );
  const deepShell = await unknown.text();

  // ---------------------------------------------------------------------
  // 浏览器视角：读外壳里写出来的 URL，然后真的去请求它。
  //
  // 下面这组断言是这个文件存在的原因：服务端 nest 得对不对（`/sp/api/projects` → 200、
  // 裸路径 → 404）与**浏览器会请求什么**是两件事。`vite build` 默认发出的是
  // 绝对路径 `/assets/...`，前端的 `fetch` 也是 `/api/...`：两者都
  // 不带前缀，在嵌套路由下全部 404 —— 应用起得来但每个接口都死。
  //
  // 静态资源由外壳自带前缀发布，故 `<script src>` 与 `fetch` 两侧用的是同一个前缀。
  // ---------------------------------------------------------------------

  /** 取外壳里所有根绝对引用（`src="..."` / `href="..."`）。 */
  function shellReferences(html) {
    return [...html.matchAll(/(?:src|href)="([^"]*)"/g)].map((m) => m[1]);
  }

  const references = shellReferences(shell);
  const assetRefs = references.filter((u) => u.startsWith(`${PREFIX}/assets/`));
  assert(
    assetRefs.length > 0,
    `shell references at least one prefixed /assets entry (got ${JSON.stringify(references)})`,
  );
  assert(
    references.every((u) => !u.startsWith('/assets/')),
    `no reference may point at the bare /assets root (got ${JSON.stringify(references)})`,
  );

  // 资源必须真的能取到，而且内容类型必须是 JS/CSS —— 返回 200 +
  // text/html 是 ServeDir 回落到 index.html 的典型特征，状态码断言
  // 看不见，只有 content-type 能看见。
  for (const ref of new Set(assetRefs)) {
    const res = await fetch(`${BASE}${ref}`);
    const type = (res.headers.get('content-type') ?? '').toLowerCase();
    assert(res.status === 200, `${ref} returns 200 (got ${res.status})`);
    assert(
      !type.includes('text/html'),
      `${ref} is a real asset, not the SPA fallback (got ${JSON.stringify(type)})`,
    );
  }

  // 前端靠这个全局拼 API 前缀。取不到就等于所有接口仍然裸奔在根上。
  const injected = shell.match(/window\.__SIMPLE_POST_BASE__\s*=\s*"([^"]*)"/);
  assert(injected !== null, 'shell injects window.__SIMPLE_POST_BASE__ for the renderer');
  if (injected) {
    assert(
      injected[1] === PREFIX,
      `injected base is ${PREFIX} (got ${JSON.stringify(injected[1])})`,
    );
    const viaInjected = `${injected[1]}/api/projects`;
    const res = await fetch(`${BASE}${viaInjected}`, {
      headers: { Authorization: 'Bearer probe' },
    });
    assert(
      res.ok || res.status === 401,
      `API URL the renderer will build resolves (${viaInjected} → ${res.status})`,
    );
  }

  // 深链（刷新）拿到的外壳必须同样带前缀，否则刷新一次就白屏。
  const deepRefs = shellReferences(deepShell);
  assert(
    deepRefs.every((u) => !u.startsWith('/assets/')),
    `deep-link shell is rewritten too (got ${JSON.stringify(deepRefs)})`,
  );
  assert(
    /window\.__SIMPLE_POST_BASE__\s*=\s*"[^"]*"/.test(deepShell),
    'deep-link shell carries the base global',
  );

  // 反向的钉子：裸根不该再暴露 /assets。补丁移除后它在 /sp 下 404。
  const bareAsset = await fetch(`${BASE}/assets/does-not-matter.js`);
  assert(
    bareAsset.status === 404,
    `bare /assets is no longer republished (got ${bareAsset.status})`,
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
  // See `test-web-auth.mjs`: `process.exit()` after `fetch` calls aborts in
  // libuv on Windows and masks the failure output. Set the code and let the
  // process drain instead.
  process.exitCode = 1;
}
console.log('\n[test:web-basepath] all assertions passed');
