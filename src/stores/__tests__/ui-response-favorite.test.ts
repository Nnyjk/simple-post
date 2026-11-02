/**
 * 响应历史的收藏、保留条数与请求快照 —— `ui-response` 与 `ui-prefs` 两个切片的新行为。
 *
 * 跑在**真实组合 store** 上，与 `ui-response-history.test.ts` 同一路子：这些行为跨切片
 * （历史列表住在 `ui-response`，保留条数住在 `ui-prefs`，裁剪时两者要碰面），手搭的
 * harness 恰好会把最容易坏的那一半 stub 掉。
 *
 * 有意思的断言都关于用户会悄无声息搞错的事：
 *   - 收藏的行**不计入**保留条数，故裁剪只丢未收藏的那些；
 *   - 取消收藏后那一行重新受条数约束；
 *   - 「清空」只清未收藏的行 —— 用户被承诺过收藏的行永久保存；
 *   - 改保留条数会立刻裁本地列表，否则界面会多显示库里已经没有的行；
 *   - 请求快照与响应同一刻写进 store，故响应区两栏描述同一次调用。
 *
 * 运行：npx tsx --test src/stores/__tests__/ui-response-favorite.test.ts
 */
import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';

import { installBrowserShim, clearIpcMock, installIpcMock } from './_browser-shim';

import { useAppStore } from '../app-store';
import { capUnfavorited } from '../slices/history-keep';
import type { ApiResponse, ResponseHistoryEntry, StoredRequestSnapshot } from '@/types/domain';

type IpcCall = { cmd: string; args: Record<string, unknown> };

const ENDPOINT_ID = 'ep1';

function makeResponse(over: Partial<ApiResponse> = {}): ApiResponse {
  return {
    status: 200,
    statusText: 'OK',
    durationMs: 12,
    sizeBytes: 2,
    headers: {},
    body: 'hi',
    contentType: 'text/plain',
    method: 'GET',
    url: '/x',
    ...over,
  };
}

const SNAPSHOT: StoredRequestSnapshot = {
  method: 'GET',
  url: 'https://example.test/x?a=1',
  headers: [['X-Token', 'abc']],
  body: '{"k":1}',
};

function makeEntry(id: string, over: Partial<ResponseHistoryEntry> = {}): ResponseHistoryEntry {
  return {
    id,
    endpointId: ENDPOINT_ID,
    response: makeResponse(),
    bodyTruncated: false,
    createdAt: 1,
    isFavorite: false,
    ...over,
  };
}

let calls: IpcCall[] = [];
let origWarn: typeof console.warn;
let origError: typeof console.error;

beforeEach(() => {
  installBrowserShim();
  calls = [];
  origWarn = console.warn.bind(console);
  origError = console.error.bind(console);
  console.warn = () => {};
  console.error = () => {};

  installIpcMock(async (cmd, args) => {
    calls.push({ cmd, args: args ?? {} });
    if (cmd === 'response_history_append') return makeEntry('rh-server');
    if (cmd === 'response_history_set_favorite') return true;
    if (cmd === 'response_history_keep_set') return args.keep as number;
    if (cmd === 'response_history_clear') return 1;
    throw new Error(`unexpected command: ${cmd}`);
  });

  useAppStore.setState({
    responseHistory: {},
    lastResponse: null,
    lastRequest: null,
    responseHistoryKeep: 10,
  });
});

afterEach(() => {
  console.warn = origWarn;
  console.error = origError;
  clearIpcMock();
});

const cmdNames = () => calls.map((c) => c.cmd);

describe('capUnfavorited 只丢未收藏的行', () => {
  it('超出的部分是未收藏里最旧的那几行，收藏的行一条不动', () => {
    const list = [
      makeEntry('a', { isFavorite: true }),
      makeEntry('b'),
      makeEntry('c'),
      makeEntry('d', { isFavorite: true }),
      makeEntry('e'),
    ];
    const kept = capUnfavorited(list, 1);
    assert.deepEqual(
      kept.map((e) => e.id),
      ['a', 'd', 'e'],
      '未收藏的 b 被丢掉（它比 e 旧），收藏的 a 与 d 留下',
    );
  });

  it('未收藏的行不够条数时原样返回', () => {
    const list = [makeEntry('a', { isFavorite: true }), makeEntry('b')];
    assert.deepEqual(
      capUnfavorited(list, 10).map((e) => e.id),
      ['a', 'b'],
    );
  });
});

describe('setResponseHistoryFavorite', () => {
  it('本地先改，星标立刻变，落盘在其后', () => {
    const { setResponseHistoryFavorite } = useAppStore.getState();
    useAppStore.setState({
      responseHistory: { [ENDPOINT_ID]: [makeEntry('rh-1')] },
    });

    setResponseHistoryFavorite('rh-1', true);

    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.equal(rows[0].isFavorite, true, '星标不等 IPC 就变');
    assert.deepEqual(cmdNames(), ['response_history_set_favorite']);
    assert.deepEqual(calls[0].args, { id: 'rh-1', favorite: true });
  });

  it('取消收藏把那行改回未收藏，它重新受条数约束', () => {
    const { setResponseHistoryFavorite } = useAppStore.getState();
    useAppStore.setState({
      responseHistory: { [ENDPOINT_ID]: [makeEntry('rh-1', { isFavorite: true })] },
    });

    setResponseHistoryFavorite('rh-1', false);

    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.equal(rows[0].isFavorite, false);
    assert.deepEqual(calls[0].args, { id: 'rh-1', favorite: false });
  });

  it('写盘失败时本地仍显示收藏状态', async () => {
    installIpcMock(async (cmd) => {
      if (cmd === 'response_history_set_favorite') throw new Error('favorite failed');
      throw new Error(`unexpected command: ${cmd}`);
    });
    useAppStore.setState({ responseHistory: { [ENDPOINT_ID]: [makeEntry('rh-1')] } });

    useAppStore.getState().setResponseHistoryFavorite('rh-1', true);
    await new Promise((resolve) => setTimeout(resolve, 0));

    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.equal(rows[0].isFavorite, true, '用户标出的那一行不该因为一次写失败就从界面上消失');
  });
});

describe('trimResponseHistory 与保留条数', () => {
  it('只裁未收藏的行，收藏的行留下', () => {
    useAppStore.setState({
      responseHistory: {
        [ENDPOINT_ID]: [
          makeEntry('old-fav', { isFavorite: true }),
          makeEntry('old-1'),
          makeEntry('old-2'),
          makeEntry('new-1'),
        ],
      },
    });

    useAppStore.getState().trimResponseHistory(1);

    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.deepEqual(
      rows.map((e) => e.id),
      ['old-fav', 'new-1'],
      '未收藏的 old-1 与 old-2 被丢掉，收藏的 old-fav 留下',
    );
  });

  it('改保留条数会写库，并按新条数裁本地列表', async () => {
    useAppStore.setState({
      responseHistory: {
        [ENDPOINT_ID]: [makeEntry('a'), makeEntry('b'), makeEntry('c')],
      },
    });

    useAppStore.getState().updateResponseHistoryKeep(2);
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(useAppStore.getState().responseHistoryKeep, 2);
    assert.deepEqual(cmdNames(), ['response_history_keep_set']);
    assert.deepEqual(calls[0].args, { keep: 2 });
    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.deepEqual(
      rows.map((e) => e.id),
      ['b', 'c'],
      '库里已经被服务端裁掉了最旧的一行，本地也要跟着裁，否则界面多显示一行',
    );
  });

  it('保留条数被夹到 1 与上限之间', () => {
    const { updateResponseHistoryKeep } = useAppStore.getState();
    updateResponseHistoryKeep(0);
    assert.equal(useAppStore.getState().responseHistoryKeep, 1);
    updateResponseHistoryKeep(9999);
    assert.equal(useAppStore.getState().responseHistoryKeep, 500);
  });
});

describe('clearResponseHistory 只清未收藏的行', () => {
  it('收藏的行留下，未收藏的行删掉', async () => {
    useAppStore.setState({
      responseHistory: {
        [ENDPOINT_ID]: [
          makeEntry('fav', { isFavorite: true }),
          makeEntry('plain-1'),
          makeEntry('plain-2'),
        ],
      },
    });

    await useAppStore.getState().clearResponseHistory(ENDPOINT_ID);

    const rows = useAppStore.getState().responseHistory[ENDPOINT_ID] ?? [];
    assert.deepEqual(
      rows.map((e) => e.id),
      ['fav'],
      '用户被承诺过收藏的行永久保存，清空动作不能把它一起清掉',
    );
    assert.deepEqual(cmdNames(), ['response_history_clear']);
  });
});

describe('请求快照与响应同一刻写入', () => {
  it('setLastResponse 同时写响应与请求', () => {
    const resp = makeResponse();
    useAppStore.getState().setLastResponse(resp, SNAPSHOT);

    assert.equal(useAppStore.getState().lastResponse, resp);
    assert.deepEqual(useAppStore.getState().lastRequest, SNAPSHOT);
  });

  it('不带快照时 lastRequest 为 null，界面据此不画请求侧栏', () => {
    useAppStore.getState().setLastResponse(makeResponse());
    assert.equal(useAppStore.getState().lastRequest, null);
  });

  it('写历史时把快照一并交给服务端', async () => {
    useAppStore.getState().addResponseHistory(ENDPOINT_ID, makeResponse(), SNAPSHOT);
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.deepEqual(cmdNames(), ['response_history_append']);
    assert.deepEqual(calls[0].args.request, SNAPSHOT);
  });
});
