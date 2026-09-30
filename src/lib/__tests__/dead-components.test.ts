import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

/**
 * Dead-code sentinel for the shared component library.
 *
 * This repo accumulated `components/ui/section.tsx` (116 lines, 5
 * exports) and `components/ui/prompt-dialog.tsx` (105 lines) with zero
 * importers. Nothing failed: typecheck passed, lint passed, all 188
 * tests passed. A component in that directory is only "a primitive" by
 * convention, so an unused one is indistinguishable from a work-in-
 * progress one until someone goes looking — and by then it has usually
 * been duplicated by hand somewhere else.
 *
 * This walks the real import graph instead of trusting convention. A
 * file is live if any non-test file under `src/` imports it, directly or
 * transitively. The transitive part matters: `KeyValueTable` is imported
 * by `BaseUrlTable` but by nothing else, and a naive one-hop check
 * would have called it dead and demanded its deletion.
 *
 * Deliberately conservative — it only asserts the strong, unambiguous
 * signal (imported by nobody at all), so a component that is exported
 * for a future caller but genuinely unreferenced will still fail. That
 * is the intended pressure: an unreferenced primitive should either
 * find a caller or go, not sit indefinitely.
 */

const SRC = resolve(process.cwd(), 'src');

/** Every .ts/.tsx file under src/, keyed by its module path without extension. */
function collectSourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      if (entry === '__tests__') continue;
      collectSourceFiles(p, out);
    } else if (/\.tsx?$/.test(entry) && !entry.endsWith('.d.ts')) {
      out.push(p);
    }
  }
  return out;
}

const files = collectSourceFiles(SRC);

/** `@/lib/utils` -> absolute path without extension. */
function resolveAlias(spec: string, fromFile: string): string | null {
  if (spec.startsWith('@/')) return join(SRC, spec.slice(2));
  if (spec.startsWith('.')) return resolve(join(fromFile, '..'), spec);
  return null; // bare package import
}

/** Module specifiers imported by a file, from both static and type imports. */
function importsOf(file: string): string[] {
  const src = readFileSync(file, 'utf8');
  const specs: string[] = [];
  const re = /(?:from|import)\s+['"]([^'"]+)['"]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) specs.push(m[1]);
  return specs;
}

const IMPORTED_BY = new Map<string, string[]>();
for (const file of files) {
  for (const spec of importsOf(file)) {
    const target = resolveAlias(spec, file);
    if (!target) continue;
    // Match on the extension-less module path; `foo` and `foo/index`
    // both resolve to the same entry.
    const key = target.replace(/\.tsx?$/, '');
    const list = IMPORTED_BY.get(key) ?? [];
    list.push(file);
    IMPORTED_BY.set(key, list);
  }
}

/** Does `entry` reach any file that is not itself part of the library? */
function isReachableFromApp(entry: string): boolean {
  const seen = new Set<string>([entry]);
  const queue = [entry];
  while (queue.length > 0) {
    const current = queue.pop()!;
    for (const importer of IMPORTED_BY.get(current) ?? []) {
      if (seen.has(importer)) continue;
      seen.add(importer);
      const isLibrary = importer.startsWith(join(SRC, 'components', 'ui'));
      if (!isLibrary) return true; // reached a feature or a store
      queue.push(importer);
    }
  }
  return false;
}

test('every component in components/ui is reachable from the app', () => {
  const uiDir = join(SRC, 'components', 'ui');
  const orphans: string[] = [];

  for (const entry of readdirSync(uiDir)) {
    if (!/\.tsx?$/.test(entry)) continue;
    const abs = join(uiDir, entry);
    if (!isReachableFromApp(abs.replace(/\.tsx?$/, ''))) {
      orphans.push(relative(process.cwd(), abs));
    }
  }

  assert.deepEqual(
    orphans,
    [],
    `unreachable component(s) in components/ui:\n  ${orphans.join('\n  ')}\n` +
      `Either wire them up or delete them — an unreferenced primitive is how\n` +
      `section.tsx and prompt-dialog.tsx accumulated unnoticed.`,
  );
});
