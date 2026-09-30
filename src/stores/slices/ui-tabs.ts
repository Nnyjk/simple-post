import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import { SETTINGS_TAB_ID } from '../store-types';
import type { EndpointDraft, OpenTab, SettingsCategoryId, TabKind } from '../store-types';

/**
 * ui-tabs — the workspace tab strip and the per-endpoint edit drafts.
 *
 * WHY THIS IS ONE CONCERN
 * -----------------------
 * `endpointDrafts` looks like a separate feature from `openTabs`, but the
 * two are the same feature seen from two sides: a draft is *why* a tab is
 * dirty, and the dirty dot is *how* a tab reports having one. Every
 * action here that touches `openTabs` also maintains `dirtyTabIds`, and
 * every action that touches `endpointDrafts` mirrors into
 * `dirtyTabIds`. Splitting them across files would scatter one invariant
 * across two modules — which is the failure mode the sub-factory split
 * is supposed to prevent, not create.
 *
 * `dirty-tabs-slice` is a different thing: it owns the *close* guard
 * (the queue that asks the user what to do with unsaved changes), not
 * the flag itself. It reads `dirtyTabIds` through the shared `AppState`.
 *
 * The whole group is spread by concern, not by slice, because several
 * actions below write state owned elsewhere (`activeTabId` transitions
 * must be atomic with the tab list they refer to). See the note in
 * `../ui-slice.ts`.
 */

export interface UiTabsState {
  /**
   * Open tabs in the right-pane workspace. A tab is identified by the
   * underlying entity id, so duplicate `openTab` calls for the same entity
   * just activate the existing tab rather than creating a new one.
   */
  openTabs: OpenTab[];
  activeTabId: string | null;
  /**
   * Tab ids the caller has flagged as "dirty" (have unsaved changes). The
   * WorkspaceTabs strip reads this to draw a "•" indicator and the
   * "Close Saved" right-click action uses it to decide which tabs to keep.
   *
   * Source of dirtiness:
   *   - HttpTester pushes the endpoint's id into this set whenever
   *     `endpointDrafts[id]` is non-empty.
   *   - EntitySettings increments a counter per in-flight
   *     `InlineEditableName` (title or sub-list rename) and calls
   *     `markTabDirty` on its parent tab.
   */
  dirtyTabIds: Set<string>;
  /**
   * Per-endpoint unsaved URL/method drafts. Lives in the store (not
   * HttpTester local state) so that switching tabs and switching back
   * preserves the user's pending edits — without this, a typed URL on
   * endpoint A would vanish the moment the user clicked endpoint B in
   * the tab strip. A tab id appearing as a key here is also what makes
   * WorkspaceTabs treat it as dirty.
   */
  endpointDrafts: Record<string, EndpointDraft | undefined>;
  /**
   * Active section inside the settings tab (left rail selection).
   * Persisted in the store so a deep link from the TopBar ("新建环境"
   * in the env switcher) can land directly on the right section even
   * if the settings tab wasn't open yet.
   */
  activeSettingsSection: SettingsCategoryId;
}

export interface UiTabsActions {
  /**
   * Open a tab for the given entity (or activate the existing one). If no
   * tabs are open yet, the new tab becomes active automatically.
   */
  openTab: (kind: TabKind, refId: string) => void;
  /** Close a tab. If it was active, falls back to the previous tab or null. */
  closeTab: (tabId: string) => void;
  /** Switch which tab is shown in the right pane. No-op for unknown ids. */
  setActiveTab: (tabId: string | null) => void;
  /**
   * Flag a tab as having unsaved changes (`true`) or not (`false`). Drives
   * the dirty indicator on the tab strip and the "Close Saved" action.
   */
  markTabDirty: (tabId: string, dirty: boolean) => void;
  /**
   * Persist a URL/method draft for an endpoint. Passing an empty patch
   * is a no-op; pass an explicit `null` to clear the draft. Mirrors
   * the dirty flag automatically — an empty draft is clean, any
   * non-empty draft is dirty.
   */
  setEndpointDraft: (
    endpointId: string,
    draft: Partial<Pick<EndpointDraft, 'url' | 'method'>> | null,
  ) => void;
  /** Drop the draft for an endpoint (called on Save). */
  clearEndpointDraft: (endpointId: string) => void;
  /**
   * Internal: drop every tab whose underlying entity no longer exists (or
   * whose ancestry was removed). Called from the delete* actions so the UI
   * never lands on a dangling tab. Exposed as an action for the cleanup
   * useEffect in WorkspaceTabs to recover from any missed edges.
   */
  pruneStaleTabs: () => void;
  /** Close every open tab in one go. Used by the right-click menu. */
  closeAllTabs: () => void;
  /**
   * Close every tab that is **not** in `dirtyTabIds` (i.e. every "saved"
   * tab). If a dirty tab is the only one open, it stays. Returns the
   * number of tabs closed — callers can use it for a toast if needed.
   */
  closeSavedTabs: () => number;
  /**
   * Open the (singleton) settings tab and select a section. Idempotent
   * — calling it when the tab is already open just switches the
   * active section and makes the tab active.
   */
  openSettingsTab: (section?: SettingsCategoryId) => void;
  /** Update which section of the settings tab is active. */
  setActiveSettingsSection: (section: SettingsCategoryId) => void;
}

export type UiTabs = UiTabsState & UiTabsActions;

export const createUiTabs: StateCreator<AppState, [], [], UiTabs> = (set, get) => ({
  // Workspace starts with no tabs — the bootstrap sequence activates the
  // first endpoint (if any) once the daemon returns data.
  openTabs: [],
  activeTabId: null,
  // Seeded empty — the first endpoint has nothing unsaved, so the strip
  // should not show a dirty dot on it. Children push into this set as
  // drafts are created (e.g. URL bar edits, inline title rename).
  dirtyTabIds: new Set<string>(),
  // Empty at boot. HttpTester writes into this map on every URL/method
  // edit; it stays populated across tab switches so a draft typed
  // for endpoint A is still there when the user comes back to A.
  endpointDrafts: {},
  // Settings tab defaults to "环境" — that's where most users expect to
  // land when they hit the settings button in the top bar.
  activeSettingsSection: 'env',

  openTab: (kind, refId) =>
    set((s) => {
      if (s.openTabs.some((t) => t.kind === kind && t.id === refId)) {
        return { activeTabId: refId };
      }
      return {
        openTabs: [...s.openTabs, { id: refId, kind }],
        activeTabId: refId,
      };
    }),
  closeTab: (tabId) =>
    set((s) => {
      const idx = s.openTabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return {};
      const nextTabs = s.openTabs.filter((t) => t.id !== tabId);
      let nextActive = s.activeTabId;
      if (s.activeTabId === tabId) {
        // Pick the previous tab, then next, then null. Mirrors what most IDE
        // tab strips do (Chrome, VS Code).
        const fallback = s.openTabs[idx - 1] ?? s.openTabs[idx + 1] ?? null;
        nextActive = fallback ? fallback.id : null;
      }
      // Drop the endpoint's draft + dirty flag if we're closing an
      // endpoint tab — otherwise the draft would linger forever.
      const closing = s.openTabs[idx];
      let endpointDrafts = s.endpointDrafts;
      let dirtyTabIds = s.dirtyTabIds;
      if (closing?.kind === 'endpoint') {
        if (s.endpointDrafts[closing.id]) {
          endpointDrafts = { ...s.endpointDrafts };
          delete endpointDrafts[closing.id];
        }
        if (s.dirtyTabIds.has(closing.id)) {
          dirtyTabIds = new Set(s.dirtyTabIds);
          dirtyTabIds.delete(closing.id);
        }
      }
      return { openTabs: nextTabs, activeTabId: nextActive, endpointDrafts, dirtyTabIds };
    }),
  setActiveTab: (tabId) =>
    set((s) => {
      if (tabId === null) {
        return { activeTabId: null };
      }
      // Reject unknown ids — don't let the UI land on a dangling tab.
      if (!s.openTabs.some((t) => t.id === tabId)) return {};
      return { activeTabId: tabId };
    }),
  markTabDirty: (tabId, dirty) =>
    set((s) => {
      const isDirty = s.dirtyTabIds.has(tabId);
      if (dirty && !isDirty) {
        const next = new Set(s.dirtyTabIds);
        next.add(tabId);
        return { dirtyTabIds: next };
      }
      if (!dirty && isDirty) {
        const next = new Set(s.dirtyTabIds);
        next.delete(tabId);
        return { dirtyTabIds: next };
      }
      return {};
    }),
  setEndpointDraft: (endpointId, draft) =>
    set((s) => {
      const current = s.endpointDrafts[endpointId] ?? {};
      let next: EndpointDraft | undefined;
      if (draft === null) {
        // Explicit clear.
        next = undefined;
      } else {
        // Merge patch with existing draft; if everything is undefined,
        // collapse to undefined so dirty tracking can flip to clean.
        const merged: EndpointDraft = { ...current, ...draft };
        const hasAny = merged.url !== undefined || merged.method !== undefined;
        next = hasAny ? merged : undefined;
      }
      // No-op when the new draft matches the existing one. Returning
      // `{}` (instead of the raw `s` reference) keeps Zustand's shallow
      // equality stable — returning `s` is fine semantically but trips
      // the strict-mode ref-equality checks that some consumers rely on.
      if (next === undefined && !(endpointId in s.endpointDrafts)) return {};
      if (
        next !== undefined &&
        s.endpointDrafts[endpointId]?.url === next.url &&
        s.endpointDrafts[endpointId]?.method === next.method
      ) {
        return {};
      }
      // Mirror into dirtyTabIds so the tab strip + "Close Saved" see it.
      const nextDirty = new Set(s.dirtyTabIds);
      if (next) nextDirty.add(endpointId);
      else nextDirty.delete(endpointId);
      // Build the new endpointDrafts map immutably.
      const nextDrafts: Record<string, EndpointDraft | undefined> = { ...s.endpointDrafts };
      if (next) nextDrafts[endpointId] = next;
      else delete nextDrafts[endpointId];
      return { endpointDrafts: nextDrafts, dirtyTabIds: nextDirty };
    }),
  clearEndpointDraft: (endpointId) =>
    set((s) => {
      if (!(endpointId in s.endpointDrafts)) return {};
      const nextDrafts: Record<string, EndpointDraft | undefined> = { ...s.endpointDrafts };
      delete nextDrafts[endpointId];
      const nextDirty = new Set(s.dirtyTabIds);
      nextDirty.delete(endpointId);
      return { endpointDrafts: nextDrafts, dirtyTabIds: nextDirty };
    }),
  pruneStaleTabs: () =>
    set((s) => {
      const valid = s.openTabs.filter((t) => {
        if (t.kind === 'settings') return true; // singleton — never stale
        if (t.kind === 'project') return s.projects.some((p) => p.id === t.id);
        // Every other tab is a tree node, and the tab's `kind` is the
        // node's `type` — so one lookup answers module, collection and
        // endpoint at once. Checking the type too means a tab can never
        // survive because some *other* node happens to share its id.
        return s.nodes.some((n) => n.id === t.id && n.kind === t.kind);
      });
      if (valid.length === s.openTabs.length) return {};
      let nextActive = s.activeTabId;
      if (nextActive !== null && !valid.some((t) => t.id === nextActive)) {
        nextActive = valid[valid.length - 1]?.id ?? null;
      }
      // Also drop dirty flags for tabs that no longer exist — prevents the
      // set from growing unbounded across deletes.
      const nextDirty = new Set<string>();
      for (const t of valid) {
        if (s.dirtyTabIds.has(t.id)) nextDirty.add(t.id);
      }
      // Also drop endpoint drafts whose endpoint is gone.
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      for (const t of valid) {
        if (t.kind === 'endpoint' && s.endpointDrafts[t.id]) {
          nextDrafts[t.id] = s.endpointDrafts[t.id];
        }
      }
      return {
        openTabs: valid,
        activeTabId: nextActive,
        dirtyTabIds: nextDirty,
        endpointDrafts: nextDrafts,
      };
    }),
  closeAllTabs: () =>
    set({
      openTabs: [],
      activeTabId: null,
      dirtyTabIds: new Set(),
      endpointDrafts: {},
    }),
  closeSavedTabs: () => {
    const state = get();
    const toClose = state.openTabs.filter((t) => !state.dirtyTabIds.has(t.id));
    if (toClose.length === 0) return 0;
    set((s) => {
      const closeSet = new Set(toClose.map((t) => t.id));
      const nextTabs = s.openTabs.filter((t) => !closeSet.has(t.id));
      let nextActive = s.activeTabId;
      if (nextActive !== null && closeSet.has(nextActive)) {
        nextActive = nextTabs[nextTabs.length - 1]?.id ?? null;
      }
      // Drop endpoint drafts for closed endpoint tabs.
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      for (const t of nextTabs) {
        if (t.kind === 'endpoint' && s.endpointDrafts[t.id]) {
          nextDrafts[t.id] = s.endpointDrafts[t.id];
        }
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        endpointDrafts: nextDrafts,
      };
    });
    return toClose.length;
  },

  openSettingsTab: (section) => {
    const state = get();
    if (section) set({ activeSettingsSection: section });
    const existing = state.openTabs.find((t) => t.kind === 'settings' && t.id === SETTINGS_TAB_ID);
    if (existing) {
      set({ activeTabId: SETTINGS_TAB_ID });
      return;
    }
    set({
      openTabs: [...state.openTabs, { id: SETTINGS_TAB_ID, kind: 'settings' }],
      activeTabId: SETTINGS_TAB_ID,
    });
  },

  setActiveSettingsSection: (section) => set({ activeSettingsSection: section }),
});
