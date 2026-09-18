#!/usr/bin/env node
// scripts/run-mcp-tests.cjs — gated MCP integration test runner used as
// `npm run build:tests`. Exits 0 if the daemon/proxy binaries are present
// and all integration tests pass; warns and exits 0 if the binaries are
// missing so `npm run build` keeps working on a fresh checkout (cargo
// hasn't been run yet). This keeps `npm run build` green on first CI run
// while still surfacing regressions once the binary is in place.

const fs = require("fs");
const path = require("path");
const { spawn } = require("node:child_process");

const ROOT = path.resolve(__dirname, "..");
const DEBUG_DIR = path.join(ROOT, "src-tauri", "target", "debug");
const exeName = process.platform === "win32" ? "simple-post.exe" : "simple-post";
const proxyName = process.platform === "win32" ? "simple-post-mcp-proxy.exe" : "simple-post-mcp-proxy";
const daemonBin = path.join(DEBUG_DIR, exeName);
const proxyBin = path.join(DEBUG_DIR, proxyName);

const haveDaemon = fs.existsSync(daemonBin);
const haveProxy = fs.existsSync(proxyBin);

if (!haveDaemon || !haveProxy) {
  console.warn("[build:tests] MCP integration tests skipped:");
  if (!haveDaemon) console.warn(`[build:tests]   missing ${daemonBin}`);
  if (!haveProxy)  console.warn(`[build:tests]   missing ${proxyBin}`);
  console.warn("[build:tests] run `cargo build --manifest-path src-tauri/Cargo.toml --bins` to enable them.");
  process.exit(0);
}

// Wall-clock cap per test. spawn()'s `timeout` is a child-side kill signal but
// can be racy on Windows when the child is wedged in a syscall; the explicit
// Promise.race below is the wrapper's last-resort safety net.
const WALL_MS = 80_000;

// Run one integration test as an async spawn, piping its output live AND
// retaining the last `ctx.stderrTail` lines for the post-mortem on exit. Resolves
// to { status, signal, stderr, timedOut }.
const run = (label) => new Promise((resolveRun) => {
  console.log(`\n[build:tests] === ${label} ===`);
  const script = label === "mcp"
    ? path.join(ROOT, "scripts", "mcp-integration.test.mjs")
    : path.join(ROOT, "scripts", "proxy-integration.test.mjs");
  const child = spawn(process.execPath, [script], {
    // We deliberately do NOT use `stdio: "inherit"` here: the previous
    // wrapper inherited the parent's stdio, so on Windows the spawnSync
    // return could race ahead of buffered stderr writes — losing the
    // signal needed to diagnose failures. Pipe both streams instead, mirror
    // them live, and keep the last 20 lines for the post-mortem.
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  let out = "";
  child.stdout.on("data", (b) => { out += b.toString(); process.stdout.write(b); });
  child.stderr.on("data", (b) => { stderr += b.toString(); process.stderr.write(b); });

  let settled = false;
  const finish = (payload) => {
    if (settled) return;
    settled = true;
    clearTimeout(wallClock);
    resolveRun(payload);
  };

  const wallClock = setTimeout(() => {
    try { child.kill("SIGKILL"); } catch { /* already dead */ }
    console.error(`[build:tests] ${label} exceeded ${WALL_MS / 1000}s wall-clock; sending SIGKILL`);
    // Give the kill a brief grace window for the OS to settle, then resolve
    // with status 124 (the conventional `timeout` exit code) regardless of
    // whether the child has actually exited yet — the wrapper must never
    // hang forever.
    setTimeout(() => finish({ status: 124, signal: null, stderr, timedOut: true }), 1_000);
  }, WALL_MS);

  child.on("exit", (code, signal) => {
    finish({ status: code, signal, stderr, timedOut: false });
  });
  child.on("error", (err) => {
    console.error(`[build:tests] ${label} spawn error: ${err.message}`);
    finish({ status: 1, signal: null, stderr: stderr + `\n${err.message}\n`, timedOut: false });
  });
});

(async () => {
  for (const label of ["mcp", "proxy"]) {
    const res = await run(label);
    if (res.status === 0) continue;
    console.error(`[build:tests] ${label} integration test FAILED (exit ${res.status}${res.timedOut ? ' — wall-clock timeout' : ''})`);
    if (res.stderr) {
      const lines = res.stderr.split(/\r?\n/);
      const tail = lines.slice(Math.max(0, lines.length - 21), lines.length - 1);
      if (tail.length > 0) {
        console.error(`[build:tests] --- last ${tail.length} line(s) of ${label} stderr ---`);
        for (const line of tail) console.error(line);
        console.error(`[build:tests] --- end ${label} stderr tail ---`);
      }
    }
    process.exit(res.status ?? 1);
  }
  console.log("\n[build:tests] all MCP integration tests passed");
  process.exit(0);
})().catch((e) => {
  console.error(`[build:tests] wrapper crashed: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
});