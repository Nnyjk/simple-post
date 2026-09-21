#!/usr/bin/env node
// scripts/dev-split.mjs — run Vite dev server + Rust serve-web together so
// an AI agent (or a browser pointed at http://localhost:5173) gets HMR
// from Vite while /api and /mcp requests are proxied (see `proxy` in
// vite.config.ts) to the Rust axum server on http://127.0.0.1:4173.
//
//   npm run dev:split
//
// Layout:
//   [vite]      → stdout/stderr from `npm run dev`           (port 5173)
//   [serve-web] → stdout/stderr from `cargo run … serve-web` (port 4173 — REST + MCP + SPA shell)
//
// Notes:
//   - First run of serve-web will compile the Rust binary (cargo build
//     step). That can take 10–60s on a cold cache. Subsequent runs are
//     incremental and quick.
//   - vite.config.ts proxies /api and /mcp → http://127.0.0.1:4173.
//     If serve-web isn't up yet, those requests 502 — that's expected.
//   - On Ctrl-C we SIGTERM both children, wait 3s, then SIGKILL any
//     survivors. On Windows we use `taskkill /T /F` because plain
//     TerminateProcess on the parent doesn't reach grandchildren
//     (cargo→simple-post.exe, npm.cmd→node.exe → vite).
//
// Pure stdlib. No concurrently, no cross-spawn — keeps the dev deps lean.

import { spawn } from "node:child_process";
import process from "node:process";
import { join } from "node:path";

const REPO = join(import.meta.dirname, "..");
const VITE_PREFIX = "[vite] ";
const SERVE_PREFIX = "[serve-web] ";
const SHUTDOWN_GRACE_MS = 3000;

// Track every child we spawn so the signal handler / exit handler can
// reach all of them.
const children = [];
let shuttingDown = false;
let forceKillTimer = null;
// Default to 0; overwritten the moment a child exits non-cleanly.
let exitCode = 0;

function prefixWrite(stream, chunk, prefix) {
  // Prefix the whole chunk in one go. Splitting on newlines would lose
  // partial-line ordering between stdout and stderr; for a dev tool this
  // is plenty.
  stream.write(prefix + chunk.toString());
}

function spawnChild(label, cmd, args) {
  const prefix = label === "vite" ? VITE_PREFIX : SERVE_PREFIX;
  console.error(`[dev-split] starting ${label}: ${cmd} ${args.join(" ")}`);
  // Node 20+ (post-CVE-2024-27980) refuses to spawn .cmd/.bat without
  // shell: true. cargo and vite (when run directly) don't have that
  // problem; npm on Windows IS a .cmd, so we need the shell. Safe here:
  // all args are hardcoded, no user input flows through.
  const needsShell = process.platform === "win32" && /\.(cmd|bat)$/i.test(cmd);
  const child = spawn(cmd, args, {
    cwd: REPO,
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    shell: needsShell,
  });
  const handle = { label, child, prefix };
  children.push(handle);

  child.stdout.on("data", (b) => prefixWrite(process.stdout, b, prefix));
  child.stderr.on("data", (b) => prefixWrite(process.stderr, b, prefix));

  child.on("exit", (code, signal) => {
    if (shuttingDown) return; // we triggered the exit; ignore
    shuttingDown = true;
    if (code === 0 || signal === "SIGTERM" || signal === "SIGINT") {
      console.error(`[dev-split] ${label} exited cleanly (code=${code} signal=${signal})`);
    } else {
      console.error(`[dev-split] ${label} exited with code=${code} signal=${signal}`);
      exitCode = code ?? 1;
    }
    shutdownAll();
  });

  child.on("error", (err) => {
    // ENOENT etc. — surface immediately, don't silently hang.
    console.error(`[dev-split] ${label} failed to spawn: ${err.message}`);
    if (!shuttingDown) {
      shuttingDown = true;
      exitCode = 127;
      shutdownAll();
    }
  });

  return child;
}

// Send SIGTERM (or taskkill-equivalent) to every still-live child.
function shutdownAll() {
  for (const { child } of children) {
    if (child.exitCode !== null) continue;
    killTree(child, /* force = */ false);
  }
  if (forceKillTimer) clearTimeout(forceKillTimer);
  forceKillTimer = setTimeout(() => {
    for (const { label, child } of children) {
      if (child.exitCode === null) {
        console.error(
          `[dev-split] ${label} still alive after ${SHUTDOWN_GRACE_MS}ms — force killing`,
        );
        killTree(child, /* force = */ true);
      }
    }
    process.exit(exitCode || 0);
  }, SHUTDOWN_GRACE_MS);
}

// Cross-platform "kill this process and all of its children".
function killTree(child, force) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === "win32") {
    // /T = walk tree (kill grandchildren too — cargo→simple-post.exe,
    // npm.cmd→node.exe→vite). /F = force. No graceful variant on
    // Windows; /T /F is the closest equivalent to SIGKILL.
    try {
      const killer = spawn("taskkill", ["/pid", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
      killer.on("error", () => { /* already dead */ });
    } catch { /* already dead */ }
    return;
  }
  try {
    child.kill(force ? "SIGKILL" : "SIGTERM");
  } catch { /* already dead */ }
}

process.on("SIGINT", () => {
  if (shuttingDown) return;
  shuttingDown = true;
  console.error("\n[dev-split] SIGINT received — shutting down children");
  shutdownAll();
});

process.on("SIGTERM", () => {
  if (shuttingDown) return;
  shuttingDown = true;
  console.error("\n[dev-split] SIGTERM received — shutting down children");
  shutdownAll();
});

// ----- Boot -----------------------------------------------------------------
// 1. serve-web first. Its port (4173) has to be listening before the
//    browser opens localhost:5173, otherwise Vite's proxy 502s on the
//    first /api or /mcp hit. We don't block the Vite spawn on readiness
//    — Vite comes up instantly and proxies will reconnect once axum
//    binds.
console.error("[dev-split] hint: first cargo build of serve-web can take 10–60s");
spawnChild(
  "serve-web",
  "cargo",
  [
    "run",
    "--manifest-path", "src-tauri/Cargo.toml",
    "--bin", "simple-post",
    "--",
    "serve-web",
    "--bind", "127.0.0.1",
    "--port", "4173",
    "--spa-dir", "./dist",
  ],
);

// 2. Vite. `npm` resolves to `npm.cmd` on Windows automatically since
//    Node 16, but we name it explicitly to be safe across CI runners.
const npmCmd = process.platform === "win32" ? "npm.cmd" : "npm";
spawnChild("vite", npmCmd, ["run", "dev"]);
