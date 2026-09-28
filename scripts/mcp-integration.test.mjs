#!/usr/bin/env node
// MCP 集成测试：启动临时 daemon，验证 JSON-RPC、当前策略可见工具和只读调用边界。
// 所有进程输出都会被消费，退出和临时目录清理都有明确上限。

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
  console.error(`[test:mcp] E2E 产物预检失败：${error.message}`);
  process.exit(2);
}

const scratch = mkdtempSync(join(tmpdir(), 'mcp-it-'));
const env = {
  ...process.env,
  SIMPLE_POST_DATA_DIR: scratch,
  RUST_LOG: 'warn',
};
const port = 41_000 + Math.floor(Math.random() * 1000);
const daemon = spawnTracked(
  exe,
  ['mcp', '--transport=http', `--port=${port}`, '--bind=127.0.0.1'],
  { env, stdio: ['ignore', 'pipe', 'pipe'] },
  { label: 'mcp daemon' },
);
let stderr = '';
let stdout = '';
daemon.child.stdout.on('data', (chunk) => {
  stdout = `${stdout}${chunk.toString()}`.slice(-8_000);
});
daemon.child.stderr.on('data', (chunk) => {
  stderr = `${stderr}${chunk.toString()}`.slice(-8_000);
});

function assert(condition, message) {
  if (condition) {
    console.log(`✓ ${message}`);
    return;
  }
  console.error(`✗ ${message}`);
  throw new Error(message);
}

async function waitForHealthz(timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (daemon.spawnError) throw new Error(`daemon spawn failed: ${daemon.spawnError.message}`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/mcp/healthz`);
      if (response.ok) return response.json();
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  throw new Error(`daemon did not become healthy; stderr:\n${stderr}`);
}

async function rpc(id, method, params) {
  const response = await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
  });
  if (!response.ok) throw new Error(`${method} returned HTTP ${response.status}`);
  return response.json();
}

let failures = 0;
try {
  const health = await waitForHealthz();
  assert(health.status === 'ok', 'daemon healthz returns status=ok');

  const initialize = await rpc(1, 'initialize', {
    protocolVersion: '2025-03-26',
    capabilities: {},
    clientInfo: { name: 'test:mcp', version: '0.1.0' },
  });
  assert(
    initialize?.result?.serverInfo?.name === 'simple-post-mcp',
    `initialize reports simple-post-mcp (got ${initialize?.result?.serverInfo?.name})`,
  );

  // 默认策略是 read_only；只验证策略允许的核心只读集合，不泄漏写工具。
  const list = await rpc(2, 'tools/list', {});
  const names = new Set((list?.result?.tools ?? []).map((tool) => tool.name));
  const readonlyTools = [
    'env_list',
    'env_get_active',
    'project_list',
    'project_get',
    'module_list',
    'collection_list',
    'endpoint_list',
    'endpoint_get',
    'endpoint_search',
    'base_url_def_list',
    'api_resolve_template',
    'api_resolve_endpoint',
    'api_validate_endpoint',
    'api_parse_response',
    'api_diff_responses',
    'api_extract_jsonpath',
  ];
  for (const name of readonlyTools) {
    assert(names.has(name), `tools/list exposes read-only tool ${name}`);
  }
  for (const name of ['env_create', 'env_set_var', 'api_run_request', 'api_call_endpoint']) {
    assert(!names.has(name), `read_only policy hides ${name}`);
  }

  const envList = await rpc(3, 'tools/call', {
    name: 'env_list',
    arguments: { project_id: 'p_demo' },
  });
  const envText = envList?.result?.content?.[0]?.text ?? '';
  const envs = JSON.parse(envText);
  assert(
    Array.isArray(envs) && envs.length === 3,
    'env_list returns the three seeded environments',
  );
  assert(
    envs.some((env) => env.name === 'dev' && env.isActive === true),
    'env_list includes active dev',
  );

  const create = await rpc(4, 'tools/call', {
    name: 'env_create',
    arguments: { project_id: 'p_demo', name: 'should-fail' },
  });
  assert(create?.result?.is_error === true, 'read_only policy rejects env_create');
  assert(
    /permission denied/i.test(create?.result?.content?.[0]?.text ?? ''),
    'env_create rejection explains permission denial',
  );

  const after = await (await fetch(`http://127.0.0.1:${port}/mcp/healthz`)).json();
  assert(
    after.requests_handled >= 4,
    `healthz reports handled requests (got ${after.requests_handled})`,
  );
  assert(after.status === 'ok', 'healthz remains ok after calls');

  console.log('\n✓ all assertions passed');
} catch (error) {
  failures++;
  console.error(`✗ test failed: ${error.message}`);
  if (stderr) console.error(`daemon stderr tail:\n${stderr}`);
  if (stdout) console.error(`daemon stdout tail:\n${stdout}`);
} finally {
  const exit = await daemon.terminate({ gracefulMs: 2_000, forceMs: 2_000 });
  if (exit.timedOut) {
    failures++;
    console.error(`✗ ${daemon.label} did not exit within cleanup deadline`);
  }
  if (!cleanupScratch(scratch)) {
    failures++;
    console.error(`✗ failed to clean temporary directory ${scratch}`);
  }
}

// Set the code and let Node exit on its own — do NOT call
// `process.exit()` here.
//
// This test `fetch`es the daemon, so undici holds a keep-alive socket.
// `process.exit()` tears the loop down while that handle is still
// registered, and on Windows libuv then aborts with
//   `Assertion failed: !(handle->flags & UV_HANDLE_CLOSING)
//    (src\win\async.c:76)` — exit code 0xC0000409 — *after* every
// assertion already printed "all assertions passed". A green run then
// reports as a crash and CI cannot tell pass from fail. Confirmed by an
// isolated repro: identical script, only difference is `process.exit()`
// vs. falling off the end (exit 0 both times, the former aborts).
process.exitCode = failures === 0 ? 0 : 1;
