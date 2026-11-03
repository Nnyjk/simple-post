/**
 * 「重命名时能按住左键框选文字」的接线回归网 —— 读源码，不渲染 React。
 *
 * **为什么只能读源码。** 本 runner（`tsx --test` + `node:assert`）没有 DOM、没有
 * jsdom、没有 React 渲染器，故测不了真实的鼠标框选。读源码是钝办法，但这是本仓库对
 * 「功能测试看不见的那类缺陷」的既定取舍，见
 * `src/features/response-viewer/__tests__/response-viewer-layout.test.ts`，理由相同。
 *
 * **它钉住的是什么。** 树里每一行的按钮都是 `draggable` 的 —— 那是这棵树拖放排序的
 * 起点。而**重命名的输入框就渲染在那个按钮里面**：浏览器把 `draggable` 祖先的拖拽手势
 * 看得比输入框的框选手势更重，于是「按住左键拖一下想选中几个字」变成了「拖这一行」。
 *
 * 故正在改名的那一行必须退出拖拽：`draggable={!isEditing}`。
 *
 * 行为层面靠真机验证；这里守的是**接线**：那三处 `draggable` 一旦又变回裸属性，右键框选
 * 就会重新被拖拽吞掉。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

function readSource(relative: string): string {
  return readFileSync(fileURLToPath(new URL(relative, import.meta.url)), 'utf8');
}

/**
 * 剥掉注释，只留代码。
 *
 * 必须剥：注释里会提到「早先这里是裸的 `draggable`」这样的**反例**，不剥的话源码级断言
 * 会被自己的说明文字满足 —— 一条不可能失败的回归测试比没有更糟。
 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
}

/**
 * 三种树行。它们的按钮结构一样：都是 `draggable` 的 `button`，里面装着 `InlineEdit`。
 * 模块、集合、接口的重命名是同一个缺陷，故一并守住。
 */
const ROWS = ['EndpointRow.tsx', 'ModuleRow.tsx', 'CollectionRow.tsx'] as const;

describe('重命名一行时：那一行不参与拖拽', () => {
  it('三种树行的按钮都把 draggable 绑到 !isEditing', () => {
    for (const row of ROWS) {
      const src = code(readSource(`../rows/${row}`));
      assert.match(
        src,
        /draggable=\{!isEditing\}/,
        `${row}：正在改名的那一行仍参与拖拽 —— draggable 的祖先会抢走输入框里的框选手势，用户想选中几个字却拖走了整行`,
      );
    }
  });

  it('没有哪一行还留着裸的 draggable 属性', () => {
    for (const row of ROWS) {
      const src = code(readSource(`../rows/${row}`));
      assert.equal(
        /\bdraggable\b(?!\s*=\s*\{)/.test(src),
        false,
        `${row}：还有裸的 draggable 属性 —— 那一行在重命名期间仍会被拖拽抢走框选手势`,
      );
    }
  });

  it('重命名输入框确实渲染在那个按钮里面（这正是必须关掉拖拽的原因）', () => {
    for (const row of ROWS) {
      const src = code(readSource(`../rows/${row}`));
      assert.match(
        src,
        /<button[\s\S]*?draggable=\{!isEditing\}[\s\S]*?<InlineEdit/,
        `${row}：InlineEdit 不在这个按钮里面了 —— 若输入框已不在 draggable 祖先内，这条约束就不成立，该重新确认修法`,
      );
    }
  });
});

describe('不在重命名时：拖拽照旧', () => {
  it('三种树行仍然接着拖放排序的回调', () => {
    for (const row of ROWS) {
      const src = code(readSource(`../rows/${row}`));
      for (const handler of ['onDragStart', 'onDragOver', 'onDrop', 'onDragEnd']) {
        assert.match(
          src,
          new RegExp(`\\b${handler}\\b`),
          `${row}：${handler} 没了 —— 树行的拖放排序被误伤了`,
        );
      }
    }
  });
});
