import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { SRC, collectSourceFiles } from './_source-scan';

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

/**
 * 「全部刷新」按钮必须走 store 的那条动作，不许自己拼「解析 + 重载列表」。
 *
 * 出过的事故：面板自己拼的时候漏了 `force`，后端按 TTL 命中缓存，按钮按下去值动都不动。
 * 语义（强制重取、重载列表、与单条刷新同一套）只有写成一条动作才有地方断言；面板里拼
 * 一次就等于把第三条语义写进 JSX，而那里没有测试能站。
 */
test('全部刷新走 store 动作，面板不自己拼解析', () => {
  const panel = readFileSync(
    join(SRC, 'features', 'dynamic-variables', 'DynamicVariablesPanel.tsx'),
    'utf8',
  );
  assert.match(panel, /refreshAllDynamicVariables\(/, '「全部刷新」应当触发 store 的那条动作');
  assert.doesNotMatch(
    panel,
    /resolveDynamicVariables\(/,
    '面板不该自己发解析：漏一个参数（例如 force）就退化成读缓存，按钮按了等于没按',
  );
});
