/**
 * 响应体只读视图的光标配置 —— `src/features/response-viewer/PrettyView.tsx`。
 *
 * 测不了光标本身：本 runner（`tsx --test` + `node:assert`）没有 DOM、没有 jsdom，
 * 渲染不出编辑器。钉住的是**决定有没有光标的那处配置**。
 *
 * CodeMirror 6 把两件事分开：`EditorView.editable` 决定 content DOM 要不要
 * `contenteditable`（也就决定了编辑器能不能拿到焦点），`EditorState.readOnly` 才拦编辑。
 * `editable={false}` 让编辑器永远不可聚焦，而 CodeMirror 自绘的光标（`.cm-cursor`）只在
 * 聚焦时出现，原生光标又被它的 `caret-color: transparent` 关掉 ⇒ 响应体里一个光标都没有。
 * 这几处配置一旦回退，运行期不会有任何失败来提醒。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const SRC = readFileSync(fileURLToPath(new URL('../PrettyView.tsx', import.meta.url)), 'utf8');

/**
 * 剥掉注释再断言：文件里那段解释「为什么用 readOnly」的注释本身就写着
 * `editable={false}`，不剥掉的话它会把自己钉住的规则打红。
 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
}

/** `<CodeMirror … />` 的属性文本。 */
function codeMirrorProps(): string {
  const body = code(SRC);
  const start = body.indexOf('<CodeMirror');
  assert.notEqual(start, -1, 'the response body editor is gone');
  const end = body.indexOf('/>', start);
  assert.notEqual(end, -1, 'the response body editor is no longer self-closing');
  return body.slice(start, end);
}

describe('响应体的只读视图要可聚焦，否则没有光标', () => {
  it('不用 editable={false}：编辑器拿不到焦点，CodeMirror 的光标就不出现', () => {
    assert.ok(
      !/editable\s*=\s*\{false\}/.test(codeMirrorProps()),
      'the response body editor must stay focusable: editable={false} removes ' +
        'contenteditable, so CodeMirror never draws its caret',
    );
  });

  it('用 readOnly 拦编辑：输入、粘贴、拖放、剪切都要被拒', () => {
    assert.ok(
      /(^|\s)readOnly(\s|$)/.test(codeMirrorProps()),
      'the response body editor must be read-only through the readOnly prop',
    );
  });

  it('关掉 history：value 同步的整篇替换会进撤销栈，Ctrl+U 能把响应体退回上一份', () => {
    assert.ok(
      /history:\s*false/.test(codeMirrorProps()),
      'the response body editor must not keep a history stack: the wrapper replaces the ' +
        'whole document when the value changes, and readOnly does not block undoSelection',
    );
  });
});
