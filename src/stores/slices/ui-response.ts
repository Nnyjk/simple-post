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

/**
 * Fold a server read into what the strip is already showing.
 *
 * The server's array is the wrong shape on two counts, and a plain
 * assignment is wrong on a third.
 *
 * **Order.** `list_by_endpoint` answers `created_at DESC` because that is
 * what a "most recent first" *query* wants, but the store's invariant is
 * newest-**last** — {@link appendCapped} pushes onto the end and drops the
 * front, and the strip reverses at render time. Storing the reply verbatim
 * turned both of those upside down: a reopened endpoint painted
 * oldest-first, and the next send then dropped the *newest* row from the
 * cache while SQLite trimmed the *oldest* one, leaving the strip and the
 * table disagreeing about which ten rows exist.
 *
 * **Union, not replace.** The reply is a snapshot from before the user
 * pressed Send, so it is blind to rows that arrived since — the
 * provisional row, or the stored one an append has already swapped in.
 * The window is real on a first open, where the panel's effect fires the
 * read and the user can send before it lands; and once the optimistic row
 * is gone, `addResponseHistory`'s swap finds nothing to replace and the
 * response sits in the database, invisible until the component remounts.
 *
 * Deduped by id, so a read that already contains a locally-appended row
 * does not double it, then re-capped under the same FIFO the in-memory
 * path uses.
 */
function mergeLoaded(
  rows: ResponseHistoryEntry[],
  local: ResponseHistoryEntry[],
): ResponseHistoryEntry[] {
  const known = new Set(rows.map((r) => r.id));
  // Reverse before sorting, never instead of it: the sort is stable, so
  // this order is what survives for two rows stamped in the same
  // millisecond — precisely the tie `rowid DESC` broke on the way out.
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
   * Per-endpoint send history, **newest-last** — the order `appendCapped`
   * writes and {@link mergeLoaded} normalises a server read into. The
   * strip reverses at render time; nothing else may.
   *
   * Backed by `response_history` and loaded per endpoint, so a `draft:`
   * endpoint's rows can be in here too: such an id is never persisted, so
   * those rows exist only for as long as the tab does.
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
   * Read one endpoint's stored history into the map.
   *
   * **Per endpoint, on demand** — fired when an endpoint tab becomes
   * active, not once per project. The first cut loaded the whole
   * project's history when a project opened, which sized the read to the
   * project instead of to the panel: the strip shows at most ten rows for
   * the endpoint in front of the user, and has no use for the other
   * endpoints' rows until it opens them.
   *
   * Fails soft. An unreadable history degrades to an empty strip, which
   * is exactly the state the renderer was in before persistence existed.
   * It must not reach `bootstrapError` — that flag means a dead startup,
   * and a missing history panel is not one.
   */
  loadResponseHistory: (endpointId: string) => Promise<void>;
  /** Drop one endpoint's history locally and on the server. */
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

  loadResponseHistory: async (endpointId) => {
    // A draft endpoint never reaches the database, so there is nothing to
    // read. Its history stays in memory for as long as the tab does.
    if (endpointId === '' || endpointId.startsWith(DRAFT_ID_PREFIX)) return;
    try {
      const rows = await invokeCmd<ResponseHistoryEntry[]>(COMMANDS.responseHistoryList, {
        endpointId,
      });
      // Merged, not assigned — see `mergeLoaded` for why a read that
      // arrives after a send must not erase that send.
      //
      // No *project*-level guard, and that is a consequence of the
      // per-endpoint scope rather than a lucky omission: this touches
      // only `responseHistory[endpointId]`, so an answer that lands after
      // the user moved on is still that endpoint's own rows. The
      // project-scoped version this replaced had to compare
      // `activeProjectId` before committing, because one late answer
      // could overwrite a whole other project's map.
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
      // No restore: the rows are the server's to keep, and putting them
      // back in the strip would advertise history the file may no longer
      // have. `ResponseHistory`'s effect re-reads whatever survived the
      // next time this endpoint is opened, because the clear dropped the
      // key it guards on.
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
