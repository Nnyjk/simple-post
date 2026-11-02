import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

/**
 * 模态层的不变量哨兵。
 *
 * 这几条都是**结构**上的约定，靠代码评审看不出来、坏了也不一定立刻出事，所以在这里
 * 钉住：
 *
 * 1. 全屏浮层只有一个实现（`ModalSurface`）。第二个铺满视口的浮层就是第二套模态
 *    语义 —— 焦点陷阱、背景 inert、Esc 关闭都会各写一遍、各漏一处。
 * 2. 浮层层级只从 `layers.ts` 取。组件里手写 `z-[200]` 或内联 `zIndex` 会让层级
 *    顺序变得不可知，而「右键菜单浮在对话框上面」正是这么来的。
 * 3. 确认框只有一处实现。同名不同 API 的两份确认框会让「初始焦点落在哪」这种决定
 *    分叉。
 * 4. 没有原生弹窗。`window.alert` 的样式不受控，在 Tauri 的 webview 里还会盖住整个
 *    应用窗口。
 * 5. 破坏性确认的初始焦点不落在确认键上。回车即删不是想要的默认。
 * 6. 确认框的调用点只用统一后的那套 prop。旧的两套 API 各有各的 prop 名，留着旧名
 *    就说明调用点没跟着改。
 *
 * 这些检查只读源码，不渲染 DOM —— DOM 层的行为（背景 inert、Tab 循环、焦点归还）由
 * 界面验证脚本覆盖。
 */

const SRC = resolve(process.cwd(), 'src');

/** `src/` 下每个 .ts/.tsx 文件（跳过测试目录）。 */
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

const files = collectSourceFiles(SRC);

/** 文件相对仓库根的路径，写进报错信息用。 */
function rel(file: string): string {
  return relative(process.cwd(), file).replace(/\\/g, '/');
}

function read(file: string): string {
  return readFileSync(file, 'utf8');
}

/**
 * 去掉注释再匹配。
 *
 * 不去掉的话，注释里的一句「这里不需要额外的 window.confirm() 提示」就会被当成一次调用
 * —— 那条注释正是在解释为什么**不**用它。行注释的 `//` 前置断言避开字符串里的 `://`
 * （URL 会被截半）。
 */
function stripComments(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/(^|[^:'"`])\/\/.*$/gm, '$1');
}

/**
 * 源码里所有可能装类名的片段：字符串字面量，加上 `cn(...)` / `clsx(...)` 的调用体。
 *
 * 为什么要把调用体也算进去：`cn('fixed', 'inset-0')` 拆成两个字符串字面量，逐个看
 * 都只有半个类名，谁也看不出它铺满视口。合并成片段再一起看，才认得出「fixed + 铺满
 * 视口」这个组合。
 */
function classChunks(src: string): string[] {
  const chunks: string[] = [];
  const literal = /(['"`])((?:\\.|(?!\1)[\s\S])*)\1/g;
  let m: RegExpExecArray | null;
  while ((m = literal.exec(src)) !== null) chunks.push(m[2]);
  const call = /\b(?:cn|clsx)\s*\(([\s\S]*?)\)/g;
  while ((m = call.exec(src)) !== null) chunks.push(m[1]);
  return chunks;
}

/** 把片段切成类名 token。 */
function tokens(chunk: string): string[] {
  return chunk.split(/[\s"'`]+/).filter(Boolean);
}

/**
 * 这些 token 合起来是不是铺满视口。
 *
 * `inset-0` 是 shorthand；`inset-x-0` + `inset-y-0` 与四条边分别给 0 是另外两种
 * 写法。三种都算「铺满视口」。
 */
function coversViewport(tk: string[]): boolean {
  if (tk.includes('inset-0')) return true;
  if (tk.includes('inset-x-0') && tk.includes('inset-y-0')) return true;
  return ['top-0', 'right-0', 'bottom-0', 'left-0'].every((c) => tk.includes(c));
}

/**
 * 抽出某个 JSX 标签的每个元素块。
 *
 * 元素可能自闭合（`/>`），也可能带子节点（`</Tag>`）—— 取先到的那个边界，否则会
 * 一路读到下一个同名元素里去。
 */
function elementBlocks(src: string, tag: string): string[] {
  const blocks: string[] = [];
  const re = new RegExp(`<${tag}\\b`, 'g');
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) {
    const start = m.index;
    const closeTag = src.indexOf(`</${tag}>`, start);
    const selfClose = src.indexOf('/>', start);
    const end = closeTag >= 0 && (selfClose < 0 || closeTag < selfClose) ? closeTag : selfClose;
    blocks.push(src.slice(start, end >= 0 ? end : start + 400));
    if (end >= 0) re.lastIndex = end;
  }
  return blocks;
}

/**
 * JSX 开始标签的结束位置（那个 `>` 的下标）。
 *
 * 属性里可能有 `=>` 这样的箭头函数，所以 `>` 不能直接找：得先数一遍花括号，落在
 * 括号外的 `>` 才是标签的结束。
 */
function openingTagEnd(src: string, start: number): number {
  let depth = 0;
  for (let i = start; i < src.length; i++) {
    const ch = src[i];
    if (ch === '{') depth++;
    else if (ch === '}') depth--;
    else if (ch === '>' && depth === 0) return i;
  }
  return -1;
}

test('全屏浮层只有 ModalSurface 一个实现', () => {
  const hits = files.filter((f) => {
    const src = stripComments(read(f));
    return classChunks(src).some((chunk) => {
      const tk = tokens(chunk);
      return tk.includes('fixed') && coversViewport(tk);
    });
  });
  assert.deepEqual(
    hits.map(rel),
    ['src/components/ui/modal-surface.tsx'],
    '这些文件自己铺了一层铺满视口的浮层：\n  ' +
      hits.map(rel).join('\n  ') +
      '\n模态层（portal / Esc / 遮罩点击 / 焦点陷阱 / 背景 inert）只在 ' +
      'src/components/ui/modal-surface.tsx 里实现，别处再铺一层就是第二套模态语义。',
  );
});

test('浮层层级只从 layers.ts 取', () => {
  // 只看「层」的数值：50 及以上才是跨组件的层。z-10 / z-20 那种是组件自己内部的
  // 堆叠顺序，写在组件里没问题。
  const LAYER_VALUE = /['"`\s]z-(\[)?(\d+)/g;
  const INLINE_Z = /zIndex\s*:\s*['"`]?(\d+)/g;
  const offenders: string[] = [];
  for (const file of files) {
    if (rel(file) === 'src/components/ui/layers.ts') continue;
    const src = read(file);
    for (const m of src.matchAll(LAYER_VALUE)) {
      const value = Number(m[2]);
      if (value >= 50) {
        offenders.push(`${rel(file)}: ${m[0].trim()}`);
      }
    }
    // 内联的 zIndex 与 Tailwind 的 z-N 是同一件事的两种写法，一起管。
    for (const m of src.matchAll(INLINE_Z)) {
      const value = Number(m[1]);
      if (value >= 50) {
        offenders.push(`${rel(file)}: ${m[0].trim()}`);
      }
    }
  }
  assert.deepEqual(
    offenders,
    [],
    '这些地方自己写了浮层层级：\n  ' +
      offenders.join('\n  ') +
      '\n层级写在 src/components/ui/layers.ts 里（popover 50 / contextMenu 60 / ' +
      'tooltip 70 / modal 100 / 模态内 tooltip 110），组件里手写 z 值，顺序就不可知了。',
  );
});

test('确认框只有一处实现', () => {
  // 写法要盖住全部形态：`export function`、`export const`、`function`、`class`。
  // 只认一种写法的话，换个写法把确认框复制一份，这条用例照样绿。
  const DEF =
    /(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function|const|let|var|class)\s+ConfirmDialog\b/;
  const defs = files.filter((f) => DEF.test(read(f)));
  assert.deepEqual(
    defs.map(rel),
    ['src/components/ui/confirm-dialog.tsx'],
    '确认框的实现应当只有 src/components/ui/confirm-dialog.tsx 一处：\n  ' +
      defs.map(rel).join('\n  '),
  );
});

test('没有原生弹窗', () => {
  // 两种写法都算：`window.alert(...)` 与裸的 `alert(...)`。
  // 只查带 `window.` 的那种，裸调用会漏过去。
  const BARE = /(?<![\w.])(?:alert|confirm|prompt)\s*\(/;
  const QUALIFIED = /\b(?:window|globalThis|self)\s*\.\s*(?:alert|confirm|prompt)\s*\(/;
  const hits = files.filter((f) => {
    const src = stripComments(read(f));
    return BARE.test(src) || QUALIFIED.test(src);
  });
  assert.deepEqual(
    hits.map(rel),
    [],
    '这些地方还在用浏览器原生弹窗：\n  ' +
      hits.map(rel).join('\n  ') +
      '\n原生弹窗的样式不受控，在 Tauri 的 webview 里还会盖住整个应用窗口；' +
      '改用 components/ui/dialog 的 ConfirmDialog 或 Dialog。',
  );
});

test('确认框的调用点只用统一后的那套 prop', () => {
  // 合并前项目树那版用的 prop 名是 `message` / `confirmLabel` / `cancelLabel`。
  // 留着旧名就说明调用点没跟着改到统一后的 API 上。
  const OLD_PROPS = ['message=', 'confirmLabel=', 'cancelLabel='];
  const offenders: string[] = [];
  for (const file of files) {
    const src = read(file);
    for (const block of elementBlocks(src, 'ConfirmDialog')) {
      for (const prop of OLD_PROPS) {
        if (block.includes(prop)) {
          offenders.push(`${rel(file)}: ${prop}`);
        }
      }
    }
  }
  assert.deepEqual(
    offenders,
    [],
    '这些地方还在用合并前的旧 prop：\n  ' +
      offenders.join('\n  ') +
      '\n确认框统一后的 prop 是 description / confirmText / cancelText。',
  );
});

test('破坏性确认的初始焦点不落在确认键上', () => {
  const file = join(SRC, 'components', 'ui', 'confirm-dialog.tsx');
  const src = read(file);
  const focused = elementBlocks(src, 'Button').filter((b) => b.includes('autoFocus'));
  assert.ok(focused.length > 0, '确认框应当把初始焦点放在某个按钮上，否则焦点会掉回背景');
  // 每处 `autoFocus` 都要查，不能只看第一处：React 按树的顺序逐个聚焦，
  // 最后一个 `autoFocus` 才是真正拿到焦点的那个。只查第一处的话，
  // 「取消」上放一个、破坏性键上再放一个，就会被漏过去。
  for (const block of focused) {
    assert.ok(
      !block.includes('onConfirm'),
      'autoFocus 落到了调用 onConfirm 的那个按钮上：回车即执行破坏性动作。' +
        '初始焦点应当落在「取消」上。',
    );
  }
});

test('命令面板走 ModalSurface，且键盘处理挂在面板自己身上', () => {
  const file = join(SRC, 'features', 'command-palette', 'CommandPalette.tsx');
  const src = read(file);
  assert.ok(
    src.includes("from '@/components/ui/modal-surface'"),
    '命令面板应当复用 ModalSurface，不要自己再铺一层模态语义。',
  );
  const start = src.indexOf('<ModalSurface');
  assert.ok(start >= 0, '命令面板应当渲染 ModalSurface');
  const end = openingTagEnd(src, start);
  assert.ok(end > start, '没能定位到 ModalSurface 的开始标签');
  const tag = src.slice(start, end);
  assert.ok(
    tag.includes('onKeyDown'),
    '命令面板的键盘处理应当挂在 ModalSurface 上：挂在面板内部的子节点上时，' +
      '面板自己收到的按键事件走不到那个处理器。',
  );
  const outside = src.slice(0, start) + src.slice(end);
  assert.ok(
    !/onKeyDown\s*=\s*\{/.test(outside),
    '命令面板里除了 ModalSurface 还有别处挂 onKeyDown：面板内部的元素上再挂一份，' +
      '同一个按键会被处理两次。',
  );
});
