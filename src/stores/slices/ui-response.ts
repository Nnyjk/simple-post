import type { StateCreator } from 'zustand';
import type {
  ApiResponse,
  ResponseHistoryEntry,
  ScriptOutcome,
  StoredRequestSnapshot,
} from '@/types/domain';
import { uid } from '@/lib/utils';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import type { ApiResponseExample } from '../store-types';
import { DRAFT_ID_PREFIX } from '../store-types';
import { capUnfavorited } from './history-keep';

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

/** FIFO 追加，受保留条数截断。 */
function appendCapped(
  prev: ResponseHistoryEntry[],
  entry: ResponseHistoryEntry,
  keep: number,
): ResponseHistoryEntry[] {
  return capUnfavorited([...prev, entry], keep);
}

/**
 * 把服务端读回的行并进历史条：按 id 去重、补上本地已有但回包里没有的行，
 * 再按内存路径同一套截断。
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
  keep: number,
): ResponseHistoryEntry[] {
  const known = new Set(rows.map((r) => r.id));
  // 先反转再排序，不能只反转：sort 稳定，同一毫秒打戳的两行靠这个次序分
  // 高下——正是服务端 `rowid DESC` 打破的那个平局。
  const merged = rows.slice().reverse();
  for (const row of local) {
    if (!known.has(row.id)) merged.push(row);
  }
  return capUnfavorited(
    merged.sort((a, b) => a.createdAt - b.createdAt),
    keep,
  );
}

export interface UiResponseState {
  lastResponse: ApiResponse | null;
  /**
   * 最近一次显示的响应对应的**请求**。
   *
   * 与 `lastResponse` 同一刻写入，故两者描述同一次调用。它是解析之后的那一份：
   * base URL 与路径已拼接、`{{var}}` 已替换、查询串已追加。
   *
   * 旧的历史行没有它，故可以是 `null`——界面据此不画请求侧栏。
   */
  lastRequest: StoredRequestSnapshot | null;
  /**
   * 最近一次发送的脚本产出（断言 / 日志 / 错误 / 写回的环境变量）。
   *
   * 与 `lastResponse` 同一刻写入，故两者永远描述同一次发送。没有脚本时为 null。
   */
  lastScriptOutcome: ScriptOutcome | null;
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
   *
   * `request` 是那次发送出去的请求；它缺席时行里就没有请求侧栏可画。
   */
  addResponseHistory: (
    endpointId: string,
    response: ApiResponse,
    request?: StoredRequestSnapshot,
  ) => void;
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
  /**
   * 删掉单个 endpoint 的**未收藏**历史，本地与服务端都删。
   *
   * 收藏的行留着：用户说它们永久保存，故「清空」这个动作只清自动保留的那部分。
   */
  clearResponseHistory: (endpointId: string) => Promise<void>;
  /**
   * 收藏或取消收藏一条历史行。
   *
   * 本地先改（星标立刻变），落盘在其后。落盘失败时本地仍显示收藏状态：用户标出的
   * 那一行不该因为一次写失败就从界面上消失。
   */
  setResponseHistoryFavorite: (id: string, favorite: boolean) => void;
  /**
   * 按新的保留条数裁一遍本地的历史列表。
   *
   * 服务端改条数时已经裁过库里的行，故本地也要按同一条规则裁，否则界面会多显示
   * 库里已经没有的行。收藏的行两边都不动。
   */
  trimResponseHistory: (keep: number) => void;
  addResponseExample: (endpointId: string, response: ApiResponse) => string;
  deleteResponseExample: (endpointId: string, exampleId: string) => void;
  setLastResponse: (resp: ApiResponse | null, request?: StoredRequestSnapshot | null) => void;
  /** 记下最近一次发送的脚本产出。 */
  setLastScriptOutcome: (outcome: ScriptOutcome | null) => void;
  setRequestPending: (pending: boolean) => void;
}

export type UiResponse = UiResponseState & UiResponseActions;

export const createUiResponse: StateCreator<AppState, [], [], UiResponse> = (set) => ({
  lastResponse: null,
  lastRequest: null,
  lastScriptOutcome: null,
  requestPending: false,
  responseHistory: {},
  responseExamples: {},

  addResponseHistory: (endpointId, response, request) => {
    // 乐观行，先于任何 await 插入。id 由渲染侧生成，明显不是服务端 id，
    // 所以下面的替换不可能与已存行用同一个 id。
    const provisional: ResponseHistoryEntry = {
      id: `pending:${uid()}`,
      endpointId,
      response,
      bodyTruncated: false,
      createdAt: Date.now(),
      isFavorite: false,
      ...(request ? { request } : {}),
    };
    set((s) => {
      const prev = s.responseHistory[endpointId] ?? [];
      return {
        responseHistory: {
          ...s.responseHistory,
          [endpointId]: appendCapped(prev, provisional, s.responseHistoryKeep),
        },
      };
    });

    // draft endpoint 只存在于渲染侧，没有 `nodes` 行给外键挂历史，服务端会拒收；
    // 它的历史继续在内存里累积。
    if (endpointId.startsWith(DRAFT_ID_PREFIX)) return;

    void invokeCmd<ResponseHistoryEntry>(COMMANDS.responseHistoryAppend, {
      endpointId,
      response,
      request: request ?? null,
    })
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
          [endpointId]: mergeLoaded(
            rows,
            s.responseHistory[endpointId] ?? [],
            s.responseHistoryKeep,
          ),
        },
      }));
    } catch (e) {
      console.warn('[store] response history load failed', e);
    }
  },

  clearResponseHistory: async (endpointId) => {
    set((s) => {
      const prev = s.responseHistory[endpointId];
      if (!prev) return {};
      // 收藏的行留着：服务端那条 `clear` 也只删未收藏的那些，两边必须一致。
      const kept = prev.filter((e) => e.isFavorite);
      if (kept.length === prev.length) return {};
      const next = { ...s.responseHistory };
      if (kept.length === 0) delete next[endpointId];
      else next[endpointId] = kept;
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

  setResponseHistoryFavorite: (id, favorite) => {
    // 本地先改：星标立刻变，不等一次 IPC。
    set((s) => {
      const next = { ...s.responseHistory };
      let hit = false;
      for (const [endpointId, entries] of Object.entries(s.responseHistory)) {
        if (!entries.some((e) => e.id === id)) continue;
        hit = true;
        next[endpointId] = entries.map((e) => (e.id === id ? { ...e, isFavorite: favorite } : e));
      }
      if (!hit) return {};
      return { responseHistory: next };
    });
    void invokeCmd<boolean>(COMMANDS.responseHistorySetFavorite, { id, favorite }).catch(
      (e: unknown) => {
        // 收藏是用户显式标出的意图，一次写失败不该让它从界面上消失；本地仍显示
        // 收藏状态，库里那一份则等下一次落盘或重启后再说。
        console.warn('[store] response history favorite failed', e);
      },
    );
  },

  trimResponseHistory: (keep) => {
    set((s) => {
      const next: Record<string, ResponseHistoryEntry[]> = {};
      let changed = false;
      for (const [endpointId, entries] of Object.entries(s.responseHistory)) {
        const capped = capUnfavorited(entries, keep);
        next[endpointId] = capped;
        if (capped.length !== entries.length) changed = true;
      }
      if (!changed) return {};
      return { responseHistory: next };
    });
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

  // 换响应就换掉脚本产出：脚本产出**属于**那一次响应，回看一条历史响应时它并不存在
  // （历史表里不存脚本结果），留着上一次发送的产出只会让人把两件事看成一件。
  // 发送路径因此先 `setLastResponse` 再 `setLastScriptOutcome`。
  setLastResponse: (resp, request) =>
    set({ lastResponse: resp, lastRequest: request ?? null, lastScriptOutcome: null }),
  setLastScriptOutcome: (outcome) => set({ lastScriptOutcome: outcome }),
  setRequestPending: (pending) => set({ requestPending: pending }),
});
