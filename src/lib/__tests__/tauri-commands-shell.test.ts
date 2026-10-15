/**
 * `IS_TAURI_SHELL` 的形态判定 —— 必须在隔离子进程里跑。
 *
 * 该值在模块求值时就被捕获，同一进程里没法重新切换（见 `tauri-commands.ts` 顶部）。
 */
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';

import { IS_TAURI_SHELL } from '../tauri-commands';

/**
 * 起一个全新的 Node 进程，加载 `tauri-commands.ts` 并把 `IS_TAURI_SHELL` 以 JSON
 * 打印出来。可选的 `window` 对象会在 import 模块**之前**装到 `globalThis` 上，好让
 * 模块加载时的检测看到正确的形态。
 */
function readIsTauriShellInChild(windowShim?: Record<string, unknown>): {
  value: boolean;
  stderr: string;
} {
  const setup = windowShim
    ? `globalThis.window = ${JSON.stringify(windowShim)};`
    : `delete globalThis.window;`;
  // 子进程里的动态 import 在 Windows 上需要 `file://` URL；裸的绝对路径会被当成
  // URL 解析并拒绝。`new URL(rel, import.meta.url).href` 返回的就是 file URL 字符串，
  // 正是动态 `import()` 要的形状。相对*本*文件，所以要 `../` 跳出 `__tests__`。
  const moduleUrl = new URL('../tauri-commands.ts', import.meta.url).href;
  const code = [
    setup,
    `import('${moduleUrl}')`,
    `.then((m) => {`,
    `  process.stdout.write(JSON.stringify(m.IS_TAURI_SHELL));`,
    `})`,
    `.catch((e) => {`,
    `  process.stderr.write(String(e && e.stack || e));`,
    `  process.exit(1);`,
    `});`,
  ].join('\n');
  const result = spawnSync(
    process.execPath,
    ['--import', 'tsx/esm', '--input-type=module', '--eval', code],
    { encoding: 'utf-8' },
  );
  if (result.status !== 0) {
    throw new Error(`child failed (status=${result.status}): ${result.stderr}`);
  }
  return { value: JSON.parse(result.stdout.trim()), stderr: result.stderr };
}
describe('IS_TAURI_SHELL (child-process probe)', () => {
  it('is false when no window globals are set', () => {
    const { value } = readIsTauriShellInChild(undefined);
    assert.equal(value, false);
  });

  it('is true when window.__TAURI_INTERNALS__ is set', () => {
    const { value } = readIsTauriShellInChild({ __TAURI_INTERNALS__: {} });
    assert.equal(value, true);
  });

  it('is true when window.__TAURI__ (v1 shim) is set', () => {
    const { value } = readIsTauriShellInChild({ __TAURI__: {} });
    assert.equal(value, true);
  });

  it('in this test file (no window) it must be false', () => {
    // 健全性检查——保证本套件余下部分都跑在 web 形态。
    assert.equal(IS_TAURI_SHELL, false);
  });
});
