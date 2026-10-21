/**
 * `ui-prefs` 的请求旋钮契约：旋钮存在后端，不在 localStorage。
 *
 * 覆盖三件事：改旋钮会把**完整三项**发给后端（web 形态的 `/api/request-settings`
 * 是全量替换语义，发补丁会丢掉另外两项）、读旋钮时用后端的值、以及旧版本留在
 * localStorage 的那份被一次性迁进库后删掉。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { clearIpcMock, installBrowserShim, installIpcMock } from './_browser-shim';
import { createUiPrefs, loadRequestSettings } from '../slices/ui-prefs';
import type { RequestSettings } from '../store-types';

type CapturedCall = { cmd: string; args: Record<string, unknown> };

function makePrefsHarness() {
  const bag: Record<string, unknown> = {};
  const set = (partial: unknown) =>
    Object.assign(
      bag,
      typeof partial === 'function' ? (partial as (s: unknown) => unknown)(bag) : partial,
    );
  const get = () => bag as never;
  const slice = createUiPrefs(set as never, get, undefined as never);
  Object.assign(bag, slice);
  return { bag, slice };
}

describe('ui-prefs: 请求旋钮存在后端', () => {
  beforeEach(() => {
    installBrowserShim();
    window.localStorage.clear();
  });

  afterEach(() => {
    clearIpcMock();
  });

  it('改一项也要把完整三项发出去', async () => {
    const harness = makePrefsHarness();
    const calls: CapturedCall[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      return null;
    });

    harness.slice.updateRequestSettings({ timeoutMs: 5000 });
    // 写入是 fire-and-forget，等一个宏任务让它发出去。
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(
      (harness.bag.requestSettings as RequestSettings).timeoutMs,
      5000,
      '本地副本立刻更新',
    );
    assert.deepEqual(calls, [
      {
        cmd: 'request_settings_update',
        args: {
          patch: {
            timeoutMs: 5000,
            followRedirects: true,
            maxResponseSizeKb: 5120,
          },
        },
      },
    ]);
  });

  it('读的是后端存的那份', async () => {
    const calls: CapturedCall[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'request_settings_get') {
        return { timeoutMs: 1234, followRedirects: false, maxResponseSizeKb: 999 };
      }
      return null;
    });

    const settings = await loadRequestSettings();
    assert.deepEqual(settings, {
      timeoutMs: 1234,
      followRedirects: false,
      maxResponseSizeKb: 999,
    });
    assert.deepEqual(
      calls.map((c) => c.cmd),
      ['request_settings_get'],
      '没有旧值时不发写入',
    );
  });

  it('后端取不到时用默认值', async () => {
    installIpcMock(async () => {
      throw new Error('offline');
    });
    const settings = await loadRequestSettings();
    assert.deepEqual(settings, {
      timeoutMs: 30_000,
      followRedirects: true,
      maxResponseSizeKb: 5_120,
    });
  });

  it('旧版本留在 localStorage 的那份被迁进库，然后删掉', async () => {
    window.localStorage.setItem(
      'simple-post:prefs',
      JSON.stringify({ theme: 'dark', requestSettings: { timeoutMs: 4321 } }),
    );
    const calls: CapturedCall[] = [];
    installIpcMock(async (cmd, args) => {
      calls.push({ cmd, args: args ?? {} });
      if (cmd === 'request_settings_get') {
        return { timeoutMs: 4321, followRedirects: true, maxResponseSizeKb: 5120 };
      }
      return null;
    });

    const settings = await loadRequestSettings();
    assert.equal(settings.timeoutMs, 4321);
    assert.equal(calls[0]?.cmd, 'request_settings_update', '旧值要先写进库');
    const left = JSON.parse(window.localStorage.getItem('simple-post:prefs') ?? '{}');
    assert.equal(
      left.requestSettings,
      undefined,
      '旧字段必须删掉，否则下一次启动它会盖掉用户新选的值',
    );
  });
});
