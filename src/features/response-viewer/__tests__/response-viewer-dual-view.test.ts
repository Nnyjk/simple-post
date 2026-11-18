/**
 * 响应体双视图的源码级回归网。
 *
 * 为什么只能读源码：本 runner（`tsx --test` + `node:assert`）没有 DOM，也没有 React 渲染器，
 * 测不了「右键菜单里到底有几条」「工具条浮在右上角」这类渲染结果。可变的部分（路径、预览、
 * 子树 JSON）都在 `src/lib/json-tree.ts` 里做成了纯函数，由 `src/lib/__tests__/json-tree.test.ts`
 * 直接断言；这里只钉住「谁挂在谁身上」这种一改就静默失效的接线。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const read = (relative: string) =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), 'utf8');

const VIEWER = read('../ResponseViewer.tsx');
const PRETTY = read('../PrettyView.tsx');

/** 剥掉整行 `//` 注释与单行 JSX 块注释，免得文件自己的散文满足某个模式。 */
function code(source: string): string {
  return source.replace(/\{\s*\/\*[^\n]*?\*\/\s*\}/g, '').replace(/^\s*\/\/.*$/gm, '');
}

/**
 * 源码里每个 `<PrettyView … />` 的属性文本，从开标签到自闭合的 `/>`，连同它在源码里的
 * 起始位置。位置要一起带出来：只靠文本去找位置的话，两个一模一样的挂载点会都映射到
 * 第一处，于是两个分支的断言其实在证明同一件事。
 */
function prettyViewBlocks(source: string): { at: number; text: string }[] {
  const blocks: { at: number; text: string }[] = [];
  let at = source.indexOf('<PrettyView');
  while (at !== -1) {
    const end = source.indexOf('/>', at);
    assert.notEqual(end, -1, '<PrettyView> 必须自闭合');
    blocks.push({ at, text: source.slice(at, end) });
    at = source.indexOf('<PrettyView', end);
  }
  return blocks;
}

describe('ResponseViewer: pretty/raw 两态都交给 PrettyView', () => {
  it('pretty 分支与 raw 的 JSON 分支都渲染 PrettyView', () => {
    const body = code(VIEWER);
    assert.match(body, /<PrettyView\b[^>]*view="pretty"/, 'pretty 分支没渲染 PrettyView');
    assert.match(body, /<PrettyView\b[^>]*view="raw"/, 'raw 分支没渲染 PrettyView');
  });

  it('文本响应也交给 PrettyView，并声明它不是 JSON', () => {
    const body = code(VIEWER);
    const blocks = prettyViewBlocks(body);
    assert.equal(blocks.length, 4, 'pretty/raw 两页的 json 与 text 分支都应渲染 PrettyView');
    // 每个挂载点都要对着它自己的分支断言：只数总次数的话，把 expectJson 从 text 挪到 json
    // 仍然通过，而纯文本响应会重新长出一条「格式化」警告。
    const branches = blocks.map(({ at, text }) => {
      // 挂载点属于哪个分支，看它之前最近的那个 `kind === '…'`：pretty 组的 json 分支在最前，
      // 它之前没有 text 分支（`textAt` 为 -1），最近的那个仍然正确地是 json。
      const before = body.slice(0, at);
      const jsonAt = before.lastIndexOf("kind === 'json'");
      const textAt = before.lastIndexOf("kind === 'text'");
      assert.notEqual(jsonAt, -1, '找不到挂载点所属的分支');
      // 页签也照同一套办法从最近的分支标记推出，再要求挂载点写的 view 与之一致 —— 若直接
      // 从挂载点自己的 `view="…"` 反推，标签与被断言的内容同源，写错也照样「一致」。
      const prettyAt = before.lastIndexOf("view === 'pretty'");
      const rawAt = before.lastIndexOf("view === 'raw'");
      assert.ok(prettyAt !== -1 || rawAt !== -1, '找不到挂载点所属的页签');
      return {
        text,
        kind: jsonAt > textAt ? 'json' : 'text',
        tab: prettyAt > rawAt ? 'pretty' : 'raw',
      };
    });
    for (const branch of branches) {
      assert.equal(
        branch.text.includes(`view="${branch.tab}"`),
        true,
        `${branch.tab} 页签下的 ${branch.kind} 分支挂载点写错了 view`,
      );
      assert.equal(
        branch.text.includes('expectJson={false}'),
        branch.kind === 'text',
        `${branch.tab} 的 ${branch.kind} 分支${branch.kind === 'text' ? '必须' : '不该'}传 expectJson={false}`,
      );
    }
    // 四种组合各一份，否则上面的断言可能对着重复的两个分支证明同一件事。
    assert.deepEqual([...new Set(branches.map((b) => `${b.tab}/${b.kind}`))].sort(), [
      'pretty/json',
      'pretty/text',
      'raw/json',
      'raw/text',
    ]);
    // 裸 pre 一出现就说明某个分支绕开了工具条，用户看到的工具条会整条消失。
    assert.equal(/<pre\b/.test(body), false, '响应体分支不得再自己渲染 pre');
  });

  it('页签条不再放「原文/格式化」按钮，开关改由宿主统一下发', () => {
    const body = code(VIEWER);
    assert.equal(
      body.includes('format-response-btn'),
      false,
      '页签条的格式化按钮应该已经搬进 raw 页',
    );
    // 四个挂载点共用同一组开关：各自持有一份的话，切一次页签就换了一份。
    const spread = body.match(/\{\.\.\.bodyViewProps\}/g) ?? [];
    assert.equal(spread.length, 4, 'pretty/raw × json/text 四处都要收同一组开关');
    assert.match(body, /const \[wrapBody, setWrapBody\] = useState\(false\)/, '换行状态要在宿主');
    assert.match(
      body,
      /const \[formatted, setFormatted\] = useState\(true\)/,
      '格式化档位要在宿主',
    );
  });
});

describe('PrettyView: 树与编辑器共处一个文件', () => {
  it('pretty 挂共享树组件，raw 挂 CodeMirror', () => {
    const body = code(PRETTY);
    assert.match(body, /from '@\/components\/ui\/json-tree-view'/, '树组件必须来自共享的 ui 目录');
    assert.match(body, /<JsonTreeView\b/);
    assert.match(body, /<CodeMirror\b/);
    assert.match(body, /view === 'raw'/, '编辑器那一支应由 view 选择');
  });

  it('工具条只有一份，格式化开关也挂在它上面', () => {
    const body = code(PRETTY);
    assert.match(body, /<FloatToolbar\b/, 'PrettyView 没接浮起工具条');
    assert.equal(body.includes('raw-format-toggle'), true, 'raw 页的格式化开关丢了');
    assert.equal(body.includes('copy-response-body-btn'), true, '复制整个响应的按钮丢了');
    assert.equal(body.includes('wrap-response-btn'), true, '自动换行按钮丢了');
  });

  it('换行与格式化档位是受控 prop，不在实例里存', () => {
    const body = code(PRETTY);
    // 这两个开关描述「这份响应体怎么读」。状态留在实例里，切一次页签就换了一份。
    assert.match(body, /wrap: boolean;/, 'wrap 应是 prop');
    assert.match(body, /onToggleWrap: \(\) => void;/, 'wrap 应通过回调上报');
    assert.match(body, /formatted: boolean;/, 'formatted 应是 prop');
    assert.match(body, /onToggleFormatted: \(\) => void;/, 'formatted 应通过回调上报');
    assert.equal(
      /const \[(?:wrap|formatted),/.test(body),
      false,
      'PrettyView 不得再自己持有 wrap/formatted 状态',
    );
    // 回调要真的接到工具条上，否则按钮点了没有反应。
    assert.match(body, /onClick: onToggleWrap,/);
    assert.match(body, /onClick: onToggleFormatted,/);
  });

  it('非法 JSON 仍给同一条警告文案与同一个类名', () => {
    const body = code(PRETTY);
    assert.equal(body.includes('alert-warning-inline'), true, '警告样式类名变了');
    assert.equal(body.includes('响应不是合法 JSON'), true, '警告文案变了');
    // 警告只属于「本该是 JSON 却解析不出来」这一种情况，纯文本响应不该看到它。
    assert.match(body, /\{expectJson && <div className="alert-warning-inline"/);
  });

  it('工具条对纯文本响应只给复制与换行', () => {
    const body = code(PRETTY);
    assert.match(body, /expectJson = true/, 'expectJson 默认值必须是 true');
    // 格式化开关挂在 asJson 上：纯文本即便恰好能 parse 也不该出现这个按钮。
    assert.match(body, /if \(view === 'raw' && asJson\)/);
    assert.match(body, /const asJson = expectJson && formatResult\.ok;/);
  });
});
