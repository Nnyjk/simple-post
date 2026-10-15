/**
 * web 形态 `invokeCmd` 的分发行为 —— 中止信号转发、未映射命令的抛错，以及反向代理
 * base path 下的前缀拼接。
 *
 * `readApiBase` 的注入值在模块入口由 serve-web 写进 SPA shell，故这里直接摆弄
 * `globalThis.window`。每一组都在 `after` 里把它还原。
 *
 * 路径与请求体的逐条映射见 `tauri-commands-routes-*.test.ts`。
 */
import { describe, it, before, after, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { COMMANDS, TauriCommand, invokeCmd, readApiBase } from '../tauri-commands';
import { calls, installFetchMock, resetCalls, restoreFetchMock } from './tauri-commands-fetch-mock';
// ---------------------------------------------------------------------------
// 把 options.signal 转发给 fetch。
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — abort signal forwarding', () => {
  before(() => installFetchMock({ json: {}, status: 200 }));
  after(() => restoreFetchMock());

  it('forwards options.signal to fetch', async () => {
    resetCalls();
    const ctrl = new AbortController();
    await invokeCmd(COMMANDS.projectsList, undefined, { signal: ctrl.signal });
    assert.ok(calls.length === 1);
    assert.ok(calls[0]!.signal, 'expected signal to be forwarded');
    // 两个引用指向同一个控制器的 signal。
    assert.ok(calls[0]!.signal === ctrl.signal);
  });
});

// ---------------------------------------------------------------------------
// 错误路径 —— 没有 `WEB_ROUTES` 条目的命令必须抛出，并同时指向两个源文件。
// ---------------------------------------------------------------------------

describe('invokeCmd (web shape) — error path', () => {
  before(() => installFetchMock({ json: {}, status: 200 }));
  after(() => restoreFetchMock());

  it('throws a self-documenting error when the command is unmapped', async () => {
    // 命令未映射时抛出自带说明的错误。
    // 经 `unknown` 强转，因为没有任何公开字面量代表未映射的命令（否则 TypeScript
    // 会在调用点就报错）。
    const fakeCmd = 'this_command_does_not_exist' as unknown as TauriCommand;
    await assert.rejects(() => invokeCmd(fakeCmd), /src\/lib\/tauri-commands\.ts/);
    await assert.rejects(() => invokeCmd(fakeCmd), /src-tauri\/src\/web_api\//);
    // `fetch` 必须**没有**被调用——这次抛错要在任何网络动作之前短路，否则来自 404 的
    // 错误会误导人。
    assert.equal(calls.length, 0);
  });
});

// ---------------------------------------------------------------------------
// 反向代理 base path。
//
// `serve-web` 可以把整个 router 嵌在一个前缀下（`--public-base-path` /
// `SIMPLE_POST_PUBLIC_BASE_PATH`），并在模块入口运行前把该值以
// `window.__SIMPLE_POST_BASE__` 注入 SPA shell。渲染层必须读它，因为每条
// `WEB_ROUTES` 路径都是根绝对的（`/api/projects`），而嵌套后的 router 从不服务裸根路径。
//
// 这一块是单测看不到的另一半：集成测试（`scripts/test-web-basepath.mjs`）断言*服务端*
// 嵌套正确，而本文件断言渲染层把命令映射到路径。两者都没断言它们实际一致的那个 URL。
// ---------------------------------------------------------------------------

describe('readApiBase', () => {
  const realWindow = (globalThis as { window?: unknown }).window;

  afterEach(() => {
    if (realWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else (globalThis as { window?: unknown }).window = realWindow;
  });

  it('is empty in Node, where there is no window at all', () => {
    delete (globalThis as { window?: unknown }).window;
    assert.equal(readApiBase(), '');
  });

  it('is empty for a root deployment that injects nothing', () => {
    (globalThis as { window?: unknown }).window = {};
    assert.equal(readApiBase(), '');
  });

  it('returns the injected prefix', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp' };
    assert.equal(readApiBase(), '/sp');
  });

  // `nest` 会去掉尾斜杠，所以运维写 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp/` 时不该产出
  // `/sp//api/...`。
  it('normalises a trailing slash so URLs do not gain a double slash', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp/' };
    assert.equal(readApiBase(), '/sp');
  });

  it('ignores a non-string injection', () => {
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: 42 };
    assert.equal(readApiBase(), '');
  });
});

describe('invokeCmd (web shape) under a reverse-proxy base path', () => {
  const realWindow = (globalThis as { window?: unknown }).window;

  beforeEach(() => {
    installFetchMock({ json: [], status: 200 });
    (globalThis as { window?: unknown }).window = { __SIMPLE_POST_BASE__: '/sp' };
  });
  after(() => {
    restoreFetchMock();
    if (realWindow === undefined) delete (globalThis as { window?: unknown }).window;
    else (globalThis as { window?: unknown }).window = realWindow;
  });

  it('requests the prefixed URL, not the bare root', async () => {
    await invokeCmd(COMMANDS.projectsList);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].url, '/sp/api/projects');
  });

  it('prefixes path params too', async () => {
    await invokeCmd(COMMANDS.projectsGet, { id: 'proj_1' });
    assert.equal(calls[0].url, '/sp/api/projects/proj_1');
  });

  it('falls back to the bare root when no prefix is injected', async () => {
    (globalThis as { window?: unknown }).window = {};
    await invokeCmd(COMMANDS.projectsList);
    assert.equal(calls[0].url, '/api/projects');
  });
});
