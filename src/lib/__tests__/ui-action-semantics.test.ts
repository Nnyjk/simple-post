import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { SRC, collectSourceFiles, hits } from './_source-scan';

/**
 * 界面上那一下点击的语义，必须写在有测试的层（store 动作），不许在 JSX 里自己拼。
 *
 * 出过的事故就是这个形状：「全部刷新」在面板里自己拼 `resolveDynamicVariables(names, projectId)`，
 * 漏了第三参 `force`，后端按 TTL 命中缓存，按钮按下去值动都不动。这仓没有组件测试
 * （`src/` 下 0 个 `.test.tsx`），所以「拼在 JSX 里的语义」没有任何东西能站；写成 store 动作
 * 才有一处可以断言载荷与落桶。
 *
 * 这三条规则只认具体位置（本层的全部源码文件、三个具名对话框、「清倒计时紧跟推相位」这个
 * 形状），不写成「整仓不许出现某个字符串」——那种宽规则会把合法的同名字样一起打死。
 */

test('界面层不直接写 store 状态：带语义的改动走 store 动作', () => {
  const offenders = collectSourceFiles(join(SRC, 'features')).flatMap((file) =>
    hits(file, /useAppStore\.setState\(/),
  );

  assert.deepEqual(
    offenders,
    [],
    '这几个界面文件直接 setState 了 store：动作没有名字、也不在 src/stores/__tests__/ 里，' +
      '顺序（先重载、后展开之类）只能靠读 JSX 才发现。请新增一条命名的 store 动作并补行为用例',
  );
});

test('三个创建对话框只调收好的「提交并收起」动作', () => {
  const dialogs = [
    'features/create/CreateProjectDialog.tsx',
    'features/create/CreateModuleDialog.tsx',
    'features/create/CreateCollectionDialog.tsx',
  ];
  const problems: string[] = [];
  for (const rel of dialogs) {
    const src = readFileSync(join(SRC, rel), 'utf8');
    if (!src.includes('submitNewItemAndClose(')) {
      problems.push(`${rel}: 递交时应当调 store 的 submitNewItemAndClose()`);
    }
    // 分开写「提交 + 清空」时，顺序没有人守着：先清空就落成默认名，对话框照旧关掉。
    for (const line of hits(join(SRC, rel), /submitNewItem\(\)/)) problems.push(line);
  }

  assert.deepEqual(
    problems,
    [],
    '这些对话框自己拼了「提交 + closeNewItem()」：closeNewItem 先跑就会读到空草稿，' +
      '落成默认名（或干脆不建），而对话框照旧关掉，界面上看不出少了什么',
  );
});

test('开新连接不自己拼「清倒计时 + 推相位」', () => {
  const problems: string[] = [];
  for (const file of collectSourceFiles(join(SRC, 'features'))) {
    const lines = readFileSync(file, 'utf8').split('\n');
    lines.forEach((line, i) => {
      if (!/\.wsRetry\(/.test(line)) return;
      // 紧跟着推相位就是旧写法：两行之间的那一刻，重连倒计时还挂在一条正在连的会话上。
      if (!lines.slice(i + 1, i + 3).some((next) => /\.wsState\(/.test(next))) return;
      problems.push(`${file.slice(SRC.length + 1)}:${i + 1}: ${line.trim()}`);
    });
  }

  assert.deepEqual(
    problems,
    [],
    '这里先用 wsRetry 清倒计时、再 wsState 推相位：顺序没人测，' +
      '漏掉或写反就会让「第 N 次重试」挂在一条正在连的会话上。请调 store 的 wsConnecting(tabId)',
  );
});
