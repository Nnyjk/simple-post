import type { StateCreator } from 'zustand';
import type { ApiResponse } from '@/types/domain';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import type { ApiResponseExample } from '../store-types';

/** Cap on how many responses we keep per endpoint in-memory. */
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
 * `responseHistory` and `responseExamples` are also read by the CRUD
 * delete actions (`deleteNode` / `deleteProject` prune both maps in the
 * same `set()` as the node removal). That cross-slice write is why this
 * stays a sub-factory composed into `ui-slice` rather than a slice of
 * its own — see the note in `../ui-slice.ts`.
 */

export interface UiResponseState {
  lastResponse: ApiResponse | null;
  requestPending: boolean;
  /** Kept in memory only — nothing here is persisted. */
  responseHistory: Record<string, ApiResponse[]>;
  responseExamples: Record<string, ApiResponseExample[]>;
}

export interface UiResponseActions {
  addResponseHistory: (endpointId: string, response: ApiResponse) => void;
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

  addResponseHistory: (endpointId, response) =>
    set((s) => {
      const prev = s.responseHistory[endpointId] ?? [];
      // FIFO cap: drop the oldest entry when over the limit.
      const next =
        prev.length >= MAX_HISTORY_PER_ENDPOINT
          ? [...prev.slice(prev.length - (MAX_HISTORY_PER_ENDPOINT - 1)), response]
          : [...prev, response];
      return {
        responseHistory: { ...s.responseHistory, [endpointId]: next },
      };
    }),
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
