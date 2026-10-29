import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

/**
 * 动态变量解析的调用点必须点名项目。
 *
 * `resolveDynamicVariables(names, projectId?, force?)` 的第二个参数缺席时，store 会跨项目
 * 查找同名定义——那正是「同名互相覆盖」这条要消除的行为。三个调用点（面板、预览、发送）
 * 都点名了项目，故这里钉住的是「不许再多出一个忘了传的调用点」。
 *
 * 读源码而不是跑界面：这条不变式说的是「调用点有没有传第二个参数」，源码里一眼可见；
 * 而界面测试要真的发一次请求，才观察得到替换进去的是哪一条定义。
 */
const SRC = join(process.cwd(), 'src');

/** `src/` 下每个 .ts/.tsx 文件（跳过测试目录——那里的调用是故意的对照组）。 */
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

test('动态变量解析的每个调用点都点名了项目', () => {
  const offenders: string[] = [];
  for (const file of collectSourceFiles(SRC)) {
    const src = readFileSync(file, 'utf8');
    // 只认「函数名紧跟左括号」的调用写法：store 里的定义是
    // `resolveDynamicVariables: async (…) => …`，取函数值的写法是 `s.resolveDynamicVariables)`，
    // 两者都不匹配这条正则，故命中的都是真的调用点。
    const re = /resolveDynamicVariables\(([^)]*)\)/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(src)) !== null) {
      const args = m[1];
      if (!args.includes(',')) {
        offenders.push(`${file.slice(SRC.length + 1)}: resolveDynamicVariables(${args})`);
      }
    }
  }
  assert.deepEqual(
    offenders,
    [],
    '这些调用点没有传项目 id，解析会跨项目找同名定义，别的项目的同名变量可能顶替它',
  );
});
