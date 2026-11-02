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
 * 1. 全屏浮层只有一个实现（`ModalSurface`）。第二个 `fixed inset-0` 就是第二套模态
 *    语义 —— 焦点陷阱、背景 inert、Esc 关闭都会各写一遍、各漏一处。
 * 2. 浮层层级只从 `layers.ts` 取。组件里手写 `z-[200]` 会让层级顺序变得不可知，
 *    而「右键菜单浮在对话框上面」正是这么来的。
 * 3. 确认框只有一处实现。同名不同 API 的两份确认框会让「初始焦点落在哪」这种决定
 *    分叉。
 * 4. 没有原生弹窗。`window.alert` 的样式不受控，在 Tauri 的 webview 里还会盖住整个
 *    应用窗口。
 * 5. 破坏性确认的初始焦点不落在确认键上。回车即删不是想要的默认。
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

/** 源码里出现某个片段的文件。 */
function filesContaining(pattern: RegExp): string[] {
  return files.filter((f) => pattern.test(stripComments(read(f))));
}

test('全屏浮层只有 ModalSurface 一个实现', () => {
  const hits = filesContaining(/fixed inset-0/);
  assert.deepEqual(
    hits.map(rel),
    ['src/components/ui/modal-surface.tsx'],
    '这些文件自己铺了一层全屏浮层：\n  ' +
      hits.map(rel).join('\n  ') +
      '\n模态层（portal / Esc / 遮罩点击 / 滚动锁 / 焦点陷阱 / 背景 inert）只在 ' +
      'src/components/ui/modal-surface.tsx 里实现，别处再铺一层就是第二套模态语义。',
  );
});

test('浮层层级只从 layers.ts 取', () => {
  // 只看「层」的数值：50 及以上才是跨组件的层。z-10 / z-20 那种是组件自己内部的
  // 堆叠顺序，写在组件里没问题。
  const LAYER_VALUE = /['"`\s]z-(\[)?(\d+)/g;
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
  }
  assert.deepEqual(
    offenders,
    [],
    '这些地方自己写了浮层层级：\n  ' +
      offenders.join('\n  ') +
      '\n层级写在 src/components/ui/layers.ts 里（popover 50 / contextMenu 60 / ' +
      'tooltip 70 / modal 100），组件里手写 z 值，顺序就不可知了。',
  );
});

test('确认框只有一处实现', () => {
  const defs = files.filter((f) => /export function ConfirmDialog/.test(read(f)));
  assert.deepEqual(
    defs.map(rel),
    ['src/components/ui/confirm-dialog.tsx'],
    '确认框的实现应当只有 src/components/ui/confirm-dialog.tsx 一处：\n  ' +
      defs.map(rel).join('\n  '),
  );
});

test('没有原生弹窗', () => {
  const hits = filesContaining(/\bwindow\.(alert|confirm|prompt)\(/);
  assert.deepEqual(
    hits.map(rel),
    [],
    '这些地方还在用浏览器原生弹窗：\n  ' +
      hits.map(rel).join('\n  ') +
      '\n原生弹窗的样式不受控，在 Tauri 的 webview 里还会盖住整个应用窗口；' +
      '改用 components/ui/dialog 的 ConfirmDialog 或 Dialog。',
  );
});

test('破坏性确认的初始焦点不落在确认键上', () => {
  const file = join(SRC, 'components', 'ui', 'confirm-dialog.tsx');
  const src = read(file);
  const at = src.indexOf('autoFocus');
  assert.ok(at >= 0, '确认框应当把初始焦点放在某个按钮上，否则焦点会掉回背景');
  // autoFocus 所在的那个 <Button> 到它自己闭合为止的那一段。按钮可能自闭合（`/>`），
  // 也可能带子节点（`</Button>`）——取先到的那个边界，否则会一路读到下一个按钮里去。
  const closeTag = src.indexOf('</Button>', at);
  const selfClose = src.indexOf('/>', at);
  const end = closeTag >= 0 && (selfClose < 0 || closeTag < selfClose) ? closeTag : selfClose;
  const block = src.slice(at, end >= 0 ? end : at + 200);
  assert.ok(
    !block.includes('onConfirm'),
    'autoFocus 落到了调用 onConfirm 的那个按钮上：回车即执行破坏性动作。' +
      '初始焦点应当落在「取消」上。',
  );
});
