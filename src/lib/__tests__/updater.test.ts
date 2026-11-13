/**
 * 自动更新模块的两条硬约束。
 *
 * 1. 非 Tauri 壳（`serve-web`、纯 Node、Vite dev）里 `checkDesktopUpdate()` 必须安静地
 *    返回 `null`。它由界面在启动时调用，抛异常会让「检查更新」变成控制台里的一串红字，
 *    而那个形态本来就没有更新可装。
 * 2. `@tauri-apps/plugin-updater` 与 `@tauri-apps/plugin-process` 不得在模块顶部静态
 *    import。这两个包在浏览器形态里没有实现，静态引入会把它们拖进 `serve-web` 的
 *    bundle。第二条只读源码，与 `tab-strip-animation.test.ts` 的哨兵同一写法。
 *
 * `IS_TAURI_SHELL` 在模块求值时定值，故先清掉可能存在的 shim 再 import ——
 * 同 `src/stores/__tests__/_browser-shim.ts` 处理 `window.__TAURI_INTERNALS__` 的理由。
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const g = globalThis as {
  window?: { __TAURI_INTERNALS__?: unknown; __TAURI__?: unknown };
};
if (g.window) {
  delete g.window.__TAURI_INTERNALS__;
  delete g.window.__TAURI__;
}

const { checkDesktopUpdate, installDesktopUpdate, isUpdaterAvailable } =
  await import('@/lib/updater');

const FILE = resolve(process.cwd(), 'src/lib/updater.ts');

/** 去掉注释再匹配：注释里写到插件包名不该被当成 import。 */
function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .map((line) => line.replace(/(^|\s)\/\/.*$/, '$1'))
    .join('\n');
}

const code = stripComments(readFileSync(FILE, 'utf8'));

describe('updater 模块的形态边界', () => {
  it('非 Tauri 壳里没有更新的入口', () => {
    assert.equal(isUpdaterAvailable(), false);
  });

  it('非 Tauri 壳里 checkDesktopUpdate 返回 null，不抛', async () => {
    assert.equal(await checkDesktopUpdate(), null);
  });

  it('非 Tauri 壳里 installDesktopUpdate 抛出 Error，而不是静默成功', async () => {
    await assert.rejects(
      () => installDesktopUpdate(() => {}),
      (err: unknown) => err instanceof Error && /不支持自动更新/.test(err.message),
    );
  });
});

describe('updater 模块的 import 形态', () => {
  it('插件包没有静态 import', () => {
    assert.doesNotMatch(code, /from\s+['"]@tauri-apps\/plugin-updater['"]/);
    assert.doesNotMatch(code, /from\s+['"]@tauri-apps\/plugin-process['"]/);
  });

  it('插件包走动态 import', () => {
    assert.match(code, /import\(\s*['"]@tauri-apps\/plugin-updater['"]\s*\)/);
    assert.match(code, /import\(\s*['"]@tauri-apps\/plugin-process['"]\s*\)/);
  });
});
