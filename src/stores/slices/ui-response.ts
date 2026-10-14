import type { StateCreator } from 'zustand';
import type { ApiResponse, ResponseHistoryEntry } from '@/types/domain';
import { uid } from '@/lib/utils';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import type { ApiResponseExample } from '../store-types';
import { DRAFT_ID_PREFIX } from '../store-types';

/**
 * store 中每个 endpoint 保留的历史条数上限。
 *
 * 与数据库的上限各守一头，不是重复：服务端那个挡住任何写入方（未来的 MCP
 * 工具、脚本），这个挡住读回来的条数超出历史条能显示的量。
 */
const MAX_HISTORY_PER_ENDPOINT = 10;

/**
 * ui-response —— 最近一次响应、请求中标记、按 endpoint 的历史与已存示例。
 *
 * 合成一个子工厂：一次发送依次写 `requestPending` → `lastResponse` →
 * `responseHistory`，拆开就把一个用户动作的三步散进三个文件。
 *
 * `responseHistory` 落盘（见 `addResponseHistory` / `loadResponseHistory`），
 * 这是本子工厂唯一接触 IPC 的理由；`responseExamples` 仍只在内存里。
 * 两者都由 CRUD 的删除动作在移除节点的同一次 `set()` 里剪掉——服务端级联
 * 已经删掉了 SQL 那一半。正因这种跨 slice 写入，它才是组合进 `ui-slice`
 * 的子工厂而非独立 slice，见 `../ui-slice.ts`。
 */

/** FIFO 追加，受 {@link MAX_HISTORY_PER_ENDPOINT} 截断。 */
function appendCapped(
  prev: ResponseHistoryEntry[],
  entry: ResponseHistoryEntry,
): ResponseHistoryEntry[] {
  return prev.length >= MAX_HISTORY_PER_ENDPOINT
    ? [...prev.slice(prev.length - (MAX_HISTORY_PER_ENDPOINT - 1)), entry]
    : [...prev, entry];
}

/**
 * 把服务端读回的行并进历史条：按 id 去重、补上本地已有但回包里没有的行，
 * 再按内存路径同一套 FIFO 截断。
 *
 * **顺序。** `list_by_endpoint` 按 `created_at DESC` 回答，那是"最近在前"的
 * *查询*要的顺序；而 store 的约定是最新在**末尾**——`appendCapped` 追加到
 * 尾部、丢头部，渲染时才反转。照抄回包会把两处都弄反：重开的 endpoint 画成
 * 最旧在前，且下一次发送丢的是最新行、SQLite 截掉的是最旧行，
 * 历史条与表里对不上是哪十行。
 *
 * **取并集而非覆盖。** 回包是用户按下发送之前的快照，看不到此后到达的行
 * （乐观行，或已被 append 换入的存储行）。首次打开时面板的 effect 发起读取、
 * 用户可以先发出去，这个窗口是真的；乐观行一旦被清掉，`addResponseHistory`
 * 的替换就找不到目标，响应留在数据库里，直到组件重挂载才可见。
 */
function mergeLoaded(
  rows: ResponseHistoryEntry[],
  local: ResponseHistoryEntry[],
): ResponseHistoryEntry[] {
  const known = new Set(rows.map((r) => r.id));
  // 先反转再排序，不能只反转：sort 稳定，同一毫秒打戳的两行靠这个次序分
  // 高下——正是服务端 `rowid DESC` 打破的那个平局。
  const merged = rows.slice().reverse();
  for (const row of local) {
    if (!known.has(row.id)) merged.push(row);
  }
  return merged.sort((a, b) => a.createdAt - b.createdAt).slice(-MAX_HISTORY_PER_ENDPOINT);
}

export interface UiResponseState {
  lastResponse: ApiResponse | null;
  requestPending: boolean;
  /**
   * 每个 endpoint 的发送历史，**最新在末尾**——`appendCapped` 写这个次序，
   * {@link mergeLoaded} 把服务端读回的行归一到它。只在渲染时反转，别处不许反。
   *
   * 来自 `response_history`，按 endpoint 读取，所以 `draft:` 开头的 id 也会
   * 出现在这里：这种 id 从不落盘，这些行只随标签页存活。
   */
  responseHistory: Record<string, ResponseHistoryEntry[]>;
  responseExamples: Record<string, ApiResponseExample[]>;
}

export interface UiResponseActions {
  /**
   * 记一次发送到历史条并落盘。
   *
   * 内存那一半必须**同步**：发送按钮的体验依赖历史行与 `lastResponse` 同一刻
   * 出现，所以先插乐观行，落盘在其后 fire-and-forget。
   */
  addResponseHistory: (endpointId: string, response: ApiResponse) => void;
  /**
   * 读单个 endpoint 的历史进 map。
   *
   * **按需、按 endpoint**：在 endpoint 标签页变为活动时触发，而不是开项目时
   * 读一整份——历史条只显示当前这个 endpoint 的至多十行，别的 endpoint 的行
   * 在打开它们之前用不上。
   *
   * **失败软化。** 读不到就退化成空历史条，也就是持久化之前渲染器的状态；
   * 但不得落到 `bootstrapError`——那个 flag 表示启动已死，缺一个历史面板不算。
   */
  loadResponseHistory: (endpointId: string) => Promise<void>;
  /** 删掉单个 endpoint 的历史，本地与服务端都删。 */
  clearResponseHistory: (endpointId: string) => Promise<void>;
  addResponseExample: (endpointId: string, response: ApiResponse) => string;
  deleteResponseExample: (endpointId: string, exampleId: string) => void;
  setLastResponse: (resp: ApiResponse | null) => void;
  setRequestPending: (pending: boolean) => void;
}

export type UiResponse = UiResponseState & UiResponseActions;

export const createUiResponse: StateCreator<AppState, [], [], UiResponse> = (set) => ({
  lastResponse: null,
  requestPending: false,
  responseHistory: {},
  responseExamples: {},

  addResponseHistory: (endpointId, response) => {
    // 乐观行，先于任何 await 插入。id 由渲染侧生成，明显不是服务端 id，
    // 所以下面的替换不可能与已存行相撞。
    const provisional: ResponseHistoryEntry = {
      id: `pending:${uid()}`,
      endpointId,
      response,
      bodyTruncated: false,
      createdAt: Date.now(),
    };
    set((s) => {
      const prev = s.responseHistory[endpointId] ?? [];
      return {
        responseHistory: { ...s.responseHistory, [endpointId]: appendCapped(prev, provisional) },
      };
    });

    // draft endpoint 只存在于渲染侧，没有 `nodes` 行给外键挂历史，服务端会拒收；
    // 它的历史继续在内存里累积。
    if (endpointId.startsWith(DRAFT_ID_PREFIX)) return;

    void invokeCmd<ResponseHistoryEntry>(COMMANDS.responseHistoryAppend, { endpointId, response })
      .then((stored) => {
        // 返回的行才是实际落库的内容：body 可能被截到 256 KiB，`bodyJson` 一并
        // 丢弃。留着乐观行会让历史条把半份文档说成整个响应。
        set((s) => {
          const prev = s.responseHistory[endpointId];
          // 追加在途期间被清空（或已重新截断）：乐观行没了，没有可替换的目标。
          if (!prev?.some((e) => e.id === provisional.id)) return {};
          return {
            responseHistory: {
              ...s.responseHistory,
              [endpointId]: prev.map((e) => (e.id === provisional.id ? stored : e)),
            },
          };
        });
      })
      .catch((e: unknown) => {
        // 故意吞掉。发送已经成功、响应已经在屏上，把一次*历史*写入失败说成
        // 发送失败是对故障的错误描述；乐观行留着就是现有最好的答案。
        console.warn('[store] response history append failed', e);
      });
  },

  loadResponseHistory: async (endpointId) => {
    // draft endpoint 从不入库，没有可读的行；它的历史只随标签页存活。
    if (endpointId === '' || endpointId.startsWith(DRAFT_ID_PREFIX)) return;
    try {
      const rows = await invokeCmd<ResponseHistoryEntry[]>(COMMANDS.responseHistoryList, {
        endpointId,
      });
      // 并入而非赋值，理由见 `mergeLoaded`。
      //
      // 不需要项目级守卫，这是按 endpoint 的作用域带来的结果而非侥幸：
      // 这里只动 `responseHistory[endpointId]`，迟到的回包仍属于那个 endpoint
      // 自己的行。
      set((s) => ({
        responseHistory: {
          ...s.responseHistory,
          [endpointId]: mergeLoaded(rows, s.responseHistory[endpointId] ?? []),
        },
      }));
    } catch (e) {
      console.warn('[store] response history load failed', e);
    }
  },

  clearResponseHistory: async (endpointId) => {
    set((s) => {
      if (!(endpointId in s.responseHistory)) return {};
      const next = { ...s.responseHistory };
      delete next[endpointId];
      return { responseHistory: next };
    });
    if (endpointId.startsWith(DRAFT_ID_PREFIX)) return;
    try {
      await invokeCmd<number>(COMMANDS.responseHistoryClear, { endpointId });
    } catch (e) {
      // 不回滚：这些行归服务端管，把它们放回历史条等于宣称文件里可能已经不存在的
      // 历史。`ResponseHistory` 的 effect 会在下次打开该 endpoint 时重读幸存者，
      // 因为清空动作丢掉了它依赖的那个 key。
      console.warn('[store] response history clear failed', e);
    }
  },

  addResponseExample: (endpointId, response) => {
    const exampleId = uid();
    set((s) => {
      const prev = s.responseExamples[endpointId] ?? [];
      const example: ApiResponseExample = { ...response, id: exampleId };
      return {
        responseExamples: { ...s.responseExamples, [endpointId]: [...prev, example] },
      };
    });
    return exampleId;
  },
  deleteResponseExample: (endpointId, exampleId) =>
    set((s) => {
      const prev = s.responseExamples[endpointId];
      if (!prev) return {};
      const next = prev.filter((ex) => ex.id !== exampleId);
      if (next.length === prev.length) return {};
      return {
        responseExamples: { ...s.responseExamples, [endpointId]: next },
      };
    }),

  setLastResponse: (resp) => set({ lastResponse: resp }),
  setRequestPending: (pending) => set({ requestPending: pending }),
});
