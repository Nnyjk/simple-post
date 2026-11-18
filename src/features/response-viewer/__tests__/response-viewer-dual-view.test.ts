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
const TREE = read('../../../components/ui/json-tree-view.tsx');

/** 剥掉整行 `//` 注释与单行 JSX 块注释，免得文件自己的散文满足某个模式。 */
function code(source: string): string {
  return source.replace(/\{\s*\/\*[^\n]*?\*\/\s*\}/g, '').replace(/^\s*\/\/.*$/gm, '');
}

describe('ResponseViewer: pretty/raw 两态都交给 PrettyView', () => {
  it('pretty 分支与 raw 的 JSON 分支都渲染 PrettyView', () => {
    const body = code(VIEWER);
    assert.match(body, /<PrettyView\b[^>]*view="pretty"/, 'pretty 分支没渲染 PrettyView');
    assert.match(body, /<PrettyView\b[^>]*view="raw"/, 'raw 分支没渲染 PrettyView');
  });

  it('文本响应也交给 PrettyView，并声明它不是 JSON', () => {
    const body = code(VIEWER);
    const occurrences = body.match(/<PrettyView\b/g) ?? [];
    assert.equal(occurrences.length, 4, 'pretty/raw 两页的 json 与 text 分支都应渲染 PrettyView');
    const explicit = body.match(/expectJson=\{false\}/g) ?? [];
    assert.equal(explicit.length, 2, 'text 的两处必须显式传 expectJson={false}');
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

describe('JsonTreeView: 展开箭头是行内容之外的独立按钮', () => {
  it('箭头是一个 <button>，行内容不是按钮，层级不手写', () => {
    const body = code(TREE);
    const toggle = body.indexOf('data-testid="json-tree-toggle"');
    const row = body.indexOf('data-testid="json-tree-row"');
    assert.notEqual(toggle, -1, '展开箭头按钮消失了');
    assert.notEqual(row, -1, '行的内容容器消失了');
    // 箭头在前、行在后，说明两者是并列的兄弟节点；嵌套的话行容器会包住箭头。
    assert.ok(toggle < row, '箭头按钮必须排在行内容之前');
    // 行没有点击行为，做成按钮会给每一行塞进一个空的 Tab 停点，并且按钮里的文本选不中。
    assert.equal(
      /<button\b[^>]*data-testid="json-tree-row"/.test(body),
      false,
      '行内容不得是按钮：它没有点击行为，且按钮内的文本无法选中',
    );
    assert.match(body, /data-testid="json-tree-row"[^>]*select-text/, '行内容要允许拖选复制');
    assert.equal(/z-\[/.test(body), false, '层级只能从 LAYER 取，不得手写 z-index');
    assert.equal(
      /<Button\b/.test(body),
      false,
      '工具条按钮由 FloatToolbar 负责，树里不该再有按钮组件',
    );
  });

  it('解析失败时什么都不渲染，交给调用方退回原文', () => {
    const body = code(TREE);
    assert.match(body, /if \(!root\) return null;/, '解析失败的分支必须返回 null');
  });
});
