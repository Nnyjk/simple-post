/**
 * 样式持续保持的哨兵 —— 几条「坏了不一定立刻出事」的约定。
 *
 * 这些规则共同的毛病是**只在某一档主题、某一处尺寸下才犯**，代码评审看不出来，界面也不
 * 一定当场报错：一个 `text-white` 在白底按钮上只是丑，在一个中高明度色相的实色底上就是
 * 不可读。故在这里把「不要写」变成会失败的用例。
 *
 * 已有先例：`src/lib/__tests__/modal-layer.test.ts` 管浮层与层级（含「层的数值只从
 * `layers.ts` 取，且 ≥50」），`src/features/workspace/__tests__/tab-strip-animation.test.ts`
 * 管动画通道。本文件只管**颜色与文字层的 token**，不重复那两边已钉住的东西；唯一的交集
 * 是任意值 `z-[…]`（模态层那条只抓 ≥50，抓不到 `z-[5]` 这种）。
 *
 * 1. 禁任意层级：`z-[…]` 与内联 `zIndex:` 一律走 `components/ui/layers.ts`。
 * 2. 禁 Tailwind 原生调色板：`bg-gray-200` 之类在深浅两档里纹丝不动，浅色档好看、深色
 *    档糊在背景上。语义 token（`bg-muted`、`border-border`…）才是两档都成立的那一份。
 * 3. 禁写死黑/白：`text-white` / `bg-white` 在深色档是刺眼的，`bg-black` 只允许出现在
 *    `modal-surface.tsx` 的遮罩上（那是唯一一处「不管主题都是黑」的地方）。
 * 4. 禁手写 hover 透明度：`bg-accent/40` 该是 `bg-overlay-hover` / `bg-surface-hover`。
 *    注意实心的 `bg-accent`（Button ghost/outline 的 hover 与选中底）是设计系统里既定的
 *    那一份，不禁 —— 禁的是 `/N` 这种每处自己调的 alpha。
 * 5. 次要文字不要手写 alpha：带字号的 `text-muted-foreground/70` 说明它是**文字**，改用
 *    `text-faint`（`--text-faint` 就是为「该读到但不该是主角」的文字准备的）。`/40` `/50`
 *    留给装饰图标 —— 那类元素不带字号，故本规则放行。
 * 6. 禁任意字号：`text-[…px]` 这类值绕过了字号档位，改档时要全仓找字符串。
 * 7. 圆角只到 `lg`：`xl` 以上是卡片与窗口的量级，非窗口组件用了就与窗口边缘打架。
 * 8. 虚线边框只有一套语汇：`ui/empty-hint.tsx`。落区与虚线按钮是另外两个意思，列出白名单。
 * 9. `opacity-0` + `group-hover:opacity-100` 的「hover 才现形」，必须同时给键盘一条路径
 *    （`focus-visible:` / `group-focus-within:` / `group-focus-visible:`），否则那个按钮
 *    对键盘用户是不可见的目标。
 * 10. 悬停与焦点的底色只有 `bg-overlay-hover` / `bg-surface-hover`；`bg-accent` 只用于
 *    「已选中」。判据：`hover:`/`focus:`/`active:` 前缀的 `bg-accent` 一律出局 —— 这类
 *    写法在浅色档比 overlay 重一倍多，行列表会看成棋盘；而未加前缀的 `bg-accent` 是
 *    selection（与 `--surface-select` 同值），属于设计系统里既有的一份。
 */
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const SRC = resolve(process.cwd(), 'src');

/** `src/` 下每个 .ts/.tsx 文件（跳过 `__tests__`：哨兵不查哨兵）。 */
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

function rel(file: string): string {
  return relative(process.cwd(), file).replace(/\\/g, '/');
}

/**
 * 去掉注释再匹配，**保留行号**。
 *
 * 注释里全是「不要写 `bg-white`」这类反例：不去掉的话，说明为什么禁止的那句话本身就是
 * 违规。块注释按字符换空格（换行留着），行注释整行清空 —— 报错里的行号才对得上。
 */
function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/(^|[^:'"`])\/\/.*$/gm, '$1');
}

const files = collectSourceFiles(SRC);
const sources = new Map(files.map((f) => [rel(f), stripComments(readFileSync(f, 'utf8'))]));

/** 逐行跑一条规则，返回 `path:line 说明`。规则返回 `null` 表示这一行没问题。 */
function scanLines(rule: (line: string) => string | null): string[] {
  const hits: string[] = [];
  for (const [name, src] of sources) {
    src.split('\n').forEach((line, i) => {
      const why = rule(line);
      if (why) hits.push(`${name}:${i + 1} ${why}`);
    });
  }
  return hits;
}

function assertClean(hits: string[], message: string): void {
  assert.deepEqual(hits, [], `${message}\n  ` + hits.join('\n  '));
}

test('层级不用任意值，也不内联 zIndex', () => {
  const hits = scanLines((line) => {
    const arbitrary = line.match(/z-\[[^\]]*\]/);
    if (arbitrary) return `用任意值写层级 ${arbitrary[0]}`;
    const inline = line.match(/\bzIndex\s*:/);
    if (inline) return '内联 zIndex';
    return null;
  }).filter((hit) => !hit.startsWith('src/components/ui/layers.ts:'));
  assertClean(
    hits,
    '层级只能从 src/components/ui/layers.ts 取（popover 50 / contextMenu 60 / tooltip 70 / ' +
      'modal 100 / 模态内 tooltip 110）：',
  );
});

test('不用 Tailwind 原生调色板', () => {
  const NATIVE =
    /\b(?:bg|text|border|ring|fill|stroke)-(?:gray|slate|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose)-\d{2,3}\b/;
  const hits = scanLines((line) => {
    const m = line.match(NATIVE);
    return m ? `原生调色板 ${m[0]}` : null;
  });
  assertClean(
    hits,
    '原生调色板不随主题变：浅色档成立、深色档糊在背景上。改用语义 token' +
      '（bg-muted / text-foreground / border-border / text-success-foreground…）：',
  );
});

test('不写死黑与白', () => {
  const hits = scanLines((line) => {
    const hit = line.match(/\b(?:text-white|bg-white|bg-black)\b/);
    return hit ? `写死 ${hit[0]}` : null;
  }).filter((hit) => !hit.startsWith('src/components/ui/modal-surface.tsx:'));
  assertClean(
    hits,
    '写死黑白在另一档主题里必然错。文字用语义配对（实色填充上写 *-foreground），' +
      '遮罩用 bg-black 的只有 modal-surface.tsx：',
  );
});

test('hover 不手写 accent 透明度', () => {
  const hits = scanLines((line) => {
    const m = line.match(/\bbg-accent\/\d+/);
    return m ? `手写 ${m[0]}` : null;
  });
  assertClean(
    hits,
    'hover / 选中底用 bg-overlay-hover 或 bg-surface-hover（见 globals.css 里 ' +
      '`--overlay-hover` 那段注释）：',
  );
});

test('次要文字不用手写 alpha', () => {
  const ALPHA = /text-muted-foreground\/\d{2}/;
  const SIZED = /\btext-(?:xs|sm|base|lg|micro|nano)\b/;
  const hits = scanLines((line) => {
    if (!ALPHA.test(line) || !SIZED.test(line)) return null;
    return `带字号的 muted-foreground alpha：${line.trim().slice(0, 80)}`;
  });
  assertClean(
    hits,
    '带字号的 muted-foreground/N 是「次要但该读」的文字，改用 text-faint；' +
      '/40 /50 只留给不带字号的装饰图标：',
  );
});

test('字号只从三档取，不写任意值', () => {
  const hits = scanLines((line) => {
    const m = line.match(/text-\[\d+(?:\.\d+)?px\]/);
    return m ? `任意字号 ${m[0]}` : null;
  });
  assertClean(
    hits,
    '字号只有 text-xs（13px）/ text-micro（11px）/ text-nano（10px）三档 —— 新档位要先' +
      '加进 tailwind.config.js 的 fontSize，再在组件里用名字（globals.css 的 `@apply` ' +
      '不在本断言的扫描范围里，那里还剩 prose 与表单内部样式）：',
  );
});

test('圆角不超过 lg', () => {
  const hits = scanLines((line) => {
    const m = line.match(/\brounded-(?:xl|2xl|3xl)\b/);
    return m ? `圆角 ${m[0]}` : null;
  });
  assertClean(hits, 'xl 以上是卡片与窗口的量级（窗口 8px），组件内用 rounded-md / rounded-lg：');
});

test('虚线边框只有 EmptyHint 一套语汇', () => {
  // 两个白名单意思不同：落区（拖文件进来）与虚线按钮不是空态，故保留虚线。
  const ALLOW = new Set([
    'src/components/ui/empty-hint.tsx',
    // 文件落区：虚线即「把文件放进来」。
    'src/features/http-tester/BodyTables.tsx',
    // 虚线按钮：「添加环境」是可选动作，划线表示还没填。
    'src/features/settings/sections/rows/EnvSwitcher.tsx',
    // 常驻提示卡：说明性质，不是空态。
    'src/features/settings/SettingsPanel.tsx',
    // 「没有项目时也能做的事」那张卡。
    'src/features/welcome/WelcomeScreen.tsx',
  ]);
  const hits = scanLines((line) => (line.includes('border-dashed') ? '虚线边框' : null)).filter(
    (hit) => !ALLOW.has(hit.slice(0, hit.indexOf(':'))),
  );
  assertClean(
    hits,
    '空态只有 components/ui/empty-hint.tsx 一个实现（虚线的圆角、内边距、字色都在那里）：',
  );
});

test('hover 才现形的东西必须也能被键盘照出来', () => {
  const hits: string[] = [];
  for (const [name, src] of sources) {
    const literal = /(['"`])((?:\\.|(?!\1)[\s\S])*)\1/g;
    for (const m of src.matchAll(literal)) {
      const chunk = m[2];
      if (!chunk.includes('group-hover:opacity-100')) continue;
      if (!chunk.includes('opacity-0')) continue;
      // 任何一个 focus 通道都算：`focus-visible:` / `group-focus-within:` / `group-focus-visible:`。
      if (/focus[^ ]*:opacity-100/.test(chunk)) continue;
      hits.push(`${name} ${chunk.trim().slice(0, 80)}`);
    }
  }
  assertClean(
    hits,
    '这些元素只在 hover 时现形，键盘用户看不到也找不到。补 focus-visible:opacity-100 ' +
      '（或 group-focus-within:opacity-100，由可聚焦的父元素带出来）：',
  );
});

test('悬停与焦点走 overlay-hover，accent 只做选中', () => {
  const hits = scanLines((line) => {
    const m = line.match(/\b(?:hover|focus|active):bg-accent\b/);
    return m ? `用 accent 做交互态 ${m[0]}` : null;
  });
  assertClean(
    hits,
    'hover/focus/active 的底色用 bg-overlay-hover（或需要不透明时 bg-surface-hover）；' +
      'bg-accent 只表示「已选中」，加了交互前缀就变成另一种观感：',
  );
});
