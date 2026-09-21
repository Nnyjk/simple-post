#!/usr/bin/env node
// packages/simple-post-mcp/bin/postinstall.mjs
//
// npm `postinstall` hook: pick the right native binary for the user's
// platform/arch and symlink (or copy on Windows) it into
// `native/current/simple-post-mcp(.exe)`. This makes the `bin/simple-post-mcp`
// wrapper (and direct `node_modules/.bin/simple-post-mcp` calls) always
// hit the correct architecture.
//
// The cargo release workflow (`.github/workflows/release-mcp.yml`)
// bundles one binary per (platform, arch) into the npm tarball under
// `native/<platform>-<arch>/`. The mapping below is the single source
// of truth — keep both this file and the workflow in sync.

import { promises as fs } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(HERE, "..");
const NATIVE_BASE = join(PKG_ROOT, "native");

const platformDir = (() => {
  switch (process.platform) {
    case "darwin":
      return "darwin";
    case "linux":
      return "linux";
    case "win32":
      return "windows";
    default:
      throw new Error(`unsupported platform: ${process.platform}`);
  }
})();

const archDir = (() => {
  switch (process.arch) {
    case "x64":
      return "x64";
    case "arm64":
      return "arm64";
    default:
      throw new Error(`unsupported arch: ${process.arch}`);
  }
})();

const binaryName = process.platform === "win32" ? "simple-post-mcp.exe" : "simple-post-mcp";

const sourceDir = join(NATIVE_BASE, `${platformDir}-${archDir}`);
const sourceBin = join(sourceDir, binaryName);

try {
  await fs.access(sourceBin);
} catch (e) {
  console.warn(
    `[simple-post-mcp] no native binary for ${platformDir}-${archDir} (looked in ${sourceDir}).\n` +
      `         Re-run with --force, or report @pin/your platform+arch on the issue tracker.`
  );
  // Don't fail `npm install` — the user can still ship the wrapper
  // without a binary and the runtime will surface a clean error.
  process.exit(0);
}

const currentDir = join(NATIVE_BASE, "current");
await fs.mkdir(currentDir, { recursive: true });
const targetBin = join(currentDir, binaryName);

// On Windows, symlinks require elevated rights; just copy.
if (process.platform === "win32") {
  await fs.copyFile(sourceBin, targetBin);
} else {
  try {
    await fs.unlink(targetBin);
  } catch (_) {
    /* missing — fine */
  }
  await fs.symlink(sourceBin, targetBin);
}

console.log(`[simple-post-mcp] linked ${sourceBin} -> ${targetBin}`);