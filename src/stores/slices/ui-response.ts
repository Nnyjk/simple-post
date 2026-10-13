import type { StateCreator } from 'zustand';
import type { ApiResponse, ResponseHistoryEntry } from '@/types/domain';
import { uid } from '@/lib/utils';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from '../app-store';
import type { ApiResponseExample } from '../store-types';
import { DRAFT_ID_PREFIX } from '../store-types';

/**
 * Cap on how many responses we keep per endpoint in the store.
 *
 * The database enforces the same number on every append, so this is not
 * bounding the file — it is bounding what the history strip renders. Two
 * copies of one cap is not redundancy to remove: the server's protects
 * the store from *any* writer (a future MCP tool, a script), and this one
 * protects the UI from a read that returns more than the strip can show.
 */
const MAX_HISTORY_PER_ENDPOINT = 10;

/**
 * ui-response — the last HTTP response, the in-flight flag, and the
 * per-endpoint history / saved examples.
 *
 * Grouped because they are one lifecycle: a send sets `requestPending`,
 * resolves into `lastResponse`, and then gets appended to
 * `responseHistory`. Splitting the pending flag from the history would
 * put the three steps of a single user action in three files.
 *
 * `responseHistory` is now **persisted** (see `addResponseHistory` /
 * `loadResponseHistory` below), which is the one reason this sub-factory
 * reaches for the IPC layer at all; `responseExamples` is still
 * memory-only, and both are read by the CRUD delete actions
 * (`deleteNode` / `deleteProject` prune both maps in the same `set()` as
 * the node removal — the server's cascade already did the SQL half).
 * That cross-slice write is why this stays a sub-factory composed into
 * `ui-slice` rather than a slice of its own — see the note in
 * `../ui-slice.ts`.
 */

/** FIFO append under {@link MAX_HISTORY_PER_ENDPOINT}. */
function appendCapped(
  prev: ResponseHistoryEntry[],
  entry: ResponseHistoryEntry,
): ResponseHistoryEntry[] {
  return prev.length >= MAX_HISTORY_PER_ENDPOINT
    ? [...prev.slice(prev.length - (MAX_HISTORY_PER_ENDPOINT - 1)), entry]
    : [...prev, entry];
}

/** Group the server's flat, per-endpoint-ordered list into the store's map. */
function groupByEndpoint(rows: ResponseHistoryEntry[]): Record<string, ResponseHistoryEntry[]> {
  const grouped: Record<string, ResponseHistoryEntry[]> = {};
  for (const row of rows) {
    (grouped[row.endpointId] ??= []).push(row);
  }
  return grouped;
}

export interface UiResponseState {
  lastResponse: ApiResponse | null;
  requestPending: boolean;
  /**
   * Per-endpoint send history, newest-last. Backed by `response_history`
   * and reloaded per project — `loadResponseHistory` replaces the whole
   * map, which is why a draft endpoint's rows can be in here too: a
   * `draft:` id is never persisted, so those rows exist only for as long
   * as the tab does.
   */
  responseHistory: Record<string, ResponseHistoryEntry[]>;
  responseExamples: Record<string, ApiResponseExample[]>;
}

export interface UiResponseActions {
  /**
   * Record a send in the history strip, and persist it.
   *
   * Stays **synchronous** for the in-memory half: the Send button's UX
   * depends on the row appearing the moment `lastResponse` does, so the
   * provisional row is inserted first and the append is fire-and-forget
   * after it.
   */
  addResponseHistory: (endpointId: string, response: ApiResponse) => void;
  /**
   * Replace the map with one project's stored history. Fired from
   * `setActiveProject`, which is the single canonical "the user is now
   * working in project X" event. Never throws and never rejects — it
   * runs beside the bootstrap critical path, not on it.
   */
  loadResponseHistory: (rootId: string) => Promise<void>;
  /** Drop one endpoint's history locally and on the server. */
  clearResponseHistory: (endpointId: string) => Promise<void>;
  addResponseExample: (endpointId: string, response: ApiResponse) => string;
  deleteResponseExample: (endpointId: string, exampleId: string) => void;
  setLastResponse: (resp: ApiResponse | null) => void;
  setRequestPending: (pending: boolean) => void;
}

export type UiResponse = UiResponseState & UiResponseActions;

export const createUiResponse: StateCreator<AppState, [], [], UiResponse> = (set, get) => ({
  lastResponse: null,
  requestPending: false,
  responseHistory: {},
  responseExamples: {},

  addResponseHistory: (endpointId, response) => {
    // Provisional row, inserted before anything can await. Its id is
    // renderer-minted and clearly not a server id, so the swap below can
    // never collide with a stored row.
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

    // A draft endpoint exists only in the renderer — there is no `nodes`
    // row for the foreign key to hold the history against, so the server
    // rejects the append. Its history keeps accumulating in memory,
    // exactly as it did when nothing was persisted at all.
    if (endpointId.startsWith(DRAFT_ID_PREFIX)) return;

    void invokeCmd<ResponseHistoryEntry>(COMMANDS.responseHistoryAppend, { endpointId, response })
      .then((stored) => {
        // The returned row is the source of truth for what was stored:
        // the body may have been clipped to 256 KiB, and `bodyJson` is
        // dropped with it. Keeping the provisional row would leave the
        // strip claiming a clipped half-document is the whole response.
        set((s) => {
          const prev = s.responseHistory[endpointId];
          // Cleared (or re-capped) while the append was in flight — the
          // optimistic row is gone and there is nothing to replace.
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
        // Deliberately swallowed. The send already succeeded and the
        // user already has the response on screen; surfacing a failed
        // *history* write as a send failure would be a lie about what
        // went wrong. The optimistic row stays as the best answer we have.
        console.warn('[store] response history append failed', e);
      });
  },

  loadResponseHistory: async (rootId) => {
    // `''` is the welcome screen, not a project — there is no row to
    // filter by, and asking for it would return some other project's
    // history under no project at all.
    if (rootId === '') return;
    try {
      const rows = await invokeCmd<ResponseHistoryEntry[]>(COMMANDS.responseHistoryList, {
        rootId,
      });
      // Race guard. This is dispatched without being awaited, so the user
      // can switch A→B→A while A's load is still in flight; committing
      // unconditionally would land A's rows under whichever project
      // happens to be in front. A's own (later) load is what fills them.
      if (get().activeProjectId !== rootId) return;
      set({ responseHistory: groupByEndpoint(rows) });
    } catch (e) {
      // Fails soft on purpose: an unreadable history degrades to "the
      // strip is empty", which is the same as the pre-persistence state.
      // It must not reach `bootstrapError` — that flag is a dead
      // startup, and this is not one.
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
      // No restore: the rows are the server's to keep, and putting them
      // back in the strip would advertise history the file may no longer
      // have. The next project switch reloads whatever actually survived.
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
