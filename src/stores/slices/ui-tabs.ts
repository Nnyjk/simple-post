import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import { SETTINGS_TAB_ID } from '../store-types';
import type {
  DraftEndpoint,
  EndpointDraft,
  OpenTab,
  ProjectTabSet,
  SettingsCategoryId,
  TabKind,
} from '../store-types';

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
  /**
   * Per-project tab strips, set aside for the duration of this app run
   * whenever the user works in a different project.
   *
   * The strip shows one project's tabs at a time; this is where the
   * others wait. Unsaved work survives a switch for free — see
   * {@link ProjectTabSet} for why nothing else has to be copied.
   */
  projectTabs: Record<string, ProjectTabSet>;
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
   * Persist unsaved edits for an endpoint. Passing an empty patch is a
   * no-op; pass an explicit `null` to clear the draft. Mirrors the dirty
   * flag automatically — an empty draft is clean, any non-empty draft is
   * dirty. Every request-config editor (URL bar, method picker, params,
   * headers, body, auth, docs, tags) routes through here.
   */
  setEndpointDraft: (endpointId: string, draft: Partial<EndpointDraft> | null) => void;
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
   * Make `projectId`'s tab set the visible one, setting the outgoing
   * project's aside in `projectTabs`.
   *
   * Called by `openProject` / `leaveProject` **before** the active project
   * changes, because deciding what to stash means asking "which project
   * owns each tab I am showing right now" — and that answer is only
   * meaningful against the project currently in front.
   *
   * Passing `''` (leaving for the welcome screen) stashes and empties the
   * strip. The settings tab is never stashed: it is a global singleton
   * panel, not something project A borrowed and project B must not see.
   */
  switchProjectTabs: (projectId: string) => void;
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
  projectTabs: {},

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
      // endpoint tab — otherwise the draft would linger forever. A draft
      // endpoint tab qualifies: its id is not in `nodes`, so this is the
      // only place the `endpointDrafts` entry can be reclaimed. Closing
      // the tab still writes nothing to the database; discarding the
      // `DraftEndpoint` itself is `discardDraftEndpoint`'s job.
      const closing = s.openTabs[idx];
      let endpointDrafts = s.endpointDrafts;
      let dirtyTabIds = s.dirtyTabIds;
      let draftEndpoints = s.draftEndpoints;
      if (closing && carriesEndpointDraft(closing.kind)) {
        if (s.endpointDrafts[closing.id]) {
          endpointDrafts = { ...s.endpointDrafts };
          delete endpointDrafts[closing.id];
        }
        if (s.dirtyTabIds.has(closing.id)) {
          dirtyTabIds = new Set(s.dirtyTabIds);
          dirtyTabIds.delete(closing.id);
        }
      }
      if (closing && closing.kind === 'endpoint-draft' && closing.id in draftEndpoints) {
        // A draft endpoint lives *only* in `draftEndpoints`, so closing its
        // tab is the single moment its row can be reclaimed. Dropping only
        // `endpointDrafts` (above) would leave the document fields behind
        // under an id nothing references any more — a draft that has
        // outlived its tab, can no longer be saved, and would resurrect if
        // that id were ever reused. Reclaimed in the same `set` rather than
        // via a follow-up `discardDraftEndpoint` call, so a failure cannot
        // leave the map and the tab disagreeing about each other.
        draftEndpoints = { ...draftEndpoints };
        delete draftEndpoints[closing.id];
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        endpointDrafts,
        dirtyTabIds,
        draftEndpoints,
      };
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
      const current = s.endpointDrafts[endpointId];
      let next: EndpointDraft | undefined;
      if (draft === null) {
        // Explicit clear.
        next = undefined;
      } else {
        // Merge patch with existing draft; if everything is undefined,
        // collapse to undefined so dirty tracking can flip to clean.
        const merged: EndpointDraft = { ...current, ...draft };
        // Generic emptiness test rather than a hand-written list: adding a
        // draft field must not also mean remembering to extend this line,
        // or the field would silently never mark the tab dirty.
        next = Object.values(merged).some((v) => v !== undefined) ? merged : undefined;
      }
      // No-op when the new draft matches the existing one. Returning
      // `{}` (instead of the raw `s` reference) keeps Zustand's shallow
      // equality stable — returning `s` is fine semantically but trips
      // the strict-mode ref-equality checks that some consumers rely on.
      if (next === undefined && !(endpointId in s.endpointDrafts)) return {};
      if (next !== undefined && sameDraft(current, next)) return {};
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
      // Layer 2 has not landed yet, so `s.nodes` is empty — which reads
      // exactly like "every tree node was deleted". Running the filter
      // now would close every module / collection / endpoint tab the
      // user has. Bail instead; the effect that calls this re-runs when
      // `nodesLoaded` flips, and that is the first moment the answer is
      // trustworthy.
      if (!s.nodesLoaded) return {};
      const valid = s.openTabs.filter((t) => {
        if (t.kind === 'settings') return true; // singleton — never stale
        // A draft endpoint has no `nodes` row by construction — that is
        // the whole point of it — so the lookup below would read "deleted"
        // and close the tab the instant this runs. It is exempt until the
        // user saves it (which opens a real `endpoint` tab) or discards it.
        if (t.kind === 'endpoint-draft') return true;
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
      // Also drop endpoint drafts whose endpoint is gone. A draft
      // endpoint's own id is not in `nodes`, so it is kept by tab kind
      // rather than by lookup — otherwise the edits would vanish from a
      // tab that is still open.
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      for (const t of valid) {
        if (carriesEndpointDraft(t.kind) && s.endpointDrafts[t.id]) {
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
      // Same reason as above, one level up: a closed draft tab is a draft
      // with no way to be saved, so "close everything" has to take the
      // documents with it.
      draftEndpoints: {},
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
      // Drop endpoint drafts for closed endpoint tabs. Draft-endpoint
      // tabs count: they are ordinary tabs, and the edits under their
      // id have to go when the tab does.
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      // …and their documents, for the same reason: a draft whose tab is
      // gone can never be saved. This sweep is reachable in practice —
      // a draft tab is marked dirty on creation precisely so this action
      // leaves it alone, but a draft can also become clean if the user
      // clears every edit back to the stored default.
      const nextDraftEndpoints: Record<string, DraftEndpoint> = {};
      for (const t of nextTabs) {
        if (carriesEndpointDraft(t.kind) && s.endpointDrafts[t.id]) {
          nextDrafts[t.id] = s.endpointDrafts[t.id];
        }
        if (t.kind === 'endpoint-draft' && s.draftEndpoints[t.id]) {
          nextDraftEndpoints[t.id] = s.draftEndpoints[t.id];
        }
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        endpointDrafts: nextDrafts,
        draftEndpoints: nextDraftEndpoints,
      };
    });
    return toClose.length;
  },

  switchProjectTabs: (projectId) =>
    set((s) => {
      const current = s.activeProjectId;
      // Re-entering the project already in front is not a switch. Without
      // this guard, opening the same project twice would stash its tabs
      // and then restore them from the cache — a no-op that still churns
      // every reference and can drop the active tab.
      if (current === projectId) return {};

      const ownerOf = (t: OpenTab) => tabOwningProject(t, s);
      // `current !== ''` is what keeps the settings singleton out of the
      // stash on the way to the welcome screen: with no project in front
      // its owner would otherwise match the empty string.
      const stashed = s.openTabs.filter((t) => current !== '' && ownerOf(t) === current);
      const staying = s.openTabs.filter((t) => !(current !== '' && ownerOf(t) === current));

      const projectTabs = { ...s.projectTabs };
      if (current !== '' && stashed.length > 0) {
        projectTabs[current] = {
          tabs: stashed,
          // Only remember an active tab we are actually stashing —
          // otherwise the restored strip would select a tab that was
          // already closed before the switch.
          activeTabId:
            s.activeTabId && stashed.some((t) => t.id === s.activeTabId) ? s.activeTabId : null,
        };
      }

      if (projectId === '') {
        // Leaving for the welcome screen: the strip goes empty, the
        // stashes stay put.
        return { projectTabs, openTabs: staying, activeTabId: null };
      }

      const remembered = projectTabs[projectId];
      const tabs = [...staying, ...(remembered?.tabs ?? [])];
      // Every project owes the strip one tab of its own — that is what
      // lets "which project am I in" be read off the strip at a glance.
      // A project with nothing cached gets one now; a returning one keeps
      // the tab it already had.
      if (!tabs.some((t) => t.kind === 'project' && t.id === projectId)) {
        tabs.push({ id: projectId, kind: 'project' });
      }
      // Coming back to a project puts the user where they left, not on
      // its project card — that would make switching feel like it forgot.
      const restored = remembered?.activeTabId ?? null;
      const activeTabId = restored && tabs.some((t) => t.id === restored) ? restored : projectId;

      return { projectTabs, openTabs: tabs, activeTabId };
    }),

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

/**
 * Does a tab of this kind own an entry in `endpointDrafts`?
 *
 * True for both `endpoint` and `endpoint-draft`, because a draft endpoint
 * is written through the *same* request-config editors as a saved one —
 * `endpointDrafts` is keyed by the tab id either way, and the two differ
 * only in whether `nodes` has a row behind them. Kept as one helper so the
 * three sweeps that rebuild that map cannot drift: each one that quietly
 * skipped the draft kind would drop a user's in-flight edits the first time
 * the tree refreshed.
 */
function carriesEndpointDraft(kind: TabKind): boolean {
  return kind === 'endpoint' || kind === 'endpoint-draft';
}

/**
 * Owner marker for a tab that belongs to no project.
 *
 * Distinct from `''`, which means "the welcome screen, no project open".
 * Collapsing the two would make the settings tab stashable: on the way out
 * its owner would compare equal to the empty project id and it would be
 * swept into a project cache, closing the panel the user had open.
 */
const GLOBAL_TAB_OWNER = '*';

/**
 * Which project owns a tab.
 *
 * Everything with an entity behind it resolves through the one key the
 * tree already denormalises onto every row: `rootId`. A draft endpoint
 * has no row to read it from, so it carries the same key on
 * `DraftEndpoint` — snapshotted at birth, which is good enough for
 * choosing a strip and cannot go stale for the reason that matters
 * (a draft whose parent moved still belongs where it was created).
 *
 * A node that cannot be found reports `''`. In practice `pruneStaleTabs`
 * retires such a tab first, so this is a transient answer at worst — and
 * treating "unknown" as "belongs to nobody" keeps a half-loaded tree from
 * dragging another project's tabs along with it.
 */
function tabOwningProject(tab: OpenTab, state: AppState): string {
  if (tab.kind === 'settings') return GLOBAL_TAB_OWNER;
  if (tab.kind === 'project') return tab.id;
  if (tab.kind === 'endpoint-draft') return state.draftEndpoints[tab.id]?.rootId ?? '';
  return state.nodes.find((n) => n.id === tab.id)?.rootId ?? '';
}

/**
 * Field-by-field draft equality.
 *
 * Reference comparison is the honest signal here: the editors hand us a
 * fresh array / object exactly when the user changed something, so a
 * structural walk would cost more than it buys on every keystroke. The
 * field list is read off `next` rather than hardcoded, for the same
 * reason the emptiness check is generic — a new draft field must not
 * require a second edit in this file to stop producing no-op churn.
 */
function sameDraft(a: EndpointDraft | undefined, b: EndpointDraft): boolean {
  if (!a) return false;
  return (Object.keys(b) as (keyof EndpointDraft)[]).every((k) => a[k] === b[k]);
}
