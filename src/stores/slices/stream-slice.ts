/**
 * stream-slice —— **按标签存放**的流式会话视图：SSE 收到的事件，以及 WebSocket 会话的
 * 状态与帧。
 *
 * 与 `lastResponses` 同一条理由：这些是那个标签工作区里的一次运行，键就是标签 id。切标签
 * 不中断流（控制器活在 `features/http-tester/stream/stream-sessions.ts` 的注册表里），故
 * 事件与帧必须留在 store 上，重新挂载的面板读得到；关标签、切协议、删接口时由关闭动作
 * 连同注册表一起清掉。
 *
 * 本切片不碰 IPC：连接与取消都在 `stream-sessions.ts`，这里只存视图。
 */
import type { StateCreator } from 'zustand';
import type {
  SseEndEvent,
  SseMetaEvent,
  StreamErrorEvent,
  WsFrameEvent,
  WsStateEvent,
} from '@/lib/stream-transport';
import type { AppState } from '../app-store';

/** 一次 SSE 事件运行到哪一步。「connecting」覆盖建连与首包之前，「streaming」从 meta 到达起。 */
export type SsePhase = 'connecting' | 'streaming' | 'ended' | 'error';

/** 事件列表在内存里保留的条数；`eventCount` 照实累计，界面超出时提示只显示最近这些。 */
export const SSE_MAX_LISTED_EVENTS = 2_000;

/** 列表里的一条事件：`data` 保留原始多行文本，`event` 缺省为 `message`。 */
export interface SseEventEntry {
  seq: number;
  event: string;
  id: string;
  data: string;
}

export interface SseRunState {
  phase: SsePhase;
  /** 建连响应头；`null` = 还没收到。 */
  meta: SseMetaEvent | null;
  /** 最近的事件（最多 `SSE_MAX_LISTED_EVENTS` 条）。 */
  events: SseEventEntry[];
  /** 收到的事件总数，含已从列表滚出的那些。 */
  eventCount: number;
  /** 已收到的正文字节数，用于「流到多大」而不是「列表多大」。 */
  bytes: number;
  truncated: boolean;
  endReason: SseEndEvent['reason'] | null;
  error: StreamErrorEvent | null;
  startedAt: number;
  durationMs: number | null;
}

/** WebSocket 会话的相位；`closed` 带关闭码，`error` 是建连或协议层失败。 */
export type WsPhase = 'connecting' | 'open' | 'closed' | 'error';

/** 帧列表在内存里保留的条数；`frameCount` 照实累计。 */
export const WS_MAX_LISTED_FRAMES = 2_000;

export interface WsSessionView {
  phase: WsPhase;
  /** 最近收到的帧（最多 `WS_MAX_LISTED_FRAMES` 条）。 */
  frames: WsFrameEvent[];
  /** 这个会话收到的帧总数，含已从列表滚出的那些。 */
  frameCount: number;
  error: StreamErrorEvent | null;
  closeCode: number | null;
  closeReason: string;
  /** 面板上的自动重连开关；关标签时随状态一起丢。 */
  reconnectEnabled: boolean;
  /** 已安排的重连序号（从 1 起），设有待触发的重连时为 0。 */
  retryAttempt: number;
  /** 下一次重连的时刻（毫秒时间戳）；没有待触发的重连时为 `null`。 */
  retryAt: number | null;
  connectedAt: number | null;
}

export interface TabStreamState {
  sse: SseRunState | null;
  ws: WsSessionView | null;
}

export interface StreamActions {
  beginSse: (tabId: string, startedAt: number) => void;
  sseMeta: (tabId: string, meta: SseMetaEvent) => void;
  sseEvents: (tabId: string, entries: SseEventEntry[], bytes: number) => void;
  sseEnd: (tabId: string, end: SseEndEvent) => void;
  sseError: (tabId: string, error: StreamErrorEvent) => void;
  /** 丢掉这个标签的 SSE 视图（重新发送或协议切走）。 */
  clearSse: (tabId: string) => void;

  beginWs: (tabId: string) => void;
  wsState: (tabId: string, state: WsStateEvent['state']) => void;
  wsFrame: (tabId: string, frame: WsFrameEvent) => void;
  wsClosed: (tabId: string, code: number | null, reason: string) => void;
  wsError: (tabId: string, error: StreamErrorEvent) => void;
  /** 记下待触发的重连；`null` = 没有待触发的重连。 */
  wsRetry: (tabId: string, attempt: number, at: number | null) => void;
  setWsReconnect: (tabId: string, enabled: boolean) => void;
  clearWs: (tabId: string) => void;

  /** 关标签时把两份都带走。 */
  dropTabStreams: (tabId: string) => void;
}

export type StreamSlice = { lastStreams: Record<string, TabStreamState> } & StreamActions;

function writeTabStream(
  map: Record<string, TabStreamState>,
  tabId: string,
  patch: (prev: TabStreamState) => TabStreamState,
): Record<string, TabStreamState> {
  return { ...map, [tabId]: patch(map[tabId] ?? { sse: null, ws: null }) };
}

/** 空运行：建连一开始就把它放进 store，面板按它画「连接中」。 */
function emptySse(startedAt: number): SseRunState {
  return {
    phase: 'connecting',
    meta: null,
    events: [],
    eventCount: 0,
    bytes: 0,
    truncated: false,
    endReason: null,
    error: null,
    startedAt,
    durationMs: null,
  };
}

function emptyWs(): WsSessionView {
  return {
    phase: 'connecting',
    frames: [],
    frameCount: 0,
    error: null,
    closeCode: null,
    closeReason: '',
    reconnectEnabled: false,
    retryAttempt: 0,
    retryAt: null,
    connectedAt: null,
  };
}

export const createStreamSlice: StateCreator<AppState, [], [], StreamSlice> = (set) => ({
  lastStreams: {},

  beginSse: (tabId, startedAt) =>
    set((s) => ({
      lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
        ...prev,
        sse: emptySse(startedAt),
      })),
    })),

  sseMeta: (tabId, meta) =>
    set((s) => {
      const run = s.lastStreams[tabId]?.sse;
      if (!run) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          sse: { ...run, phase: 'streaming', meta },
        })),
      };
    }),

  sseEvents: (tabId, entries, bytes) =>
    set((s) => {
      const run = s.lastStreams[tabId]?.sse;
      if (!run || entries.length === 0) return {};
      const merged = [...run.events, ...entries];
      const listed =
        merged.length > SSE_MAX_LISTED_EVENTS ? merged.slice(-SSE_MAX_LISTED_EVENTS) : merged;
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          sse: {
            ...run,
            events: listed,
            eventCount: run.eventCount + entries.length,
            bytes: run.bytes + bytes,
          },
        })),
      };
    }),

  sseEnd: (tabId, end) =>
    set((s) => {
      const run = s.lastStreams[tabId]?.sse;
      if (!run) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          sse: {
            ...run,
            phase: 'ended',
            durationMs: end.durationMs,
            truncated: end.truncated,
            endReason: end.reason,
          },
        })),
      };
    }),

  sseError: (tabId, error) =>
    set((s) => {
      const run = s.lastStreams[tabId]?.sse;
      if (!run) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          sse: { ...run, phase: 'error', error },
        })),
      };
    }),

  clearSse: (tabId) =>
    set((s) => {
      if (!s.lastStreams[tabId]?.sse) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({ ...prev, sse: null })),
      };
    }),

  beginWs: (tabId) =>
    set((s) => ({
      lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
        ...prev,
        // 重连开关与已收到的帧跨连接保留：自动重连就是同一个面板再连一次。
        ws: {
          ...emptyWs(),
          reconnectEnabled: prev.ws?.reconnectEnabled ?? false,
          frames: prev.ws?.frames ?? [],
          frameCount: prev.ws?.frameCount ?? 0,
        },
      })),
    })),

  wsState: (tabId, state) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          ws:
            state === 'open'
              ? {
                  ...view,
                  phase: 'open',
                  connectedAt: Date.now(),
                  error: null,
                  closeCode: null,
                  closeReason: '',
                }
              : { ...view, phase: 'connecting' },
        })),
      };
    }),

  wsFrame: (tabId, frame) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => {
          const merged = [...view.frames, frame];
          const frames =
            merged.length > WS_MAX_LISTED_FRAMES ? merged.slice(-WS_MAX_LISTED_FRAMES) : merged;
          return { ...prev, ws: { ...view, frames, frameCount: view.frameCount + 1 } };
        }),
      };
    }),

  wsClosed: (tabId, code, reason) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          ws: { ...view, phase: 'closed', closeCode: code, closeReason: reason },
        })),
      };
    }),

  wsError: (tabId, error) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          ws: { ...view, phase: 'error', error },
        })),
      };
    }),

  wsRetry: (tabId, attempt, at) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          ws: { ...view, retryAttempt: attempt, retryAt: at },
        })),
      };
    }),

  setWsReconnect: (tabId, enabled) =>
    set((s) => {
      const view = s.lastStreams[tabId]?.ws;
      if (!view) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({
          ...prev,
          ws: { ...view, reconnectEnabled: enabled },
        })),
      };
    }),

  clearWs: (tabId) =>
    set((s) => {
      if (!s.lastStreams[tabId]?.ws) return {};
      return {
        lastStreams: writeTabStream(s.lastStreams, tabId, (prev) => ({ ...prev, ws: null })),
      };
    }),

  dropTabStreams: (tabId) =>
    set((s) => {
      if (!s.lastStreams[tabId]) return {};
      const next = { ...s.lastStreams };
      delete next[tabId];
      return { lastStreams: next };
    }),
});
