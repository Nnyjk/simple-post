#!/usr/bin/env node
// scripts/test-web-auth.mjs — verify commit 5's bearer auth flow.
//
//   node scripts/test-web-auth.mjs
//
// Asserts:
//   - with no SIMPLE_POST_WEB_PASSWORD set, /api/projects is open
//     (backward-compatible default — same as commit 4)
//   - with SIMPLE_POST_WEB_PASSWORD=secret:
//       GET /api/projects       → 401 (no token)
//       GET /api/projects + bad → 401
//       GET /api/projects + OK  → 200 (token = constant-time compare)
//       GET /api/healthz        → 200 always (probes stay open)
//       GET /                   → 200 always (SPA shell stays open)
//       401 response carries `WWW-Authenticate: Bearer realm="simple-post"`
//
// The script spawns the binary twice (once without password, once with)
// because there is no in-process toggle to flip the password at runtime —
// the env var is read once at boot. A 2-process lifecycle is the same
// pattern mcp-integration.test.mjs uses (commit 4).

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, existsSync } from "node:fs";
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
  console.error(`[test:web-auth] simple-post binary not found (looked for debug + release builds).`);
  console.error(`[test:web-auth] run \`cargo build\` (in src-tauri/) first.`);
  process.exit(2);
}

const PASSWORD = "shh-this-is-secret-9c8a";
const BASE = "http://127.0.0.1:4173";

let failures = 0;
function assert(cond, msg) {
  if (!cond) { console.error(`✗ ${msg}`); failures++; }
  else { console.log(`✓ ${msg}`); }
}

function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

async function waitForHttp(timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`${BASE}/api/healthz`);
      if (r.ok) return;
    } catch { /* keep polling */ }
    await sleep(100);
  }
  throw new Error(`simple-post serve-web did not come up at ${BASE}`);
}

async function killGracefully(proc, label) {
  if (!proc || proc.exitCode !== null || proc.signalCode !== null) return;
  proc.kill("SIGTERM");
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null || proc.signalCode !== null) {
      console.log(`[test:web-auth] ${label} exited code=${proc.exitCode} sig=${proc.signalCode}`);
      return;
    }
    await sleep(50);
  }
  proc.kill("SIGKILL");
}

// ----- Phase 1: no password (backward-compat) ------------------------------
const scratch1 = mkdtempSync(join(tmpdir(), "web-auth-open-"));
console.log(`[test:web-auth] phase 1 scratch = ${scratch1}`);
const proc1 = spawn(exe, ["serve-web", "--bind=127.0.0.1", "--port=4173"], {
  env: { ...process.env, SIMPLE_POST_DATA_DIR: scratch1, RUST_LOG: "warn" },
  stdio: ["ignore", "pipe", "pipe"],
});
let stderr1 = "";
proc1.stderr.on("data", (b) => { stderr1 += b.toString(); });

try {
  await waitForHttp();
  console.log("[test:web-auth] phase 1: server up (no password)");

  const r = await fetch(`${BASE}/api/projects`);
  assert(r.status === 200,
    `phase 1: GET /api/projects returns 200 (got ${r.status})`);

  // healthz + SPA shell are always public, no matter the auth tier.
  const h = await fetch(`${BASE}/api/healthz`);
  assert(h.status === 200, `phase 1: /api/healthz is 200 (got ${h.status})`);
  const body = await h.json();
  assert(body?.status === "ok",
    `phase 1: /api/healthz body.status === "ok" (got ${JSON.stringify(body)})`);
} finally {
  await killGracefully(proc1, "phase 1");
  rmSync(scratch1, { recursive: true, force: true });
}

// ----- Phase 2: SIMPLE_POST_WEB_PASSWORD set --------------------------------
const scratch2 = mkdtempSync(join(tmpdir(), "web-auth-locked-"));
console.log(`[test:web-auth] phase 2 scratch = ${scratch2}`);
const proc2 = spawn(exe, ["serve-web", "--bind=127.0.0.1", "--port=4173"], {
  env: {
    ...process.env,
    SIMPLE_POST_DATA_DIR: scratch2,
    SIMPLE_POST_WEB_PASSWORD: PASSWORD,
    RUST_LOG: "warn",
  },
  stdio: ["ignore", "pipe", "pipe"],
});
let stderr2 = "";
proc2.stderr.on("data", (b) => { stderr2 += b.toString(); });

try {
  await waitForHttp();
  console.log(`[test:web-auth] phase 2: server up (password set)`);

  // 2a — no token → 401
  const r401 = await fetch(`${BASE}/api/projects`);
  assert(r401.status === 401,
    `phase 2: GET /api/projects (no token) returns 401 (got ${r401.status})`);
  const wwwAuth = r401.headers.get("www-authenticate") ?? "";
  assert(wwwAuth.toLowerCase().includes("bearer"),
    `phase 2: 401 carries WWW-Authenticate: Bearer (got ${JSON.stringify(wwwAuth)})`);

  // 2b — wrong token → 401 (constant-time compare must reject mismatches)
  const rWrong = await fetch(`${BASE}/api/projects`, {
    headers: { authorization: "Bearer wrong-password" },
  });
  assert(rWrong.status === 401,
    `phase 2: GET /api/projects (wrong token) returns 401 (got ${rWrong.status})`);

  // 2c — right token → 200
  const rOk = await fetch(`${BASE}/api/projects`, {
    headers: { authorization: `Bearer ${PASSWORD}` },
  });
  assert(rOk.status === 200,
    `phase 2: GET /api/projects (correct token) returns 200 (got ${rOk.status})`);

  // 2d — healthz stays open under password
  const rHealthz = await fetch(`${BASE}/api/healthz`);
  assert(rHealthz.status === 200,
    `phase 2: /api/healthz stays 200 under password (got ${rHealthz.status})`);

  // 2e — SPA shell stays open under password
  const rSpa = await fetch(`${BASE}/`);
  assert(rSpa.status === 200,
    `phase 2: GET / (SPA shell) stays 200 under password (got ${rSpa.status})`);
  const ctype = rSpa.headers.get("content-type") ?? "";
  assert(ctype.toLowerCase().includes("text/html"),
    `phase 2: GET / returns text/html (got ${JSON.stringify(ctype)})`);

  // 2f — /mcp healthz also stays open (MCP probes need it)
  const rMcpH = await fetch(`${BASE}/mcp/healthz`);
  assert(rMcpH.status === 200,
    `phase 2: /mcp/healthz stays 200 under password (got ${rMcpH.status})`);
} finally {
  await killGracefully(proc2, "phase 2");
  rmSync(scratch2, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`\n[test:web-auth] FAILED with ${failures} assertion failure(s)`);
  process.exit(1);
}
console.log(`\n[test:web-auth] all assertions passed`);