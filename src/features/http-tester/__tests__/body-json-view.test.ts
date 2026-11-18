/**
 * 请求体 json 载荷区的接线回归网 —— 读源码，不渲染 React。
 *
 * 本 runner（`tsx --test` + `node:assert`）没有 DOM 与 React 渲染器，故测不了树的实际
 * 交互；这里守的是**接线**，与 `src/features/response-viewer/__tests__/response-viewer-layout.test.ts`
 * 同一套取舍。
 *
 * 钉住三件事：
 *
 *   1. 树那一档只读 —— 载荷只由 Raw 的编辑器写回，树一旦接上 onChange 就会变成两块
 *      互相覆盖的编辑器。
 *   2. 写回只发生在 raw 分支，且用户在 Raw 里动手之后视图不再自动切换，否则正在打字的
 *      编辑器会从眼皮底下消失。
 *   3. 自动换行走的是 CodeMirror 扩展，不是样式表 —— 它按扩展重算行高。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const SOURCE = readFileSync(
  resolve(process.cwd(), 'src/features/http-tester/BodyEditor.tsx'),
  'utf8',
);

/** 剥掉注释，只留代码，免得文件自己的散文满足下面某个模式。 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
}

const CODE = code(SOURCE);

/** 一个自闭合 JSX 元素的原文。 */
function element(name: string): string {
  const start = CODE.indexOf(`<${name}`);
  assert.notEqual(start, -1, `找不到 <${name}>`);
  const end = CODE.indexOf('/>', start);
  assert.notEqual(end, -1, `<${name}> 必须是自闭合元素，否则这个切片要重写`);
  return CODE.slice(start, end);
}

describe('请求体 json 载荷区：树 / Raw 两档', () => {
  it('树只读：JsonTreeView 只吃文本与换行开关', () => {
    const tree = element('JsonTreeView');
    assert.ok(tree.includes('text={value.content}'), '树要展示当前载荷');
    assert.ok(tree.includes('wrap={wrap}'), '树要跟随同一个换行开关');
    assert.ok(!tree.includes('onChange'), '树一旦能写回，载荷就有两个写入口');
  });

  it('默认视图由载荷能否解析成 JSON 决定', () => {
    assert.ok(
      /isJsonPayload\(value\.content\)\s*\?\s*'tree'\s*:\s*'raw'/.test(CODE),
      '能解析就进树，解析不出来或空就落在 Raw',
    );
  });

  it('用户手选过视图之后不再自动切换', () => {
    assert.ok(
      /if \(userPickedBodyView\.current\) return;/.test(CODE),
      '没有这道守卫，Raw 里刚改到合法就会被拽去树视图',
    );
    assert.ok(
      /onUserPick\(\);\s*\n\s*onChange\(body\);/.test(CODE),
      '在 Raw 里编辑也算用户选过视图',
    );
  });

  it('格式化只出现在 Raw 档，且由工具条承接', () => {
    const rawAt = CODE.indexOf("if (view === 'raw') {");
    assert.notEqual(rawAt, -1, '格式化按钮的档位条件不见了');
    assert.ok(
      CODE.slice(rawAt, rawAt + 400).includes("testId: 'format-body-btn'"),
      '工具条里那个格式化按钮要沿用它原来的 testid',
    );
    assert.ok(
      !CODE.includes('data-testid="format-body-btn"'),
      '左栏顶部那个独立格式化按钮必须已经删掉，能力只在工具条里有一份',
    );
  });

  it('自动换行走 CodeMirror 扩展而不是 CSS', () => {
    assert.ok(CODE.includes('EditorView.lineWrapping'), '换行要靠扩展，编辑器据它重算行高');
  });

  it('树渲染不出来时给可见说明，不留空白', () => {
    const emptyAt = CODE.indexOf('JSON 格式有误，切到 Raw 修改');
    assert.notEqual(emptyAt, -1, '载荷非法时树返回 null，必须有一句替代说明');
    assert.ok(CODE.lastIndexOf('<EmptyHint', emptyAt) < emptyAt, '那句说明要渲染在 EmptyHint 里');
  });
});
