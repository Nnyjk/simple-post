/**
 * UI slice — owns every piece of state that the renderer manipulates
 * but the Tauri daemon doesn't persist (workspace tabs, theme, request
 * settings, the new-item modal draft, response history & saved
 * examples, the last HTTP response, etc.).
 *
 * State distribution rationale:
 *   - Workspace tabs (`openTabs`, `activeTabId`, `dirtyTabIds`,
 *     `endpointDrafts`, `activeSettingsSection`) are tied to the
 *     user-facing right pane; dirty-tabs-slice only owns the queue
 *     that mediates how those tabs get closed.
 *   - Prefs (`theme`, `requestSettings`) are persisted to
 *     localStorage here (the slice's helpers `loadPrefs` /
 *     `savePrefs` are private to the file).
 *   - New-item modal (`newItem`, `lastCreatedId`) is pure UI state.
 *   - Response history & saved examples are kept in-memory only; the
 *     renderer-side cap (MAX_HISTORY_PER_ENDPOINT) lives here.
 *
 * Cross-slice writes: this slice frequently writes to state owned by
 * other slices (e.g. `submitNewItem` flips
 * `dirtyTabIds`/`lastResponse` from bootstrap-slice; `closeTab` clears
 * `endpointDrafts`/`dirtyTabIds` which are still in this slice; the
 * `lastResponse` reset is on this slice). That's fine because all
 * slices share the same Zustand backing store — they only couple via
 * the shared `AppState` type, never by direct module import.
 */
import type { StateCreator } from 'zustand';
import type { ApiResponse, ResourceNode } from '@/types/domain';
import { uid } from '@/lib/utils';
import type {
  Theme,
  RequestSettings,
  NewType,
  TabKind,
  SettingsCategoryId,
  OpenTab,
  EndpointDraft,
  NewItemDraft,
  ApiResponseExample,
} from './store-types';
import { SETTINGS_TAB_ID } from './store-types';
import type { AppState } from './app-store';

/** Cap on how many responses we keep per endpoint in-memory. */
const MAX_HISTORY_PER_ENDPOINT = 10;

const PREFS_KEY = 'simple-post:prefs';

/** Persisted user preferences — only the fields that survive reloads. */
interface PersistedPrefs {
  theme?: Theme;
  requestSettings?: Partial<RequestSettings>;
}

function loadPrefs(): PersistedPrefs {
  if (typeof window === 'undefined') return {};
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as PersistedPrefs;
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

function savePrefs(prefs: PersistedPrefs): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // localStorage may be disabled (private mode, quota); silently ignore.
  }
}

const initialPrefs = loadPrefs();
const initialTheme: Theme = initialPrefs.theme ?? 'dark';
const initialRequestSettings: RequestSettings = {
  timeoutMs: initialPrefs.requestSettings?.timeoutMs ?? 30000,
  followRedirects: initialPrefs.requestSettings?.followRedirects ?? true,
  maxResponseSizeKb: initialPrefs.requestSettings?.maxResponseSizeKb ?? 5120,
};

/** State portion owned by the UI slice. */
export interface UiSliceState {
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

  // request / response
  lastResponse: ApiResponse | null;
  requestPending: boolean;

  // preferences (theme + request settings)
  theme: Theme;
  requestSettings: RequestSettings;

  // response history / saved examples — keyed by endpointId
  responseHistory: Record<string, ApiResponse[]>;
  responseExamples: Record<string, ApiResponseExample[]>;

  // global "new item" modal state
  newItem: NewItemDraft;
  /**
   * Set by `submitNewItem` after a successful create. Consumers (e.g.
   * ProjectTree) watch this via useEffect to drive their own side-effects
   * (e.g. enter inline-rename mode for the freshly created row) and then
   * call `clearLastCreated()` to acknowledge.
   */
  lastCreatedId: string | null;
}

/** Action portion owned by the UI slice. */
export interface UiSliceActions {
  // actions: workspace tabs
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

  // actions: response history & examples
  addResponseHistory: (endpointId: string, response: ApiResponse) => void;
  addResponseExample: (endpointId: string, response: ApiResponse) => string;
  deleteResponseExample: (endpointId: string, exampleId: string) => void;

  // actions: ui
  setLastResponse: (resp: ApiResponse | null) => void;
  setRequestPending: (pending: boolean) => void;

  // actions: preferences
  setTheme: (t: Theme) => void;
  updateRequestSettings: (patch: Partial<RequestSettings>) => void;

  // actions: new-item modal
  openNewItem: (opts?: {
    type?: NewType;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  closeNewItem: () => void;
  setNewItemType: (type: NewType) => void;
  setNewItemName: (n: string) => void;
  setNewItemDescription: (d: string) => void;
  setNewItemBaseUrl: (u: string) => void;
  setNewItemColor: (c: string) => void;
  setNewItemParentModule: (id: string) => void;
  setNewItemParentCollection: (id: string) => void;
  /**
   * Commit the in-memory `newItem` draft by calling the corresponding
   * store action. Now async because the underlying add* calls hit the
   * Tauri daemon. The NewItemModal calls this fire-and-forget — it
   * doesn't need the id synchronously.
   */
  submitNewItem: () => Promise<string | null>;
  clearLastCreated: () => void;
}

export type UiSlice = UiSliceState & UiSliceActions;

export const createUiSlice: StateCreator<AppState, [], [], UiSlice> = (set, get) => ({
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

  lastResponse: null,
  requestPending: false,

  theme: initialTheme,
  requestSettings: initialRequestSettings,

  responseHistory: {},
  responseExamples: {},

  newItem: {
    open: false,
    type: null,
    parentModuleId: '',
    parentCollectionId: '',
    name: '',
    description: '',
    baseUrl: '',
    color: '#60a5fa',
  },
  lastCreatedId: null,

  // ---------- workspace tabs ----------

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

  // ---------- response history & examples ----------

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

  // ---------- ui ----------

  setLastResponse: (resp) => set({ lastResponse: resp }),
  setRequestPending: (pending) => set({ requestPending: pending }),

  // ---------- preferences (persisted) ----------

  setTheme: (t) => {
    set({ theme: t });
    const s = get();
    savePrefs({ theme: t, requestSettings: s.requestSettings });
  },
  updateRequestSettings: (patch) => {
    set((s) => ({ requestSettings: { ...s.requestSettings, ...patch } }));
    const s = get();
    savePrefs({ theme: s.theme, requestSettings: s.requestSettings });
  },

  // ---------- new-item modal ----------

  openNewItem: (opts) => {
    set({
      newItem: {
        open: true,
        type: opts?.type ?? null,
        parentModuleId: opts?.parentModuleId ?? '',
        parentCollectionId: opts?.parentCollectionId ?? '',
        name: '',
        description: '',
        baseUrl: '',
        color: '#60a5fa',
      },
    });
  },
  closeNewItem: () => {
    set({
      newItem: {
        open: false,
        type: null,
        parentModuleId: '',
        parentCollectionId: '',
        name: '',
        description: '',
        baseUrl: '',
        color: '#60a5fa',
      },
    });
  },
  setNewItemType: (type) => set((s) => ({ newItem: { ...s.newItem, type } })),
  setNewItemName: (n) => set((s) => ({ newItem: { ...s.newItem, name: n } })),
  setNewItemDescription: (d) => set((s) => ({ newItem: { ...s.newItem, description: d } })),
  setNewItemBaseUrl: (u) => set((s) => ({ newItem: { ...s.newItem, baseUrl: u } })),
  setNewItemColor: (c) => set((s) => ({ newItem: { ...s.newItem, color: c } })),
  setNewItemParentModule: (id) =>
    set((s) => ({
      // Resetting parentCollectionId when the module changes is safe for
      // both 'collection' and 'endpoint' types — the old collection no
      // longer belongs to the chosen module. The modal re-fills the
      // default from projectModules via its own useEffect if needed.
      newItem: { ...s.newItem, parentModuleId: id, parentCollectionId: '' },
    })),
  setNewItemParentCollection: (id) =>
    set((s) => ({ newItem: { ...s.newItem, parentCollectionId: id } })),
  submitNewItem: async () => {
    const state = get();
    const draft = state.newItem;
    if (!draft.type) return null;

    const defaultName: Record<NewType, string> = {
      project: '新项目',
      module: '新模块',
      collection: '新集合',
      endpoint: '新接口',
    };
    const name = draft.name.trim() || defaultName[draft.type];

    if (draft.type === 'project') {
      const id = await state.addProject(name, {
        description: draft.description,
        baseUrl: draft.baseUrl,
        color: draft.color,
      });
      if (id) state.setActiveProject(id);
      state.setActiveEndpoint(null);
      set({ lastCreatedId: id });
      return id;
    }

    if (draft.type === 'module') {
      const id = await state.addNode({
        rootId: state.activeProjectId,
        parentId: null,
        kind: 'module',
        name,
        description: draft.description,
      });
      set({ lastCreatedId: id });
      return id;
    }

    if (draft.type === 'collection') {
      if (!draft.parentModuleId) return null;
      const parent = state.nodes.find((n) => n.id === draft.parentModuleId);
      if (parent && !parent.expanded) state.toggleNodeExpanded(parent.id);
      const id = await state.addNode({
        // `rootId` is derived from the parent server-side; naming it
        // here as well would be a second source of truth for a value the
        // parent already determines.
        parentId: draft.parentModuleId,
        kind: 'collection',
        name,
        description: draft.description,
      });
      set({ lastCreatedId: id });
      return id;
    }

    // draft.type === 'endpoint'
    if (!draft.parentCollectionId) return null;
    // Expand every ancestor so the new endpoint is visible where it
    // landed. With one node type this is a walk up `parentId` instead of
    // the old "open the collection, then find its module" two-step.
    let ancestor: ResourceNode | undefined = state.nodes.find(
      (n) => n.id === draft.parentCollectionId,
    );
    while (ancestor) {
      if (!ancestor.expanded) state.toggleNodeExpanded(ancestor.id);
      ancestor = ancestor.parentId
        ? state.nodes.find((n) => n.id === ancestor!.parentId)
        : undefined;
    }
    const id = await state.addNode({
      parentId: draft.parentCollectionId,
      kind: 'endpoint',
      name,
      // An endpoint is born with a detail row; without one the tree
      // would hold a node the tester cannot open.
      detail: { url: '' },
    });
    if (id) state.setActiveEndpoint(id);
    set({ lastCreatedId: id });
    return id;
  },
  clearLastCreated: () => set({ lastCreatedId: null }),
});
