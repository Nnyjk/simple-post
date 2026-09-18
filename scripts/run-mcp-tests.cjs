#!/usr/bin/env node
// scripts/run-mcp-tests.cjs — gated MCP integration test runner used as
// `npm run build:tests`. Exits 0 if the daemon/proxy binaries are present
// and all integration tests pass; warns and exits 0 if the binaries are
// missing so `npm run build` keeps working on a fresh checkout (cargo
// hasn't been run yet). This keeps `npm run build` green on first CI run
// while still surfacing regressions once the binary is in place.

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("node:child_process");

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

const run = (label) => {
  console.log(`\n[build:tests] === ${label} ===`);
  const res = spawnSync(process.execPath, [path.join(ROOT, "scripts", label === "mcp" ? "mcp-integration.test.mjs" : "proxy-integration.test.mjs")], {
    stdio: "inherit",
  });
  if (res.status !== 0) {
    console.error(`[build:tests] ${label} integration test FAILED (exit ${res.status})`);
    process.exit(res.status ?? 1);
  }
};

run("mcp");
run("proxy");

console.log("\n[build:tests] all MCP integration tests passed");
process.exit(0);
