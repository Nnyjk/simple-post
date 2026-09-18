#!/usr/bin/env node
// scripts/proxy-integration.test.mjs — spawn the stdio↔HTTP proxy binary
// with stdin piped to a Node-side JSON-RPC stream, verify it forwards
// frames to the running daemon and writes responses back to stdout.
//
//   npm run test:proxy
//
// This is what Claude Desktop / Cursor does in production — they spawn
// `simple-post-mcp-proxy.exe` and talk stdio to it. If the proxy can't
// relay a round-trip, the AI agent can never reach a tool.

import { spawn } from "node:child_process";
import { mkdtempSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";

const REPO = join(import.meta.dirname, "..");
const DAEMON_EXE = join(REPO, "src-tauri", "target", "debug",
  process.platform === "win32" ? "simple-post.exe" : "simple-post");
const PROXY_EXE = join(REPO, "src-tauri", "target", "debug",
  process.platform === "win32" ? "simple-post-mcp-proxy.exe" : "simple-post-mcp-proxy");

if (!existsSync(DAEMON_EXE) || !existsSync(PROXY_EXE)) {
  console.error(`[test:proxy] missing daemon or proxy binary — run \`cargo build --bins\` first.`);
  process.exit(2);
}

const scratch = mkdtempSync(join(tmpdir(), "mcp-proxy-it-"));
console.log(`[test:proxy] scratch = ${scratch}`);

const env = { ...process.env, SIMPLE_POST_DATA_DIR: scratch, RUST_LOG: "warn" };

// 1. Boot the daemon so port_registry exists.
const port = 41_500 + Math.floor(Math.random() * 500);
const daemon = spawn(DAEMON_EXE, ["mcp", "--transport=http", `--port=${port}`, "--bind=127.0.0.1"], {
  env, stdio: ["ignore", "pipe", "pipe"],
});
// Track the proxy handle for the cleanup paths below.
let proxy = null;
daemon.stderr.on("data", (b) => { if (process.env.VERBOSE) process.stderr.write(`[daemon] ${b}`); });

async function waitForHealthz(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/mcp/healthz`);
      if (r.ok) return true;
    } catch { /* keep polling */ }
    await new Promise(r => setTimeout(r, 100));
  }
  return false;
}

// Race-free reap helper. On Windows, attaching `proc.once("exit", ...)` AFTER
// the kill has been issued can miss the emit entirely (the `exit` event has
// already fired and won't re-fire). Polling `proc.exitCode` short-circuits the
// common case and the listener catches the rest.
async function waitForExit(proc) {
  if (proc.exitCode !== null) return;
  await new Promise((r) => {
    if (proc.exitCode !== null) return r();
    const t = setInterval(() => {
      if (proc.exitCode !== null) { clearInterval(t); r(); }
    }, 50);
    proc.once("exit", () => { clearInterval(t); r(); });
  });
}

let failures = 0;
function assert(cond, msg) {
  if (!cond) { console.error(`✗ ${msg}`); failures++; }
  else { console.log(`✓ ${msg}`); }
}

try {
  const up = await waitForHealthz();
  assert(up, "daemon came up");

  // 2. Spawn the proxy with stdio piped. AI clients do the same.
  proxy = spawn(PROXY_EXE, [`--port=${port}`], { env, stdio: ["pipe", "pipe", "pipe"] });
  let stderr = "";
  proxy.stderr.on("data", (b) => { stderr += b.toString(); });

  // Frame accumulator — JSON-RPC over stdio is line-delimited (each
  // tool response is one JSON object on its own line).
  let buf = "";
  const responses = new Map(); // id → resolve fn
  proxy.stdout.setEncoding("utf8");
  proxy.stdout.on("data", (chunk) => {
    buf += chunk;
    let nl;
    while ((nl = buf.indexOf("\n")) !== -1) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line);
        if (typeof msg.id === "number" && responses.has(msg.id)) {
          responses.get(msg.id)(msg);
          responses.delete(msg.id);
        }
      } catch (e) {
        console.error(`[test:proxy] non-JSON line from proxy: ${line}`);
      }
    }
  });

  function send(method, params, id) {
    return new Promise((resolve, reject) => {
      const frame = JSON.stringify({ jsonrpc: "2.0", id, method, params });
      responses.set(id, resolve);
      proxy.stdin.write(frame + "\n");
      setTimeout(() => {
        if (responses.has(id)) {
          responses.delete(id);
          reject(new Error(`timeout waiting for id=${id}`));
        }
      }, 5000);
    });
  }

  // 3. Drive the proxy the same way Claude Desktop would.
  const init = await send("initialize", {
    protocolVersion: "2025-03-26",
    capabilities: {},
    clientInfo: { name: "test:proxy", version: "0.1.0" },
  }, 1);
  assert(init?.result?.serverInfo?.name === "simple-post-mcp",
    `proxy relayed initialize → serverInfo.name=simple-post-mcp`);

  const listed = await send("tools/list", {}, 2);
  const names = (listed?.result?.tools ?? []).map(t => t.name);
  assert(names.length >= 44, `proxy relayed tools/list with ≥44 tools (got ${names.length})`);

  const envs = await send("tools/call", {
    name: "env_list", arguments: { project_id: "p_demo" },
  }, 3);
  const envText = envs?.result?.content?.[0]?.text ?? "[]";
  const parsed = JSON.parse(envText);
  assert(parsed.length === 3, `proxy relayed env_list with 3 seeded envs (got ${parsed.length})`);

  console.log("\n✓ all assertions passed");
} catch (e) {
  console.error(`\n✗ test failed: ${e.message}`);
  failures++;
} finally {
  // Kill the proxy first — close stdin so its EOF path fires before we escalate.
  if (proxy && proxy.exitCode === null) {
    try { proxy.stdin.end(); } catch { /* ignore */ }
    proxy.kill(process.platform === "win32" ? undefined : "SIGTERM");
    // give proxy a moment to flush stderr (Windows signal handling is slower)
    await new Promise(r => setTimeout(r, 500));
    if (proxy.exitCode === null) proxy.kill("SIGKILL");
    // ensure full exit before cleaning daemon data dir (polling fallback for
    // the case where the exit event already fired before this listener).
    await waitForExit(proxy);
  }
  if (daemon.exitCode === null) {
    daemon.kill(process.platform === "win32" ? undefined : "SIGTERM");
    await new Promise(r => setTimeout(r, 500));
    if (daemon.exitCode === null) daemon.kill("SIGKILL");
    await waitForExit(daemon);
  }
  try { rmSync(scratch, { recursive: true, force: true }); }
  catch (e) { console.error(`[test:proxy] failed to rm scratch ${scratch}: ${e.message}`); }
}

process.exit(failures === 0 ? 0 : 1);
