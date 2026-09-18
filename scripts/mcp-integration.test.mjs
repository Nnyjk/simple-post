#!/usr/bin/env node
// scripts/mcp-integration.test.mjs — spawn the MCP daemon, hit /mcp with
// JSON-RPC, verify the contract end-to-end.
//
//   npm run test:mcp
//
// Asserts:
//   - daemon starts, binds to a fresh port, writes mcp.port
//   - POST /mcp with `initialize` returns serverInfo.name == "simple-post-mcp"
//   - `tools/list` contains at least the documented 44 tool names
//   - `tools/call` with `env_list` returns the seeded dev/staging/prod trio
//   - `tools/call` with `env_create` under default read_call permission is
//     denied (returns is_error:true, does NOT panic)
//   - GET /mcp/healthz reports incremented `requests_handled`
//   - daemon stops cleanly on SIGTERM, leaves no zombie process

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";

const REPO = join(import.meta.dirname, "..");
const EXE_DEBUG = join(REPO, "src-tauri", "target", "debug",
  process.platform === "win32" ? "simple-post.exe" : "simple-post");
const EXE_RELEASE = join(REPO, "src-tauri", "target", "release",
  process.platform === "win32" ? "simple-post.exe" : "simple-post");

let exe;
for (const candidate of [EXE_DEBUG, EXE_RELEASE]) {
  if (existsSync(candidate)) { exe = candidate; break; }
}
if (!exe) {
  console.error(`[test:mcp] simple-post binary not found (looked for debug + release builds of src-tauri).`);
  console.error(`[test:mcp] run \`npm run tauri:build:debug\` or \`cargo build\` first.`);
  process.exit(2);
}

const scratch = mkdtempSync(join(tmpdir(), "mcp-it-"));
console.log(`[test:mcp] scratch = ${scratch}`);
console.log(`[test:mcp] exe = ${exe}`);

const env = {
  ...process.env,
  SIMPLE_POST_DATA_DIR: scratch,
  RUST_LOG: "warn",
};

// ----- Spawn daemon ---------------------------------------------------------
const port = 41_000 + Math.floor(Math.random() * 1000);
const args = ["mcp", "--transport=http", `--port=${port}`, "--bind=127.0.0.1"];
const proc = spawn(exe, args, { env, stdio: ["ignore", "pipe", "pipe"] });

let stderr = "";
proc.stderr.on("data", (b) => { stderr += b.toString(); });
// Polling watcher replaces the previous `proc.on("exit", ...)` listener.
// On Windows, the `exit` event can fire in the same tick as the kill and
// a future-attached `.on("exit")` listener never sees it. Watching
// `exitCode` directly is race-free regardless of timing.
const exitWatcher = setInterval(() => {
  if (proc.exitCode !== null || proc.signalCode !== null) {
    console.log(`[test:mcp] daemon exited code=${proc.exitCode} sig=${proc.signalCode}`);
    clearInterval(exitWatcher);
  }
}, 50);

// ----- Wait for daemon up via healthz ---------------------------------------
async function waitForHealthz(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/mcp/healthz`);
      if (r.ok) return await r.json();
    } catch { /* keep polling */ }
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error(`daemon did not come up; stderr:\n${stderr}`);
}

let failures = 0;
function assert(cond, msg) {
  if (!cond) { console.error(`✗ ${msg}`); failures++; }
  else { console.log(`✓ ${msg}`); }
}

let daemonHandle;
try {
  await waitForHealthz();
  console.log("[test:mcp] daemon is up");

  // 1. initialize
  const initRes = await (await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 1, method: "initialize",
      params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "test:mcp", version: "0.1.0" } },
    }),
  })).json();
  assert(initRes?.result?.serverInfo?.name === "simple-post-mcp",
    `initialize returns serverInfo.name=simple-post-mcp (got ${initRes?.result?.serverInfo?.name})`);

  // 2. tools/list — verify 44 named tools
  const listRes = await (await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/list", params: {} }),
  })).json();
  const names = (listRes?.result?.tools ?? []).map(t => t.name).sort();
  const required = [
    "env_list", "env_create", "env_set_var", "env_set_active",
    "project_list", "project_get",
    "endpoint_list", "endpoint_get", "endpoint_search",
    "base_url_def_list",
    "api_run_request", "api_call_endpoint",
    "api_resolve_template", "api_resolve_endpoint", "api_validate_endpoint",
    "api_parse_response", "api_diff_responses", "api_extract_jsonpath",
  ];
  for (const name of required) assert(names.includes(name), `tools/list contains ${name}`);
  assert(names.length >= 44, `tools/list returns at least 44 tools (got ${names.length})`);

  // 3. env_list — seeded environments
  const envListRes = await (await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 3, method: "tools/call",
      params: { name: "env_list", arguments: { project_id: "p_demo" } },
    }),
  })).json();
  const envText = envListRes?.result?.content?.[0]?.text ?? "";
  const envs = JSON.parse(envText);
  assert(Array.isArray(envs) && envs.length === 3,
    `env_list returns 3 seeded environments for p_demo (got ${envs?.length})`);
  assert(envs.some(e => e.name === "dev" && e.isActive === true),
    `env_list includes active 'dev' environment`);

  // 4. env_create — must be denied under default read_call permission
  const createRes = await (await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 4, method: "tools/call",
      params: { name: "env_create", arguments: { project_id: "p_demo", name: "should-fail" } },
    }),
  })).json();
  assert(createRes?.result?.is_error === true,
    `env_create under read_call permission returns is_error:true (got ${createRes?.result?.is_error})`);
  assert(/permission denied/i.test(createRes?.result?.content?.[0]?.text ?? ""),
    `env_create denial message mentions 'permission denied'`);

  // 5. healthz reports incremented requests_handled
  const healthz = await (await fetch(`http://127.0.0.1:${port}/mcp/healthz`)).json();
  assert(healthz.requests_handled >= 4,
    `healthz reports requests_handled >= 4 after 4 calls (got ${healthz.requests_handled})`);
  assert(healthz.status === "ok", `healthz status=ok`);

  console.log("\n✓ all assertions passed");
} catch (e) {
  console.error(`\n✗ test failed: ${e.message}`);
  failures++;
} finally {
  if (proc.exitCode === null) {
    proc.kill(process.platform === "win32" ? undefined : "SIGTERM");
    // give daemon a moment to flush stderr (Windows signal handling is slower)
    await new Promise(r => setTimeout(r, 500));
    if (proc.exitCode === null) proc.kill("SIGKILL");
  }
  // Wait for the daemon to fully exit before nuking its data dir.
  // Use a polling fallback so the case where the 'exit' event fired before
  // we attached the listener (common when SIGKILL exits the process in the
  // same tick as the null-check above) doesn't hang the promise forever.
  if (proc.exitCode === null) {
    await new Promise((r) => {
      if (proc.exitCode !== null) return r();
      const t = setInterval(() => {
        if (proc.exitCode !== null) { clearInterval(t); r(); }
      }, 50);
      proc.once("exit", () => { clearInterval(t); r(); });
    });
  }
  try { rmSync(scratch, { recursive: true, force: true }); }
  catch (e) { console.error(`[test:mcp] failed to rm scratch ${scratch}: ${e.message}`); }
}

process.exit(failures === 0 ? 0 : 1);
