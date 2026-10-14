import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

/**
 * 共享组件库的死代码哨兵。
 *
 * `components/ui` 里的文件只靠约定算「原语」，所以一个没人 import 的原语与一个
 * 写了一半的原语看不出来，而没人去找时它通常已经被手工复制到别处了。这里走真实
 * import 图而不是信约定：`src/` 下只要有任一非测试文件（直接或传递）import 它，
 * 它就算活的。传递那部分是必需的——`KeyValueTable` 只被设置页表格 import，
 * 只看一跳会判它死并要求删除。
 *
 * 刻意保守：只断言那个强而无歧义的信号（完全没有 import 者）。所以为将来调用方
 * 导出、但确实没有引用的组件仍会失败。这正是想要的压力——没人用的原语要么找到
 * 调用方，要么走掉，不该无限期挂着。
 */

const SRC = resolve(process.cwd(), 'src');

/** `src/` 下每个 .ts/.tsx 文件，键为去掉扩展名的模块路径。 */
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

/** `@/lib/utils` → 不带扩展名的绝对路径。 */
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
    // 按无扩展名的模块路径匹配；`foo` 与 `foo/index` 指向同一个入口
    const key = target.replace(/\.tsx?$/, '');
    const list = IMPORTED_BY.get(key) ?? [];
    list.push(file);
    IMPORTED_BY.set(key, list);
  }
}

/** `entry` 能否到达任何一个不属于该库的文件？ */
function isReachableFromApp(entry: string): boolean {
  const seen = new Set<string>([entry]);
  const queue = [entry];
  while (queue.length > 0) {
    const current = queue.pop()!;
    for (const importer of IMPORTED_BY.get(current) ?? []) {
      if (seen.has(importer)) continue;
      seen.add(importer);
      const isLibrary = importer.startsWith(join(SRC, 'components', 'ui'));
      if (!isLibrary) return true; // 到达了 feature 或 store
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
