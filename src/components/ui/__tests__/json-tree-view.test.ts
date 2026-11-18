/**
 * JSON 树视图的源码级回归网。
 *
 * 为什么只能读源码：本 runner（`tsx --test` + `node:assert`）没有 DOM，也没有 React 渲染器，
 * 测不了「菜单里到底有几条」「按菜单键真的弹出菜单」。可变的部分（路径、预览、子树 JSON、
 * 原始 token）都在 `src/lib/json-tree.ts` 里做成了纯函数，由
 * `src/lib/__tests__/json-tree.test.ts` 直接断言；这里只钉住「谁挂在谁身上」这种一改就
 * 静默失效的接线。
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

const TREE = readFileSync(fileURLToPath(new URL('../json-tree-view.tsx', import.meta.url)), 'utf8');

/**
 * 剥掉注释（含跨行的 JSDoc / JSX 块注释与整行 `//`），免得文件自己的散文满足某个模式。
 * 只按 `/* … *\/` 剥：把 `{` 一起吃掉的那条规则会从 `Props {` 一路匹配到后面某个 `*\/}`，
 * 顺手把整段代码删掉。
 */
function code(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
}

describe('JsonTreeView: 展开箭头是行内容之外的独立按钮', () => {
  it('箭头是一个 <button>，行内容不是按钮，层级不手写', () => {
    const body = code(TREE);
    const toggle = body.indexOf('data-testid="json-tree-toggle"');
    const row = body.indexOf('data-testid="json-tree-row"');
    assert.notEqual(toggle, -1, '展开箭头按钮消失了');
    assert.notEqual(row, -1, '行的内容容器消失了');
    // 箭头在前、行在后，说明两者是并列的兄弟节点；嵌套的话行容器会包住箭头。
    const toggleTag = body.lastIndexOf('<button', toggle);
    assert.notEqual(toggleTag, -1, '箭头必须落在一个 <button 元素上');
    assert.ok(body.indexOf('>', toggleTag) < row, '箭头按钮必须排在行内容之前');
    // 行没有点击行为，做成按钮会给每一行塞进一个空的 Tab 停点，并且按钮里的文本选不中。
    const rowTag = body.lastIndexOf('<', row);
    assert.notEqual(body.slice(rowTag, rowTag + 7), '<button', '行内容不得是按钮');
    const rowOpen = body.slice(row, body.indexOf('>', row));
    assert.match(rowOpen, /select-text/, '行内容要允许拖选复制');
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

describe('JsonTreeView: 行的键盘入口', () => {
  it('行容器可聚焦，菜单键与 Shift+F10 都能打开菜单', () => {
    const body = code(TREE);
    // 标量行与空容器没有箭头按钮，键盘用户只能靠聚焦整行拿到「复制 JSON 路径」。
    assert.match(body, /tabIndex=\{0\}/, '行容器要能拿到焦点');
    assert.match(body, /aria-haspopup="menu"/, '行要声明它带菜单');
    assert.match(body, /event\.key !== 'ContextMenu'/, '菜单键要能打开菜单');
    assert.match(body, /event\.shiftKey && event\.key === 'F10'/, 'Shift+F10 要能打开菜单');
    assert.match(body, /preventDefault\(\)/, '按键要被接住，不能触发浏览器的默认上下文菜单');
    assert.match(body, /focusRingBare/, '键盘焦点要看得见');
    // 坐标来自行自己的矩形，不是指针位置：键盘事件没有坐标。
    assert.match(body, /getBoundingClientRect\(\)/);
  });
});

describe('JsonTreeView: 换行开关与菜单生命周期', () => {
  it('换行同时作用于键名与标量文本', () => {
    const body = code(TREE);
    assert.match(
      body,
      /const textWrap = wrap \? 'whitespace-pre-wrap break-all' : 'whitespace-pre';/,
      '换行档位的两个类名必须成对出现',
    );
    // 一处定义 + 键名 + 标量：长键名在换行开启时不会横着把行顶出去。
    assert.ok((body.match(/textWrap/g) ?? []).length >= 3, '键名与标量要共用同一份换行类名');
  });

  it('换了一棵树就关掉菜单，菜单不会指向上一棵树的节点', () => {
    const body = code(TREE);
    const start = body.indexOf('useEffect(');
    const end = body.indexOf('}, [root]);');
    assert.notEqual(start, -1, '重置展开状态的 effect 消失了');
    assert.ok(end > start, '重置 effect 的依赖必须只有 root');
    assert.match(body.slice(start, end), /setMenu\(null\)/, '重置展开状态的那一支必须一并关掉菜单');
  });

  it('菜单项复制的是惰性拼出来的子树 JSON', () => {
    const body = code(TREE);
    // 节点不再自带 `json` 字段：每行都序列化一遍，深树会白跑几十次。
    assert.equal(/node\.json\b/.test(body), false, '节点不得再存急切序列化的 json 字段');
    assert.match(body, /copy\(subtreeJson\(menu\.node\)\)/, '复制子树要现拼');
  });
});
