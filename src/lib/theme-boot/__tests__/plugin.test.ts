/**
 * 构建期注入的那段脚本与共享模块必须给出同一个答案。
 *
 * `index.html` 的首帧脚本是编译期生成的，生成之后它与 `src/lib/theme-boot/index.ts`
 * 就是两份文本。故这里断言的是**行为**：同一份 localStorage 内容，注入的脚本与模块
 * 算出来的深浅色必须一致——不一致就是首帧画深色、随后 React 画浅色那种闪烁。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

import themeBoot from '../index';
import { buildThemeBootScript, themeBootPlugin, THEME_BOOT_MARKER } from '../plugin';

/** 共享模块的源文件。插件编译的就是它。 */
const SOURCE = readFileSync(new URL('../index.ts', import.meta.url), 'utf8');

/** 一份最小的 `index.html`：只有插件要替换的那个标记。 */
const HTML = `<!doctype html><html><head>${THEME_BOOT_MARKER}</head><body></body></html>`;

/** 从插件的 hook 里取出处理函数。Vite 的 hook 有函数与对象两种形状。 */
function handlerOf(plugin: ReturnType<typeof themeBootPlugin>): (html: string) => unknown {
  const hook = plugin.transformIndexHtml as unknown as
    | ((html: string, ctx: unknown) => unknown)
    | { handler: (html: string, ctx: unknown) => unknown };
  const fn = typeof hook === 'function' ? hook : hook.handler;
  return (html: string) => fn(html, {});
}

async function inject(html: string): Promise<string> {
  const plugin = themeBootPlugin(SOURCE);
  const result = await handlerOf(plugin)(html);
  if (typeof result === 'string') return result;
  return (result as { html: string }).html;
}

/** 取出注入 HTML 里的那段脚本。 */
function scriptOf(html: string): string {
  const m = /<script>([\s\S]*?)<\/script>/.exec(html);
  assert.ok(m, '注入后的 HTML 里应当有一段脚本');
  return m![1];
}

/**
 * 在假环境里跑注入的那段脚本，返回 `<html>` 上最终有没有 `dark` 这个 class。
 */
function runInjected(script: string, stored: Record<string, string>, systemDark: boolean): boolean {
  const storage = new Map<string, string>(Object.entries(stored));
  let darkClass = false;
  const sandbox = {
    window: {
      localStorage: {
        getItem: (k: string) => (storage.has(k) ? storage.get(k)! : null),
      },
      matchMedia: (query: string) => ({
        matches: query.includes('dark') ? systemDark : false,
      }),
    },
    document: {
      documentElement: {
        classList: {
          toggle: (name: string, on: boolean) => {
            if (name === 'dark') darkClass = on;
          },
        },
      },
    },
  };
  vm.runInNewContext(script, sandbox);
  return darkClass;
}

/** 模块那一侧的答案：读同一份 localStorage 内容，算出要不要深色。 */
function expectedFromModule(raw: string | null, systemDark: boolean): boolean {
  const g = globalThis as any;
  const before = g.window;
  g.window = {
    matchMedia: (query: string) => ({ matches: query.includes('dark') ? systemDark : false }),
  };
  try {
    const theme = themeBoot.resolveTheme(themeBoot.parsePrefs(raw).theme);
    return themeBoot.prefersDark(theme);
  } finally {
    if (before === undefined) delete g.window;
    else g.window = before;
  }
}

describe('注入的那段脚本', () => {
  it('是脚本标签里的 classic script，且不含模块语法', async () => {
    const html = await inject(HTML);
    assert.ok(html.includes('<script>'), '标记应当被换成一段内联脚本');
    const script = scriptOf(html);
    assert.ok(!/^[ \t]*export\b/m.test(script), 'classic script 里不能有 export');
    assert.ok(!/^[ \t]*import\b/m.test(script), 'classic script 里不能有 import');
    assert.ok(!/\bimport\s*\(/.test(script), 'classic script 里不能有动态 import');
  });

  it('读的是共享模块里写的那个偏好键', async () => {
    const html = await inject(HTML);
    assert.ok(scriptOf(html).includes(themeBoot.PREFS_KEY), '注入的脚本里应当出现共享模块那个键');
    assert.equal(themeBoot.PREFS_KEY, 'simple-post:prefs');
  });

  it('把共享模块挂在全局上，末尾调用一次应用主题', async () => {
    const script = scriptOf(await inject(HTML));
    assert.ok(script.includes('globalThis.__simplePostTheme'), '共享模块要挂到全局上');
    assert.ok(
      script.includes('globalThis.__simplePostTheme.applyInitialTheme();'),
      '脚本一被解析就该把主题写到 <html> 上',
    );
  });

  it('与共享模块对同一份 localStorage 给出同一个答案', async () => {
    const script = scriptOf(await inject(HTML));
    const cases: Array<{ name: string; stored: Record<string, string>; systemDark: boolean }> = [
      { name: '没有记录', stored: {}, systemDark: false },
      {
        name: '存的是 light',
        stored: { 'simple-post:prefs': '{"theme":"light"}' },
        systemDark: false,
      },
      {
        name: '存的是 dark',
        stored: { 'simple-post:prefs': '{"theme":"dark"}' },
        systemDark: true,
      },
      {
        name: '存的是 system 且系统是深色',
        stored: { 'simple-post:prefs': '{"theme":"system"}' },
        systemDark: true,
      },
      {
        name: '存的是 system 且系统是浅色',
        stored: { 'simple-post:prefs': '{"theme":"system"}' },
        systemDark: false,
      },
      {
        name: '存的是不合法的主题',
        stored: { 'simple-post:prefs': '{"theme":"blue"}' },
        systemDark: false,
      },
      {
        name: '存的是坏掉的 JSON',
        stored: { 'simple-post:prefs': '{not json' },
        systemDark: false,
      },
      { name: '存的是 null', stored: { 'simple-post:prefs': 'null' }, systemDark: false },
      {
        name: '存的主题是 null',
        stored: { 'simple-post:prefs': '{"theme":null}' },
        systemDark: false,
      },
    ];

    for (const c of cases) {
      const raw = c.stored['simple-post:prefs'] ?? null;
      const got = runInjected(script, c.stored, c.systemDark);
      const want = expectedFromModule(raw, c.systemDark);
      assert.equal(got, want, `${c.name}：注入的脚本与共享模块的答案必须一致`);
    }
  });
});

describe('形状不对时构建期就失败', () => {
  it('共享模块多了一个导出会被拒', async () => {
    await assert.rejects(
      () => buildThemeBootScript('export const a = 1;\nexport default { a };\n'),
      /导出/,
    );
  });

  it('共享模块没有默认导出会被拒', async () => {
    await assert.rejects(() => buildThemeBootScript('const a = 1;\n'), /默认导出/);
  });

  it('共享模块带 import 会被拒', async () => {
    await assert.rejects(
      () => buildThemeBootScript("import x from 'y';\nexport default { x };\n"),
      /import/,
    );
  });

  it('index.html 里没有标记会被拒', async () => {
    await assert.rejects(() => inject('<!doctype html><html></html>'), /标记/);
  });
});
