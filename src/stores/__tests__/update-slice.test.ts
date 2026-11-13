/**
 * 更新切片的契约测试：三条路由的调用、执行结果的保留、以及「已忽略」与节流记账
 * 不会把同一份偏好里的其它字段抹掉。
 *
 * 宿主判定（`isUpdaterAvailable`）在 Node 里恒为「桌面」（shim 装了
 * `__TAURI_INTERNALS__`），故这里不测桌面分支的插件调用；测的是与宿主无关的那半：
 * `applyUpdate` 的往返、`dismissUpdate` 的合并写、`autoCheckUpdate` 的 24 小时节流。
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import {
  clearIpcMock,
  installBrowserShim,
  installIpcMock,
  type ShimGlobals,
} from './_browser-shim';
import { createUpdateSlice, type UpdateSlice } from '../update-slice';
import { isUpdaterAvailable } from '@/lib/updater';
import { readUpdatePrefs, saveLastProjectId, saveLastUpdateCheckAt } from '../slices/ui-prefs';
import type { UpdateApplyResult } from '@/types/update';

type CapturedCall = { cmd: string; args: Record<string, unknown> };

function makeHarness() {
  const bag: Record<string, unknown> & Partial<UpdateSlice> = {
    updateInfo: null,
    updateDeployment: null,
    updateBusy: false,
    updateError: null,
    dismissedVersion: null,
    updateReleaseDate: null,
    updateProgress: null,
    updatePhase: null,
    applyResult: null,
  };
  const set = (partial: unknown) => {
    const next =
      typeof partial === 'function' ? (partial as (state: typeof bag) => unknown)(bag) : partial;
    if (next && typeof next === 'object') Object.assign(bag, next);
  };
  const get = () => bag;
  Object.assign(bag, createUpdateSlice(set as never, get as never, {} as never));
  return bag as Record<string, unknown> & UpdateSlice;
}

let shim: ShimGlobals;

beforeEach(() => {
  shim = installBrowserShim();
  window.localStorage.clear();
});

afterEach(() => {
  clearIpcMock();
  shim.teardown();
});

describe('update slice — applyUpdate', () => {
  it('把 update_apply 的结果整份存下，失败的退出码不丢', async () => {
    const calls: CapturedCall[] = [];
    const result: UpdateApplyResult = {
      command: 'docker compose pull simple-post',
      exitCode: 2,
      stdout: 'pulling',
      stderr: 'denied',
    };
    installIpcMock((cmd, args) => {
      calls.push({ cmd, args });
      return result;
    });

    const slice = makeHarness();
    await slice.applyUpdate();

    assert.deepEqual(
      calls.map((c) => c.cmd),
      ['update_apply'],
    );
    assert.deepEqual(slice.applyResult, result);
    assert.equal(slice.updateBusy, false);
    assert.equal(slice.updateError, null);
  });

  it('命令被拒时把原因写进 updateError，且不留下上一次的结果', async () => {
    const slice = makeHarness();
    slice.applyResult = {
      command: 'old',
      exitCode: 0,
      stdout: '',
      stderr: '',
    };
    installIpcMock(() => {
      throw new Error('update is not enabled on this deployment');
    });

    await slice.applyUpdate();

    assert.equal(slice.applyResult, null);
    assert.match(slice.updateError ?? '', /not enabled/);
    assert.equal(slice.updateBusy, false);
  });
});

describe('update slice — 桌面分支与 web 专用命令的边界', () => {
  it('桌面形态且插件报无新版时，checkForUpdate / loadDeployment 一次也不调用 update_* 命令', async () => {
    // 本用例的前提：测试环境装了 `__TAURI_INTERNALS__`，故宿主判定为「桌面」。
    assert.equal(isUpdaterAvailable(), true);

    const calls: string[] = [];
    installIpcMock((cmd) => {
      calls.push(cmd);
      // 插件没有可用更新、也读不到版本号 —— 这是「为了 notes 再打一次网络」最容易被
      // 顺手写错的一条路径：没有对话框要渲染，那次调用只有配额成本。
      return undefined;
    });

    const slice = makeHarness();
    await slice.checkForUpdate(false);
    await slice.loadDeployment();

    assert.deepEqual(
      calls.filter((cmd) => cmd.startsWith('update_')),
      [],
    );
    assert.equal(slice.updateInfo?.available, false);
    assert.equal(slice.updateDeployment, null);
  });
});

describe('update slice — 桌面端用线上发布说明覆盖 latest.json 的装机说明', () => {
  // 插件报的那份：`tauri-action` 在构建期从 `releaseBody` 模板写进 `latest.json`。
  const pluginUpdate = {
    version: '0.1.35',
    body: '请下载对应平台的安装包并运行',
    date: '2026-11-13T02:00:00Z',
    close: async () => undefined,
  };

  const mockDesktop = (onCheck: () => unknown) =>
    installIpcMock((cmd) => {
      if (cmd.includes('updater')) return pluginUpdate;
      if (cmd.includes('app|version')) return '0.1.34';
      if (cmd === 'update_check') return onCheck();
      return undefined;
    });

  it('update_check 拿到 notes 时，notes 与 url 都来自它', async () => {
    mockDesktop(() => ({
      current: '0.1.34',
      latest: '0.1.35',
      available: true,
      checkedAt: '2026-11-13T02:05:00Z',
      cached: false,
      notes: '## 这一版\n- 更新对话框认得线上的发布说明',
      url: 'https://github.com/Nnyjk/simple-post/releases/tag/v0.1.35',
      error: null,
    }));

    const slice = makeHarness();
    await slice.checkForUpdate(false);

    assert.equal(slice.updateInfo?.notes, '## 这一版\n- 更新对话框认得线上的发布说明');
    assert.equal(
      slice.updateInfo?.url,
      'https://github.com/Nnyjk/simple-post/releases/tag/v0.1.35',
    );
    // 版本、发布时间仍以插件为准：按钮装的就是插件报的那个版本。
    assert.equal(slice.updateInfo?.latest, '0.1.35');
    assert.equal(slice.updateReleaseDate, '2026-11-13T02:00:00Z');
    assert.equal(slice.updateError, null);
  });

  it('update_check 抛错时静默回落到插件的 notes，url 为空且不置错误', async () => {
    mockDesktop(() => {
      throw new Error('command update_check not found');
    });

    const slice = makeHarness();
    await slice.checkForUpdate(false);

    assert.equal(slice.updateInfo?.available, true);
    assert.equal(slice.updateInfo?.notes, pluginUpdate.body);
    assert.equal(slice.updateInfo?.url, null);
    // 补充信息失败不是用户要处理的事：对话框照常打开，不弹错误条。
    assert.equal(slice.updateError, null);
    assert.equal(slice.updateBusy, false);
  });
});

describe('update slice — 已忽略', () => {
  it('记住版本号，且不抹掉同一份偏好里的其它字段', () => {
    saveLastProjectId('p1');
    const slice = makeHarness();

    slice.dismissUpdate('0.1.35');

    assert.equal(slice.dismissedVersion, '0.1.35');
    const raw = window.localStorage.getItem('simple-post:prefs') ?? '';
    assert.match(raw, /"lastProjectId":"p1"/);
    assert.equal(readUpdatePrefs().dismissedVersion, '0.1.35');
  });
});

describe('update slice — 自动检查的节流', () => {
  it('刚查过就再启动 → 不查', async () => {
    saveLastUpdateCheckAt(new Date().toISOString());
    const slice = makeHarness();
    let checked = 0;
    slice.checkForUpdate = async () => {
      checked += 1;
    };

    await slice.autoCheckUpdate();

    assert.equal(checked, 0);
  });

  it('从未查过 → 查一次，且不强制绕缓存', async () => {
    const slice = makeHarness();
    const forces: boolean[] = [];
    slice.checkForUpdate = async (force) => {
      forces.push(Boolean(force));
    };

    await slice.autoCheckUpdate();

    assert.deepEqual(forces, [false]);
  });

  it('超过 24 小时 → 再查一次', async () => {
    saveLastUpdateCheckAt(new Date(Date.now() - 25 * 60 * 60 * 1000).toISOString());
    const slice = makeHarness();
    let checked = 0;
    slice.checkForUpdate = async () => {
      checked += 1;
    };

    await slice.autoCheckUpdate();

    assert.equal(checked, 1);
  });
});
