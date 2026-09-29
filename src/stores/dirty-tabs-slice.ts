/**
 * Dirty tabs slice — owns the queue that mediates "what should we do
 * about unsaved edits?" when a close path would otherwise drop a draft.
 *
 * State on this slice: only `pendingDirtyClose` (the queue itself).
 * The other inputs (`openTabs`, `dirtyTabIds`, `endpointDrafts`,
 * `activeTabId`) live in the UI slice — the queue is a tiny
 * orchestration layer over them.
 *
 * Action contract (unchanged from the previous monolithic store):
 *   - `partitionDirty(ids)`         → pure helper used by the other
 *                                     requestClose* actions.
 *   - `requestCloseTab(id)`         → single-tab dirty-guarded close.
 *   - `requestCloseOthers()`        → close all tabs except the active.
 *   - `requestCloseAllTabs()`       → close every open tab.
 *   - `saveDirtyTab(id)`            → commit any pending edits on the
 *                                     tab (endpoint: apply draft via
 *                                     `updateEndpointDetail` + clear draft).
 *   - `resolveDirtyClose(action)`   → drive the queue one step forward
 *                                     based on the dialog button.
 *
 * Like every other slice, this one only couples to the rest of the
 * store via the shared `AppState` type — it never imports another
 * slice module directly. Cross-slice actions (e.g. `closeTab`,
 * `updateEndpointDetail`, `clearEndpointDraft`) are called via `get().…`.
 */
import type { StateCreator } from 'zustand';
import type { EndpointDetail } from '@/types/domain';
import type { AppState } from './app-store';

/** State portion owned by the dirty tabs slice. */
export interface DirtyTabsSliceState {
  /**
   * Batch of dirty tab ids waiting for the user to decide what to do
   * about their unsaved changes before the close proceeds. Populated by
   * the `requestClose*` actions when a close path would otherwise drop
   * a draft. The DirtyCloseDialog component reads this and walks
   * through the queue one tab at a time.
   *
   * Semantics for the dialog buttons (see resolveDirtyClose):
   *   - "直接关闭" — close the current tab; abort the rest of the queue
   *     (the user's other dirty tabs stay open).
   *   - "保存"     — save the current tab, then advance to the next.
   *   - "全部保存" — save every remaining tab in the queue, then close
   *     them all.
   *
   * Non-endpoint tabs (project / module / collection) become "dirty"
   * only while an inline rename is in flight; closing the tab forces
   * the inline input to blur, which fires its own commit — so the
   * "save" path for those kinds is effectively "just close".
   */
  pendingDirtyClose: { tabIds: string[]; index: number } | null;
}

/** Action portion owned by the dirty tabs slice. */
export interface DirtyTabsSliceActions {
  /**
   * Dirty-guarded close variants. If the targeted tab is dirty, the
   * action populates `pendingDirtyClose` and lets the user decide
   * (discard / save / save-all) before any close happens. If nothing is
   * dirty, the close proceeds immediately.
   *
   * `requestCloseTab` is single-tab; `requestCloseOthers` is "close
   * every tab except the active one"; `requestCloseAllTabs` is
   * "close every tab". The single-tab form is what the tab X buttons
   * and the right-click "关闭" item use; the batch forms power the
   * "Close All" / "Close Others" affordances.
   */
  requestCloseTab: (tabId: string) => void;
  requestCloseOthers: () => void;
  requestCloseAllTabs: () => void;
  /**
   * Commit any pending edits on the given tab without closing it. For
   * endpoint tabs this means applying the URL/method draft through
   * `updateEndpointDetail` and clearing the draft. For other tabs the inline
   * edit (if any) auto-commits on its own when the tab unmounts, so
   * this is a no-op for them.
   */
  saveDirtyTab: (tabId: string) => void;
  /**
   * Move `pendingDirtyClose.index` forward, or clear the batch when
   * the end is reached. Used by the "保存" / "直接关闭" / "全部保存"
   * buttons in DirtyCloseDialog.
   */
  resolveDirtyClose: (action: 'discard' | 'save' | 'saveAll') => void;
  /**
   * Filter the given tab ids down to the ones currently flagged as
   * dirty. Internal helper shared by the request-close paths.
   */
  partitionDirty: (targetIds: string[]) => string[];
}

export type DirtyTabsSlice = DirtyTabsSliceState & DirtyTabsSliceActions;

export const createDirtyTabsSlice: StateCreator<AppState, [], [], DirtyTabsSlice> = (set, get) => ({
  // No pending close batch at boot. The DirtyCloseDialog component
  // only mounts when this becomes non-null.
  pendingDirtyClose: null,

  // ---------- dirty-guarded close paths ----------
  // The plain closeTab / closeAllTabs / closeSavedTabs above are still
  // exported because:
  //   - `closeTab` is the internal "actually close" primitive that the
  //     dialog and the delete* actions call.
  //   - `closeAllTabs` is convenient when we *know* there are no dirty
  //     tabs (e.g. a fresh data import wipes everything).
  // The WorkspaceTabs strip and the right-click menu instead use
  // `requestClose*`, which routes through the dirty guard.

  /**
   * Internal helper — given the list of tab ids a close action intends
   * to close, partition them into the "drop immediately" set (clean
   * tabs) and the "ask the user" set (dirty tabs). Returns the dirty
   * subset; the caller is responsible for closing the clean ones
   * via the existing closeTab primitive.
   */
  partitionDirty: (targetIds: string[]) => {
    const state = get();
    const dirtySet = state.dirtyTabIds;
    return targetIds.filter((id) => dirtySet.has(id));
  },

  requestCloseTab: (tabId) => {
    const state = get();
    if (!state.openTabs.some((t) => t.id === tabId)) return;
    if (state.dirtyTabIds.has(tabId)) {
      // Dirty — open the dialog for just this one. Non-dirty fast path
      // is irrelevant because there's nothing else to close.
      set({ pendingDirtyClose: { tabIds: [tabId], index: 0 } });
      return;
    }
    get().closeTab(tabId);
  },

  requestCloseOthers: () => {
    const state = get();
    const targetIds = state.openTabs.filter((t) => t.id !== state.activeTabId).map((t) => t.id);
    if (targetIds.length === 0) return;
    const dirty = get().partitionDirty(targetIds);
    // Close the clean ones right away; queue the dirty for the dialog.
    for (const id of targetIds) {
      if (!dirty.includes(id)) get().closeTab(id);
    }
    if (dirty.length > 0) {
      set({ pendingDirtyClose: { tabIds: dirty, index: 0 } });
    }
  },

  requestCloseAllTabs: () => {
    const state = get();
    const targetIds = state.openTabs.map((t) => t.id);
    if (targetIds.length === 0) return;
    const dirty = get().partitionDirty(targetIds);
    for (const id of targetIds) {
      if (!dirty.includes(id)) get().closeTab(id);
    }
    if (dirty.length > 0) {
      set({ pendingDirtyClose: { tabIds: dirty, index: 0 } });
    }
  },

  saveDirtyTab: (tabId) => {
    const state = get();
    const tab = state.openTabs.find((t) => t.id === tabId);
    if (!tab) return;
    if (tab.kind === 'endpoint' && state.endpointDrafts[tabId]) {
      // Apply the draft to the stored endpoint, then clear the draft.
      // Mirrors the body of HttpTester's `handleSave` so dialog "保存"
      // and button save produce the same persisted state.
      const draft = state.endpointDrafts[tabId]!;
      // Url and method live in `endpoint_details`, so the draft is
      // applied there — not to the node. A single `updateEndpoint` that
      // split across two tables is exactly what the split is meant to
      // avoid: renaming an endpoint must not rewrite request config.
      const patch: Partial<EndpointDetail> = {};
      if (draft.url !== undefined) patch.url = draft.url;
      if (draft.method !== undefined) patch.method = draft.method;
      if (Object.keys(patch).length > 0) state.updateEndpointDetail(tabId, patch);
      state.clearEndpointDraft(tabId);
      return;
    }
    // Project / module / collection / settings — dirty state comes from
    // an in-flight inline rename. Closing the tab (next call) will
    // unmount the editor and the blur will fire its own commit, so
    // there's nothing for us to do here.
  },

  resolveDirtyClose: (action) => {
    const state = get();
    const batch = state.pendingDirtyClose;
    if (!batch) return;
    const currentId = batch.tabIds[batch.index];
    if (!currentId) {
      set({ pendingDirtyClose: null });
      return;
    }
    if (action === 'saveAll') {
      // Commit every remaining tab in the queue, then close them all
      // and clear the batch. We use the un-guarded `closeTab` so it
      // doesn't re-enter the guard and queue itself again.
      for (let i = batch.index; i < batch.tabIds.length; i++) {
        state.saveDirtyTab(batch.tabIds[i]);
        state.closeTab(batch.tabIds[i]);
      }
      set({ pendingDirtyClose: null });
      return;
    }
    if (action === 'save') {
      state.saveDirtyTab(currentId);
      state.closeTab(currentId);
      const nextIndex = batch.index + 1;
      if (nextIndex >= batch.tabIds.length) {
        set({ pendingDirtyClose: null });
      } else {
        set({ pendingDirtyClose: { tabIds: batch.tabIds, index: nextIndex } });
      }
      return;
    }
    // action === 'discard' — close this tab without saving, and abort
    // the rest of the queue. The user's other dirty tabs stay open so
    // nothing else is lost without their say-so.
    state.closeTab(currentId);
    set({ pendingDirtyClose: null });
  },
});
