/**
 * Test runner: discover every `src/**\/*.test.ts` and hand the list to
 * `tsx --test` (Node's built-in test runner).
 *
 * This replaces the previous hardcoded file list in `package.json` so a
 * newly added test file is picked up by `npm test` automatically —
 * AGENTS.md used to document that footgun ("New test files are *not*
 * picked up until you add them to the script").
 *
 * Usage:
 *   node scripts/run-tests.mjs           # single run
 *   node scripts/run-tests.mjs --watch   # watch mode (same as npm run test:watch)
 *
 * Caveat: in `--watch` mode the file list is captured at spawn time;
 * restart the watch process after adding or renaming a test file.
 *
 * Paths are emitted repo-relative so they contain no spaces or shell
 * metacharacters — safe to join into a shell command on Windows, where
 * spawning a `.cmd` shim (node_modules/.bin/tsx.cmd) requires a shell.
 */
import { spawn } from 'node:child_process';
import { readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SRC = path.join(root, 'src');
const SKIP_DIRS = new Set(['node_modules', 'dist', 'target', 'build']);

/** @param {string} dir @param {string[]} out @returns {string[]} */
function collect(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return out; // src/ missing — fall through to the empty-list error below
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) collect(full, out);
    } else if (entry.isFile() && entry.name.endsWith('.test.ts')) {
      out.push(path.relative(root, full).replaceAll(path.sep, '/'));
    }
  }
  return out;
}

const files = collect(SRC).sort();
if (files.length === 0) {
  console.error('[test] no *.test.ts files found under src/');
  process.exit(1);
}

const watch = process.argv.includes('--watch');
const flags = watch ? ['--watch', '--test'] : ['--test'];
console.log(`[test] ${files.length} test file(s):`);
for (const f of files) console.log(`  - ${f}`);

// Build the full command string instead of passing an args array with
// `shell: true` — Node deprecated that combination (DEP0190). Safe
// because every argument is a repo-relative path without spaces or
// shell metacharacters (see header).
const command = ['tsx', ...flags, ...files].join(' ');
const child = spawn(command, {
  cwd: root,
  stdio: 'inherit',
  shell: true, // Windows: node_modules/.bin/tsx.cmd must go through a shell
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
child.on('error', (err) => {
  console.error(`[test] failed to spawn tsx: ${err.message}`);
  process.exit(1);
});
