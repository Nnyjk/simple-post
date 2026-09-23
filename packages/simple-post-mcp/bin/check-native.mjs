#!/usr/bin/env node
// packages/simple-post-mcp/bin/check-native.mjs
//
// `prepublishOnly` guard: refuse to publish if no native binary is
// present for at least one (platform, arch). The release workflow
// populates `native/<platform>-<arch>/`; running `npm publish` from
// a dev checkout without that prep would ship a broken package.

import { promises as fs } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(HERE, '..');
const NATIVE_BASE = join(PKG_ROOT, 'native');

let hasAny = false;
try {
  const entries = await fs.readdir(NATIVE_BASE, { withFileTypes: true });
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    if (!/^[a-z]+-(x64|arm64)$/.test(e.name)) continue;
    const dir = join(NATIVE_BASE, e.name);
    for (const candidate of ['simple-post-mcp', 'simple-post-mcp.exe']) {
      try {
        await fs.access(join(dir, candidate));
        console.log(`[simple-post-mcp] ok: ${e.name}/${candidate}`);
        hasAny = true;
        break;
      } catch (_) {
        /* try next */
      }
    }
  }
} catch (_) {
  /* native/ doesn't exist yet — see below */
}

if (!hasAny) {
  console.error(
    '[simple-post-mcp] no native binaries found under native/. ' +
      'Run the cargo release workflow (or copy a build artifact manually) before publishing.',
  );
  process.exit(1);
}
