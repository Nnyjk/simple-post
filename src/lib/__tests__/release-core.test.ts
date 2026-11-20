/**
 * 发版说明的生成规则。
 *
 * `npm run release` 的发布说明不再手写，而是这一版的提交标题
 * （`git log --pretty=format:%s v<上一版>..v<新版>`）。于是「哪些标题算数、归到哪一组、
 * 小标题叫什么」就是用户唯一会读到的那段文字的全部契约 —— 这些用例钉住它。
 *
 * 脚本本身是 `.mjs`（零依赖 CLI），类型声明在 `scripts/release-core.d.mts`。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  GROUP_ORDER,
  GROUP_TITLES,
  groupCommits,
  isMergeCommit,
  isReleaseCommit,
  mergeReleaseBody,
  parseCommitTitle,
  renderReleaseNotes,
  renderReleaseSection,
  resolveNextVersion,
} from '../../../scripts/release-core.mjs';

/** 一份覆盖全部六组的标题，故意乱序 —— 说明里的顺序只能由组序决定。 */
const TITLES = [
  'chore(release): 0.1.40',
  'docs: 补充 serve-web 的反向代理说明',
  'perf(ui): 树展开动画只补间可见的几行，批量动作直落终态',
  'test: 补 release-core 的用例',
  'fix(ui): 剪贴板复制在 WebView2 与非安全上下文下不再失败',
  'feat(dynamic): 动态变量支持指定环境',
  'refactor(db): 把节点删除的级联拆进一个事务',
  'Merge pull request #42 from Nnyjk/chore/release-0.1.39',
  'build(deps): 升级 tauri 到 2.9',
  'ci: docker 冒烟测试等 HEALTHCHECK 而不是 sleep',
  '更新 README',
];

test('分组顺序固定：新功能 → 修复 → 重构 → 性能 → 测试与构建 → 其它', () => {
  const groups = groupCommits(TITLES);
  assert.deepEqual(
    groups.map((g) => g.title),
    ['新功能', '修复', '重构', '性能', '测试与构建', '其它'],
  );
  // 组序表本身也是公开契约（release.mjs 之外的阅读者按它找小节）。
  assert.deepEqual(GROUP_ORDER, ['feat', 'fix', 'refactor', 'perf', 'build-test', 'other']);
  assert.equal(GROUP_TITLES['build-test'], '测试与构建');
});

test('`test` / `build` / `ci` 合成「测试与构建」，认不出的标题落进「其它」', () => {
  const byTitle = new Map(groupCommits(TITLES).map((g) => [g.title, g.entries]));
  assert.deepEqual(
    byTitle.get('测试与构建')?.map((e) => e.text),
    [
      '补 release-core 的用例',
      '**deps**: 升级 tauri 到 2.9',
      'docker 冒烟测试等 HEALTHCHECK 而不是 sleep',
    ],
  );
  // `docs` 有 type 但归不到前五组；末一条没有 Conventional Commits 形状 —— 两者都原样保留。
  assert.deepEqual(
    byTitle.get('其它')?.map((e) => e.text),
    ['补充 serve-web 的反向代理说明', '更新 README'],
  );
  assert.deepEqual(
    byTitle.get('新功能')?.map((e) => e.text),
    ['**dynamic**: 动态变量支持指定环境'],
  );
});

test('`chore(release)` 与 merge 提交被跳过（`chore` 的其它 scope 不算）', () => {
  assert.equal(isReleaseCommit('chore(release): 0.1.40'), true);
  assert.equal(isReleaseCommit('chore: 整理发行版依赖'), false);
  assert.equal(isMergeCommit('Merge pull request #42 from Nnyjk/chore/release-0.1.39'), true);
  assert.equal(isMergeCommit('merge: 合并分支'), false, '要的是 git 的 `Merge` 前缀，不是 type');

  const titles = groupCommits(TITLES).flatMap((g) => g.entries.map((e) => e.text));
  assert.equal(
    titles.some((text) => text.includes('0.1.40')),
    false,
  );
  assert.equal(
    titles.some((text) => text.includes('pull request')),
    false,
  );
  assert.equal(titles.length, TITLES.length - 2);
});

test('条目的 scope 加粗、不兼容变更带标注，正文里没有空小节', () => {
  const parsed = parseCommitTitle('feat(api)!: 去掉 v1 的兼容层');
  assert.deepEqual(parsed, {
    type: 'feat',
    scope: 'api',
    subject: '去掉 v1 的兼容层',
    breaking: true,
  });
  const section = renderReleaseSection(['feat(api)!: 去掉 v1 的兼容层', 'chore(release): 0.1.40']);
  assert.equal(section, '### 新功能\n\n- **api**: 去掉 v1 的兼容层（不兼容变更）');
  assert.equal(section.includes('### 修复'), false, '空组不输出');
});

test('空输入与全是发版提交：正文不是空字符串', () => {
  assert.deepEqual(groupCommits([]), []);
  assert.deepEqual(groupCommits(['chore(release): 0.1.40', 'Merge branch main']), []);
  const section = renderReleaseSection([]);
  assert.equal(section.includes('本次没有可归类的提交'), true);
  const notes = renderReleaseNotes([], '0.1.40');
  assert.equal(notes.startsWith('## Simple Post v0.1.40\n\n'), true);
  // 有标题、有正文、有收尾换行 —— 空说明不是空字符串，否则正文看起来像构建失败。
  assert.equal(notes.trimEnd(), `## Simple Post v0.1.40\n\n${section}`);
});

test('解析版本号：patch / minor / 显式三条路', () => {
  assert.equal(resolveNextVersion('0.1.39', 'patch'), '0.1.40');
  assert.equal(
    resolveNextVersion('0.1.39', undefined as unknown as string),
    '0.1.40',
    '缺省即 patch',
  );
  assert.equal(resolveNextVersion('0.1.39', 'minor'), '0.2.0');
  assert.equal(resolveNextVersion('0.1.39', '0.2.3'), '0.2.3');
  assert.equal(resolveNextVersion('0.1.39', '1.0.0'), '1.0.0');
});

test('非法版本参数报错', () => {
  for (const bad of ['', 'major', '0.1', '0.1.40-rc1', 'v0.1.40', '0.1.39', '0.1.3', 'x']) {
    assert.throws(() => resolveNextVersion('0.1.39', bad), /版本/);
  }
  assert.throws(() => resolveNextVersion('not-a-version', 'patch'), /不是 x\.y\.z/);
});

test('说明嵌进草案正文：保留安装说明，只替换 `### 这一版的改动` 一节', () => {
  const body = [
    '### 这一版的改动',
    '<!-- 发布前替换本节 -->',
    '',
    '## Simple Post v0.1.36',
    '',
    'Desktop API client.',
    '',
    '### Install',
    '- **macOS**: open the `.dmg`',
  ].join('\n');
  const merged = mergeReleaseBody(body, '### 新功能\n\n- 早点名量');
  assert.equal(merged.includes('<!-- 发布前替换本节 -->'), false);
  assert.equal(merged.includes('## Simple Post v0.1.36'), true);
  assert.equal(merged.includes('### Install'), true);
  assert.equal(merged.includes('- 早点名量'), true);
  // 占位节在正文最前面：合并后仍是它最前面，且只有一个「这一版的改动」标题。
  assert.equal(merged.split('### 这一版的改动').length, 2);
  assert.equal(merged.split('### 新功能').length, 2);
});

test('模板里没有占位节时，改动小节放最前而不是丢掉安装说明', () => {
  const merged = mergeReleaseBody(
    '## Simple Post v0.1.36\n\n### Install\n- x',
    '### 修复\n\n- 修好了',
  );
  assert.equal(merged.startsWith('### 修复'), true);
  assert.equal(merged.includes('### Install'), true);
  assert.equal(mergeReleaseBody('', '### 修复\n\n- 修好了'), '### 修复\n\n- 修好了\n');
});
