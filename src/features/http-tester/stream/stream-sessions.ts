/**
 * 流式会话注册表 —— 每个标签最多一条 SSE 流或一条 WebSocket 会话，键是标签 id。
 *
 * **控制器为什么不在组件里。** 会话必须活过标签内容的重新挂载：切到别的标签再切回来，流还在
 * 跑、帧与事件还在 store 里。故句柄存在这里，视图只读 store（`slices/stream-slice.ts`）。
 *
 * **谁负责关掉。** store 不知道传输层，故本模块自己订阅 `openTabs`：标签集合一变，就把不在
 * 其中的会话关掉（关标签、切项目、删除接口都是这条路径）。切换协议由调用方显式调
 * `disposeSession`——那时标签还在，只是同一份工作区换了形态。
 *
 * SSE 结束时在这里写一条 `response_history`：面板只是视图，历史行是用户之后能在响应历史里
 * 找到的那份记录，两者不能各写一半。
 */
import { openHttpStream, openWsSession } from '@/lib/stream-transport';
import type {
  HttpStreamArgs,
  HttpStreamHandle,
  SseMetaEvent,
  WsConnectArgs,
  WsFrameEvent,
  WsSessionHandle,
} from '@/lib/stream-transport';
import { createSseParser } from '@/lib/sse';
import { applyStreamAuth } from './stream-auth';
import type { SseEvent } from '@/lib/sse';
import type { PreparedRequest } from '@/lib/http-build';
import { uid } from '@/lib/utils';
import { useAppStore } from '@/stores/app-store';
import type { RequestSettings } from '@/stores/store-types';
import type { ApiResponse, AuthConfig, StoredRequestSnapshot, TlsSettings } from '@/types/domain';
import { toRequestSnapshot } from '../model/requestInput';
import { createReconnect } from './ws-reconnect';
import type { ReconnectController } from './ws-reconnect';

/**
 * 面板的「已截断」阈值：与后端落库的默认裁剪一致（256 KiB）。
 *
 * 它只喂面板徽标，不切正文 —— 历史正文是未截断的原文，裁剪由服务端按
 * `maxResponseSizeKb` 统一做。
 */
const PANEL_BODY_LIMIT_BYTES = 256 * 1024;

/** 放弃一条 SSE 流之后，等它自己收尾的上限；超过就无条件摘掉注册表里那一格。 */
const DISPOSE_SETTLE_MS = 5000;

interface SseEntry {
  kind: 'sse';
  handle: HttpStreamHandle;
  request: PreparedRequest;
  /** 已存接口的 id；草稿没有落库的行，故没有历史可写。 */
  endpointId: string | undefined;
  /** 收到过 meta 才写历史：建连就失败的那次没有任何响应可记。 */
  meta: SseMetaEvent | null;
  /** 原始正文，按到达顺序拼接，结束时截到 256 KiB。 */
  text: string;
  seq: number;
  /**
   * 调用方放弃了这条流（切协议、关标签、重新发送）：取消之后后端仍会回一条
   * `end`（`reason: 'stopped'`），那一条只用来把这个条目从注册表里摘掉，不再写
   * store 与历史 —— 视图那一格已经由 `dropTabStreams` 清掉了。
   */
  abandoned: boolean;
}

interface WsEntry {
  kind: 'ws';
  args: WsConnectArgs;
  handle: WsSessionHandle | null;
  /** 用户主动断开：不再自动重连。 */
  manual: boolean;
  disposed: boolean;
  reconnect: ReconnectController;
}

type SessionEntry = SseEntry | WsEntry;

const sessions = new Map<string, SessionEntry>();

/** 标签还在不在；关得太快时事件可能晚于清理到达，句柄是旧的那一份。 */
function isLive(tabId: string, entry: SessionEntry): boolean {
  return sessions.get(tabId) === entry;
}

function tuplesToRecord(tuples: Array<[string, string]>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [name, value] of tuples) {
    out[name] = name in out ? `${out[name]}, ${value}` : value;
  }
  return out;
}

/** 原文的 UTF-8 字节数是否越过面板显示上限（只判长度，不切字符串）。 */
function exceedsPanelBodyLimit(text: string): boolean {
  return new TextEncoder().encode(text).length > PANEL_BODY_LIMIT_BYTES;
}

/**
 * 每个标签一次建连的票号。取 OAuth2 token 是一次 await，期间用户可能又点了一次发送、切了
 * 协议或关了标签；票号对不上就说明这次建连已经作废，回来之后不再往这个标签上挂流。
 */
const startTickets = new Map<string, number>();

function takeStartTicket(tabId: string): number {
  const next = (startTickets.get(tabId) ?? 0) + 1;
  startTickets.set(tabId, next);
  return next;
}

function invalidatePendingStarts(tabId: string): void {
  startTickets.set(tabId, (startTickets.get(tabId) ?? 0) + 1);
}

function isCurrentStart(tabId: string, ticket: number): boolean {
  return startTickets.get(tabId) === ticket;
}

/**
 * 开始一条 SSE 流。已经在跑的旧流先被取消：同一个标签上「发送」两次，第二次就是重来。
 *
 * 建连之前先把 OAuth2 的 token 取好（其余鉴权已在 `prepareRequest` 里落进 header / URL）：
 * 不带它，用户配了 OAuth2 的端点在这里会静默丢掉凭据。
 */
export async function startSse(
  tabId: string,
  input: {
    request: PreparedRequest;
    settings: RequestSettings;
    tlsOverride: TlsSettings | null;
    endpointId: string | undefined;
    auth: AuthConfig | undefined;
  },
): Promise<void> {
  disposeSession(tabId);
  const ticket = takeStartTicket(tabId);
  const authed = await applyStreamAuth({ request: input.request, auth: input.auth });
  if (!isCurrentStart(tabId, ticket)) return;
  const store = useAppStore.getState();
  if ('error' in authed) {
    // 取 token 也是一次真实的网络调用；失败落在与建连错误同一格，面板不必区分是哪一步。
    store.beginSse(tabId, Date.now());
    store.sseError(tabId, { type: 'error', kind: 'network', message: authed.error });
    return;
  }
  const request = authed.request;
  const args: HttpStreamArgs = {
    streamId: uid(),
    method: request.method,
    url: request.url,
    headers: request.headers,
    body: request.body,
    bodyMode: request.bodyMode,
    timeoutMs: input.settings.timeoutMs,
    followRedirects: input.settings.followRedirects,
    maxResponseSizeKb: input.settings.maxResponseSizeKb,
    tls: input.settings.tls,
    tlsOverride: input.tlsOverride,
  };
  const parser = createSseParser();
  const startedAt = Date.now();
  const entry: SseEntry = {
    kind: 'sse',
    request,
    endpointId: input.endpointId,
    meta: null,
    text: '',
    seq: 0,
    abandoned: false,
    handle: { cancel: () => {} },
  };
  store.beginSse(tabId, startedAt);

  const appendEvents = (events: SseEvent[]) => {
    if (events.length === 0) return;
    const entries = events.map((e) => ({
      seq: entry.seq++,
      event: e.event,
      id: e.id,
      data: e.data,
    }));
    useAppStore.getState().sseEvents(tabId, entries, 0);
  };

  entry.handle = openHttpStream(args, {
    onMeta: (meta) => {
      if (!isLive(tabId, entry) || entry.abandoned) return;
      entry.meta = meta;
      useAppStore.getState().sseMeta(tabId, meta);
    },
    onChunk: (chunk) => {
      if (!isLive(tabId, entry) || entry.abandoned) return;
      entry.text += chunk.text;
      useAppStore.getState().sseEvents(tabId, [], chunk.bytes);
      appendEvents(parser.push(chunk.text));
    },
    onEnd: (end) => {
      if (!isLive(tabId, entry)) return;
      sessions.delete(tabId);
      if (entry.abandoned) return;
      appendEvents(parser.flush());
      // 面板与历史各有一道裁剪，别叠加：面板只按本地 256 KiB 标「已截断」，历史正文交给
      // 服务端按 `maxResponseSizeKb` 统一裁（`db::response_history` 超限时置
      // `bodyTruncated`），故写历史的是未截断的 `entry.text`。`truncated` 仍以服务端事件
      // 为准，再并上本地那一次 —— 面板的「已截断」不能因为历史走了另一条路而丢。
      useAppStore
        .getState()
        .sseEnd(tabId, { ...end, truncated: end.truncated || exceedsPanelBodyLimit(entry.text) });
      if (!entry.meta || entry.endpointId === undefined) return;
      const response: ApiResponse = {
        status: entry.meta.status,
        statusText: entry.meta.statusText,
        durationMs: end.durationMs,
        sizeBytes: end.sizeBytes,
        headers: tuplesToRecord(entry.meta.headers),
        // 写历史的是未截断的原文：服务端落库时按 `maxResponseSizeKb` 统一裁。
        body: entry.text,
        // 线上真实的那份类型（可能带 charset）优先：服务端没报才回落到事件流。
        contentType: entry.meta.contentType || 'text/event-stream',
        method: entry.request.method,
        url: entry.meta.url || entry.request.url,
        requestHeaders: entry.meta.requestHeaders,
      };
      const snapshot: StoredRequestSnapshot = toRequestSnapshot({
        ...entry.request,
        // 线上发出的是后端报回来的那一份请求头；它缺席时才回落到本地拼的。
        headers:
          entry.meta.requestHeaders.length > 0 ? entry.meta.requestHeaders : entry.request.headers,
      });
      useAppStore.getState().addResponseHistory(entry.endpointId, response, snapshot);
    },
    onError: (error) => {
      if (!isLive(tabId, entry)) return;
      sessions.delete(tabId);
      if (entry.abandoned) return;
      useAppStore.getState().sseError(tabId, error);
      // meta 之前就失败 = 一个响应都没收到；标 status 0 的历史会把一次失败的连接说成一次
      // 空响应，故这一次不写。
    },
  });
  sessions.set(tabId, entry);
}

/** 用户按停止：取消流，`end` 事件随后会带着 `reason: 'stopped'` 到达。 */
export function stopSse(tabId: string): void {
  const entry = sessions.get(tabId);
  if (!entry || entry.kind !== 'sse' || entry.abandoned) return;
  // 只取消，**不删**注册表里这一条：后端随后回的 `end`（`reason: 'stopped'`）要靠
  // `isLive` 为真才写得了 `sseEnd` 与响应历史，删除在 `onEnd` 里做。
  entry.handle.cancel();
}

/**
 * 开始一条 WebSocket 会话。会自动重连（面板开关打开时），故这里建一个重连调度器，
 * 每次尝试都复用同一个会话条目与同一个 sessionId 之外的连接。
 */
export async function startWs(
  tabId: string,
  args: {
    /** 变量替换之后的请求：`url` / `headers` 原样当握手参数用。 */
    request: PreparedRequest;
    protocols: string[];
    tls: TlsSettings;
    tlsOverride: TlsSettings | null;
    auth: AuthConfig | undefined;
  },
): Promise<void> {
  disposeSession(tabId);
  const ticket = takeStartTicket(tabId);
  const authed = await applyStreamAuth({ request: args.request, auth: args.auth });
  if (!isCurrentStart(tabId, ticket)) return;
  const store = useAppStore.getState();
  if ('error' in authed) {
    store.beginWs(tabId);
    store.wsError(tabId, { type: 'error', kind: 'network', message: authed.error });
    return;
  }
  const request = authed.request;
  store.beginWs(tabId);
  store.wsState(tabId, 'connecting');
  const entry: WsEntry = {
    kind: 'ws',
    // TLS 两项原样带上：合成规则在后端（全局在前，端点覆盖整体取代它），重连复用同一份
    // `entry.args`，故每次尝试看到的都是用户保存的那套设置。
    args: {
      sessionId: uid(),
      url: request.url,
      headers: request.headers,
      protocols: args.protocols,
      tls: args.tls,
      tlsOverride: args.tlsOverride,
    },
    handle: null,
    manual: false,
    disposed: false,
    reconnect: createReconnect({
      timers: {
        setTimeout: (fn, ms) => window.setTimeout(fn, ms),
        clearTimeout: (h) => window.clearTimeout(h as number),
      },
      now: () => Date.now(),
      connect: () => {
        if (entry.disposed) return;
        entry.manual = false;
        connectWs(tabId, entry);
      },
      onSchedule: (nextAt, attempt) => {
        if (entry.disposed || !isLive(tabId, entry)) return;
        useAppStore.getState().wsRetry(tabId, attempt, nextAt);
      },
    }),
  };
  sessions.set(tabId, entry);
  connectWs(tabId, entry);
}

function connectWs(tabId: string, entry: WsEntry): void {
  if (entry.disposed) return;
  // 每次尝试都是新连接：旧句柄已经不在了，但 sessionId 换了，后端据此建新会话。
  entry.args.sessionId = uid();
  useAppStore.getState().wsConnecting(tabId);
  entry.handle = openWsSession(
    { ...entry.args },
    {
      onState: (event) => {
        if (entry.disposed || !isLive(tabId, entry)) return;
        if (event.state === 'open') entry.reconnect.onOpen();
        useAppStore.getState().wsState(tabId, event.state);
      },
      onFrame: (frame) => {
        if (entry.disposed || !isLive(tabId, entry)) return;
        useAppStore.getState().wsFrame(tabId, frame);
      },
      onClosed: (closed) => {
        if (entry.disposed || !isLive(tabId, entry)) return;
        useAppStore.getState().wsClosed(tabId, closed.code, closed.reason);
        scheduleReconnect(tabId, entry);
      },
      onError: (error) => {
        if (entry.disposed || !isLive(tabId, entry)) return;
        useAppStore.getState().wsError(tabId, error);
        // 建连就失败时后端可能只报 error 不报 closed；这里也排一次，重复调用由调度器的
        // 「已有待触发的重试」守卫吞掉。
        scheduleReconnect(tabId, entry);
      },
    },
  );
}

function scheduleReconnect(tabId: string, entry: WsEntry): void {
  if (entry.manual) return;
  if (!useAppStore.getState().lastStreams[tabId]?.ws?.reconnectEnabled) return;
  entry.reconnect.onClosed();
}

/** 面板上的自动重连开关；关闭中的会话打开开关会立刻再连一次。 */
export function setWsReconnect(tabId: string, enabled: boolean): void {
  useAppStore.getState().setWsReconnect(tabId, enabled);
  const entry = sessions.get(tabId);
  if (!entry || entry.kind !== 'ws') return;
  if (!enabled) {
    // 开关关掉只取消待触发的重试：之后重新打开，次数从零开始算。
    entry.reconnect.cancel();
    return;
  }
  const phase = useAppStore.getState().lastStreams[tabId]?.ws?.phase;
  if (phase === 'closed' || phase === 'error') {
    entry.manual = false;
    connectWs(tabId, entry);
  }
}

/** 面板发送一帧；文本帧传 `data`，二进制帧传 `base64`。 */
export async function sendWsFrame(
  tabId: string,
  kind: WsFrameEvent['kind'],
  data: string,
  base64?: string,
): Promise<void> {
  const entry = sessions.get(tabId);
  if (!entry || entry.kind !== 'ws' || !entry.handle) throw new Error('WebSocket 未连接');
  await entry.handle.send(kind, data, base64);
}

/** 面板的「断开」：主动断开不重连。`code` 缺省交给后端（正常关闭）。 */
export async function closeWs(tabId: string, code?: number, reason?: string): Promise<void> {
  const entry = sessions.get(tabId);
  if (!entry || entry.kind !== 'ws') return;
  entry.manual = true;
  entry.reconnect.dispose();
  await entry.handle?.close(code, reason);
}

/**
 * 关掉并丢掉这个标签的会话：关标签、切协议、重新建连前都走这里。传输层先停（不再有事件
 * 到达），再清 store 视图——顺序反过来会让清理后到达的事件把面板画回运行中。
 */
export function disposeSession(tabId: string): void {
  // 正在取 token 的那次建连也一并作废：它回来之后不该再往这个标签上挂流。
  invalidatePendingStarts(tabId);
  const entry = sessions.get(tabId);
  if (!entry) return;
  if (entry.kind === 'sse') {
    // 先标放弃、再取消，条目留在注册表里等后端回的 `end` 把它摘掉：取消之后提前删条目会
    // 让那一条 `end` 被 `isLive` 判成过期事件而丢弃。视图那一格由下面的
    // `dropTabStreams` 清空，故这段时间里面板不再显示这条流。
    entry.abandoned = true;
    entry.handle.cancel();
    // 收尾事件负责摘条目；但连接若在回 `end` 之前就断了（Web 形态的 `abort` 就是这样），
    // 那一格会连同已收到的正文一直留在注册表里。等一会儿还没人来摘就自己摘掉。
    window.setTimeout(() => {
      if (sessions.get(tabId) === entry && entry.abandoned) sessions.delete(tabId);
    }, DISPOSE_SETTLE_MS);
  } else {
    sessions.delete(tabId);
    entry.disposed = true;
    entry.manual = true;
    entry.reconnect.dispose();
    void entry.handle?.close();
  }
  useAppStore.getState().dropTabStreams(tabId);
}

// 标签集合一变就关掉不在其中的会话。store 不认识传输层，故订阅放在这一侧。
useAppStore.subscribe((state, prev) => {
  if (state.openTabs === prev.openTabs) return;
  const live = new Set(state.openTabs.map((t) => t.id));
  for (const tabId of [...sessions.keys()]) {
    if (!live.has(tabId)) disposeSession(tabId);
  }
});
