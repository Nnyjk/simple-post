/**
 * Simple Post — global app store (Zustand).
 *
 * --------------------------------------------------------------------------
 * SELECTOR RULES (HARD CONSTRAINT — read before adding new selectors)
 * --------------------------------------------------------------------------
 * Zustand v5 uses `Object.is` to compare selector outputs. Any selector that
 * returns a freshly-allocated value (`.map`, `.filter`, `.sort`, spread, etc.)
 * WILL trigger an infinite re-render loop in the consuming component.
 *
 * Therefore:
 *   ✅ ALLOWED:  return raw state references or single `.find()` results
 *                e.g. `s => s.projects`, `s => s.endpoints.find(e => e.id === id)`
 *   ❌ FORBIDDEN: `s => s.endpoints.filter(...)`, `s => [...s.endpoints]`,
 *                 `s => s.endpoints.sort(...)`, anything that allocates.
 *
 * All filtering / sorting / grouping that needs derived arrays MUST happen
 * inside the consuming component via `useMemo`, not inside the selector.
 *
 * The three exported `useActive*` selectors below are the canonical pattern.
 * If you need a new selector, follow the same shape (`s => s.x` or
 * `s => s.x.find(...)`). Do not introduce array-derived selectors.
 * --------------------------------------------------------------------------
 */

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  Project,
  Module,
  Collection,
  Endpoint,
  Environment,
  ApiResponse,
  HttpMethod,
  BaseUrlDefinition,
} from '@/types/domain';
import { uid } from '@/lib/utils';
import { COMMANDS } from '@/lib/tauri-commands';
import type { McpRuntimeStatus, McpUiState } from '@/lib/mcp-types';

export type Theme = 'dark' | 'light' | 'system';

export interface RequestSettings {
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
}

/**
 * The 4 kinds of "new" actions the global NewItemModal supports.
 *
 * NOTE: ProjectTree also has a local `NewType` alias with the same shape.
 * The two are intentionally the same so the modal can pre-fill a draft
 * for the same target as the tree's old popover.
 */
export type NewType = 'project' | 'module' | 'collection' | 'endpoint';

/** Kinds of tabs the right-pane workspace can show. */
export type TabKind = 'project' | 'module' | 'collection' | 'endpoint' | 'settings';

/**
 * Sentinel id used for the (single) settings tab. Real entities use their
 * own id, but settings is a singleton panel — using a fixed id keeps the
 * open/close/active logic identical to the entity tabs.
 */
export const SETTINGS_TAB_ID = '__settings__';

/** Inner sections of the settings tab. Drives the left rail. */
export type SettingsCategoryId = 'env' | 'connection' | 'appearance' | 'request';

/**
 * An open tab in the right-pane workspace.
 *
 * `id` reuses the underlying entity id (project / module / collection /
 * endpoint), which makes lookup O(1) and lets `closeTab` cleanly remove
 * a tab when the user deletes the entity.
 */
export interface OpenTab {
  id: string;
  kind: TabKind;
}

/**
 * Per-endpoint unsaved URL/method draft. Lives in `endpointDrafts`
 * (not in HttpTester local state) so that switching tabs and
 * switching back preserves the user's pending edits. `url` and
 * `method` are individually optional — only the fields the user
 * actually edited are present.
 */
export interface EndpointDraft {
  url?: string;
  method?: HttpMethod;
}

/** In-memory draft of the NewItemModal. */
export interface NewItemDraft {
  open: boolean;
  type: NewType | null;
  parentModuleId: string;
  parentCollectionId: string;
  name: string;
  description: string;
  baseUrl: string;
  color: string;
}

/** Saved example — same shape as ApiResponse but with a stable `id` for delete. */
export type ApiResponseExample = ApiResponse & { id: string };

/** Persisted user preferences — only the fields that survive reloads. */
interface PersistedPrefs {
  theme?: Theme;
  requestSettings?: Partial<RequestSettings>;
}

const PREFS_KEY = 'simple-post:prefs';
const MAX_HISTORY_PER_ENDPOINT = 10;

/**
 * Polling cadence for the shared MCP daemon snapshot. Single source of
 * truth — both `startMcpPolling` (in the store) and the legacy
 * `setInterval` it replaced used this value.
 */
const MCP_REFRESH_INTERVAL_MS = 3000;

// Browser-only — holds the bound `visibilitychange` handler so the
// store can detach it on `stopMcpPolling`. The store action is the
// only consumer so a module-scoped variable (no React state) is fine.
let _mcpVisibilityHandler: ((this: Document, ev: Event) => unknown) | null = null;
function _setVisibilityHandler(
  h: ((this: Document, ev: Event) => unknown) | null,
): void {
  _mcpVisibilityHandler = h;
}
function _getVisibilityHandler():
  | ((this: Document, ev: Event) => unknown)
  | null {
  return _mcpVisibilityHandler;
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

interface AppState {
  // data
  projects: Project[];
  modules: Module[];
  collections: Collection[];
  endpoints: Endpoint[];
  environments: Environment[];
  /** True once bootstrap() has resolved at least once. */
  loaded: boolean;

  // ui state
  activeProjectId: string;
  activeEnvironmentId: string;
  activeEndpointId: string | null;
  loading: boolean;

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

  // request / response
  lastResponse: ApiResponse | null;
  requestPending: boolean;

  // preferences (theme + request settings)
  theme: Theme;
  requestSettings: RequestSettings;

  /**
   * Shared MCP daemon snapshot, polled every 3s by exactly one
   * `setInterval` regardless of how many components consume it.
   * `null` until the first successful poll (renderers render a
   * loading-style state for this).
   */
  mcpStatus: McpUiState | null;
  /** Browser-only. The active 3s timer id, or `null` when idle. */
  _mcpTimer: number | null;
  /**
   * Last `status` for which the ConnectionSection's auto-open restart
   * dialog has already fired. Used to dedupe — when the daemon
   * reports `crashed` (or `unhealthy`) twice in a row, the dialog
   * should appear once, not twice. Lives on the store so the
   * auto-open effect survives remounts of the settings tab.
   */
  restartDialogSeen: McpRuntimeStatus | null;

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

  // actions: data — selection / expansion
  setActiveProject: (id: string) => void;
  setActiveEnvironment: (id: string) => void;
  setActiveEndpoint: (id: string | null) => void;
  toggleModuleExpanded: (id: string) => void;
  toggleCollectionExpanded: (id: string) => void;

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
   * `updateEndpoint` and clearing the draft. For other tabs the inline
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
   * Open the (singleton) settings tab and select a section. Idempotent
   * — calling it when the tab is already open just switches the
   * active section and makes the tab active.
   */
  openSettingsTab: (section?: SettingsCategoryId) => void;
  /** Update which section of the settings tab is active. */
  setActiveSettingsSection: (section: SettingsCategoryId) => void;
  /**
   * Filter the given tab ids down to the ones currently flagged as
   * dirty. Internal helper shared by the request-close paths.
   */
  partitionDirty: (targetIds: string[]) => string[];

  // actions: data — CRUD (async — all hit Tauri commands, mirror to local state)
  addProject: (name: string, opts?: { description?: string; baseUrl?: string; color?: string }) => Promise<string | null>;
  updateProject: (id: string, patch: Partial<Omit<Project, 'id' | 'createdAt'>>) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;

  addModule: (projectId: string, name: string, opts?: { description?: string }) => Promise<string | null>;
  updateModule: (id: string, patch: Partial<Omit<Module, 'id' | 'projectId'>>) => Promise<void>;
  deleteModule: (id: string) => Promise<void>;

  addCollection: (
    moduleId: string,
    name: string,
    opts?: { description?: string; parentCollectionId?: string | null },
  ) => Promise<string | null>;
  updateCollection: (id: string, patch: Partial<Omit<Collection, 'id' | 'moduleId' | 'parentCollectionId'>>) => Promise<void>;
  deleteCollection: (id: string) => Promise<void>;

  addEndpoint: (
    collectionId: string,
    name: string,
    opts?: { method?: HttpMethod; url?: string },
  ) => Promise<string | null>;
  updateEndpoint: (id: string, patch: Partial<Endpoint>) => Promise<void>;
  deleteEndpoint: (id: string) => Promise<void>;
  /**
   * Duplicate an endpoint. Returns the new id **synchronously** because
   * the caller (ProjectTree) immediately activates the new tab — making
   * this async would break the existing UX. Implementation: optimistic
   * local copy first, then background invoke to persist; on invoke
   * success the temp id is replaced with the persisted entity, on
   * failure the optimistic copy is removed.
   */
  duplicateEndpoint: (id: string) => string | null;

  // actions: data — move (drag-and-drop reorder + cross-container moves)
  /**
   * Move a module within its project. The target index is the desired
   * position in the project's sorted module list. No-op if the module
   * is not in any project (shouldn't happen) or the index is out of
   * range (clamped).
   *
   * Children of the module (collections, recursively) are carried
   * automatically — the parent FKs are unchanged.
   *
   * Optimistic local reorder + background invoke; rolls back on failure.
   */
  moveModule: (moduleId: string, targetIndex: number) => Promise<void>;
  /**
   * Move one or more collections to a new parent + position.
   *
   * `parent` is either a module (top-level in that module) or a
   * collection (nested under that collection). `index` is the
   * 0-based position inside the new parent's sorted children.
   *
   * The new module is inferred from the parent (the target module's
   * own `moduleId`, or the target collection's `moduleId`). When the
   * move crosses modules, the moved collection and **all of its
   * descendants** get their `moduleId` updated to match — this is the
   * "模块/集合的变动需要携带其下的子项一起变动" rule.
   *
   * Guards:
   *   - A collection cannot be moved under itself or any descendant
   *     (cycle prevention).
   *   - The target parent must exist in the same project as the
   *     source collection.
   *
   * Optimistic local move + background invoke; rolls back on failure.
   */
  moveCollections: (
    collectionIds: string[],
    target: {
      parentKind: 'module' | 'collection';
      parentId: string;
      index: number;
    },
  ) => Promise<void>;
  /**
   * Move one or more endpoints to a new collection + position.
   * Supports multi-select (the "接口与接口形成集合" gesture: select
   * several endpoints and drop on a collection to consolidate them
   * into one place).
   *
   * If the source endpoints are already in the target collection,
   * this acts as a pure reorder. If they come from different source
   * collections, they all end up in the target — that's how multiple
   * endpoints "form a collection" without us having to create a new
   * collection node.
   *
   * Optimistic local move + background invoke; rolls back on failure.
   */
  moveEndpoints: (
    endpointIds: string[],
    target: { collectionId: string; index: number },
  ) => Promise<void>;

  // actions: environment (async — Tauri commands)
  setEnvironmentVar: (envId: string, key: string, value: string) => Promise<void>;
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  updateEnvironment: (envId: string, patch: Partial<Pick<Environment, 'name'>>) => Promise<void>;
  /**
   * Set the URL of a single baseUrl definition inside an env. The
   * baseUrl is stored as `env.baseUrls[defId]`, not as a top-level
   * `Environment.baseUrl` string — that's how multiple named
   * baseUrls (e.g. "鉴权服务" / "数据服务") coexist per env.
   */
  setEnvironmentBaseUrl: (envId: string, defId: string, url: string) => Promise<void>;
  deleteEnvironment: (envId: string) => Promise<void>;
  setEnvironmentActive: (envId: string) => Promise<void>;

  // actions: baseUrl definitions (project-scoped)
  /**
   * Add a project-scoped baseUrl definition. Returns the new id
   * **synchronously** because the caller (EnvironmentSection) passes
   * it straight into `setEnvironmentBaseUrl` to seed an empty row.
   * Optimistic local add + background invoke; rolls back on failure.
   */
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (projectId: string, defId: string, patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>) => Promise<void>;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => Promise<void>;
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => Promise<void>;

  // actions: bootstrap (one-shot at app start)
  /**
   * Fetch all persistent data from the SQLite daemon and mirror into
   * the store. Idempotent — a second call after the first is a no-op.
   * Triggered from TopBar's useEffect on mount.
   */
  bootstrap: () => Promise<void>;

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
  openNewItem: (opts?: { type?: NewType; parentModuleId?: string; parentCollectionId?: string }) => void;
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

  // actions: MCP daemon polling
  /**
   * Begin the 3-second MCP polling loop if it isn't already running.
   * Idempotent — calling this from N components still results in exactly
   * one `setInterval` and one IPC round-trip per tick.
   *
   * Also installs a `document.visibilitychange` listener that pauses
   * the timer while the tab is hidden (no point polling for users
   * who can't see the badge). The listener is removed by
   * `stopMcpPolling`.
   */
  startMcpPolling: () => void;
  /**
   * Stop the polling loop and detach the visibility listener. Pair
   * every `startMcpPolling` (typically from a `useMcpStatus` effect
   * mount) with a `stopMcpPolling` cleanup so we don't leak.
   */
  stopMcpPolling: () => void;
  /**
   * Force a single refresh from the daemon and push the result into
   * `mcpStatus`. On failure we only log (so the UI keeps rendering
   * the last-known good state) — the bootstrap path's silent
   * `return []` behaviour was the wrong default here.
   */
  refreshMcpStatus: () => Promise<void>;
  /**
   * Trigger the daemon restart command. The action is idempotent at
   * the *UI* level (the component is the caller, so the button is
   * disabled while a restart is already in flight). On failure we
   * flip `mcpStatus.lastError` so the crashed dialog can show the
   * underlying cause.
   */
  restartMcp: () => Promise<void>;
  /**
   * Record that the auto-open restart dialog has been shown for the
   * given status. Idempotent — the ConnectionSection effect's dedupe
   * logic calls this when the user dismisses the dialog so the next
   * occurrence of the same status doesn't immediately reopen it.
   */
  markRestartDialogSeen: (status: McpRuntimeStatus) => void;
}

// ---------- helpers (pure, used inside set callbacks) ----------

/** Re-number sortOrder for a slice after a reorder. */
function renumber<T extends { sortOrder: number }>(items: readonly T[]): T[] {
  return items.map((it, idx) => ({ ...it, sortOrder: idx }));
}

/** Move an item within a sorted-by-sortOrder list. */
function reorderByIndex<T extends { sortOrder: number }>(
  items: readonly T[],
  fromIndex: number,
  toIndex: number,
): T[] {
  if (fromIndex === toIndex) return items.slice();
  const clampedFrom = Math.max(0, Math.min(fromIndex, items.length - 1));
  const clampedTo = Math.max(0, Math.min(toIndex, items.length - 1));
  const next = items.slice();
  const [moved] = next.splice(clampedFrom, 1);
  next.splice(clampedTo, 0, moved);
  return renumber(next);
}

/**
 * Build a fast lookup: `parentId -> child collections`, sorted by
 * `sortOrder`. Used by both the renderer (to flatten the tree) and the
 * store (to find siblings when a move happens).
 */
function indexCollectionChildren(collections: readonly Collection[]): Map<string | null, Collection[]> {
  const byParent = new Map<string | null, Collection[]>();
  for (const c of collections) {
    const key = c.parentCollectionId;
    const bucket = byParent.get(key);
    if (bucket) bucket.push(c);
    else byParent.set(key, [c]);
  }
  for (const bucket of byParent.values()) {
    bucket.sort((a, b) => a.sortOrder - b.sortOrder);
  }
  return byParent;
}

/**
 * Collect a collection's id and all descendant collection ids (BFS
 * down `parentCollectionId`). Used by `moveCollections` to update the
 * denormalized `moduleId` on every node in the moved subtree when
 * the move crosses modules.
 */
function collectCollectionSubtree(
  rootId: string,
  childrenIndex: Map<string | null, Collection[]>,
): Set<string> {
  const out = new Set<string>();
  const queue: string[] = [rootId];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (out.has(id)) continue;
    out.add(id);
    const kids = childrenIndex.get(id) ?? [];
    for (const k of kids) queue.push(k.id);
  }
  return out;
}

/**
 * Build the set of entity ids that should be closed alongside a deletion.
 * Pure helper so each delete* action can call it without duplicating logic.
 *
 *   deleteProject(p)  → { project p, all modules in p, all collections in
 *                        those modules, all endpoints in those collections }
 *   deleteModule(m)   → { module m, all collections in m, all endpoints
 *                        in those collections }
 *   deleteCollection(c) → { collection c, all endpoints in c }
 *   deleteEndpoint(e) → { endpoint e }
 */
function idsToCloseForDelete(
  scope: 'project' | 'module' | 'collection' | 'endpoint',
  targetId: string,
  state: Pick<AppState, 'projects' | 'modules' | 'collections' | 'endpoints'>,
): { project: string[]; module: string[]; collection: string[]; endpoint: string[] } {
  const moduleIds: string[] = [];
  const collectionIds: string[] = [];
  const endpointIds: string[] = [];

  if (scope === 'project') {
    const ms = state.modules.filter((m) => m.projectId === targetId);
    moduleIds.push(...ms.map((m) => m.id));
    collectionIds.push(
      ...state.collections.filter((c) => moduleIds.includes(c.moduleId)).map((c) => c.id),
    );
    endpointIds.push(
      ...state.endpoints.filter((e) => collectionIds.includes(e.collectionId)).map((e) => e.id),
    );
    return { project: [targetId], module: moduleIds, collection: collectionIds, endpoint: endpointIds };
  }

  if (scope === 'module') {
    moduleIds.push(targetId);
    collectionIds.push(
      ...state.collections.filter((c) => c.moduleId === targetId).map((c) => c.id),
    );
    endpointIds.push(
      ...state.endpoints.filter((e) => collectionIds.includes(e.collectionId)).map((e) => e.id),
    );
    return { project: [], module: moduleIds, collection: collectionIds, endpoint: endpointIds };
  }

  if (scope === 'collection') {
    collectionIds.push(targetId);
    endpointIds.push(
      ...state.endpoints.filter((e) => e.collectionId === targetId).map((e) => e.id),
    );
    return { project: [], module: [], collection: collectionIds, endpoint: endpointIds };
  }

  // endpoint
  endpointIds.push(targetId);
  return { project: [], module: [], collection: [], endpoint: endpointIds };
}

/**
 * Drop tabs whose underlying entity was removed. Returns the new tab list,
 * the new active tab id (picking the previous neighbor if the active one
 * was removed), and a flag indicating whether anything changed.
 */
function filterTabsAfterRemoval(
  tabs: readonly OpenTab[],
  activeId: string | null,
  toClose: ReturnType<typeof idsToCloseForDelete>,
): { tabs: OpenTab[]; activeId: string | null; changed: boolean } {
  const closeSet = new Set<string>();
  for (const id of toClose.project) closeSet.add(`project:${id}`);
  for (const id of toClose.module) closeSet.add(`module:${id}`);
  for (const id of toClose.collection) closeSet.add(`collection:${id}`);
  for (const id of toClose.endpoint) closeSet.add(`endpoint:${id}`);

  const nextTabs: OpenTab[] = [];
  for (const t of tabs) {
    if (closeSet.has(`${t.kind}:${t.id}`)) continue;
    nextTabs.push(t);
  }
  if (nextTabs.length === tabs.length) {
    return { tabs: tabs.slice(), activeId, changed: false };
  }

  let nextActive = activeId;
  if (activeId !== null && !nextTabs.some((t) => t.id === activeId)) {
    const oldIdx = tabs.findIndex((t) => t.id === activeId);
    // Prefer the previous tab (left neighbor), fall back to next, then null.
    const fallback =
      tabs[oldIdx - 1] ?? tabs[oldIdx + 1] ?? nextTabs[nextTabs.length - 1] ?? null;
    nextActive = fallback ? fallback.id : null;
  }
  return { tabs: nextTabs, activeId: nextActive, changed: true };
}

export const useAppStore = create<AppState>((set, get) => ({
  // v0.1 MCP rollout — empty at boot; `bootstrap()` populates from the
  // daemon (which reads SQLite). The selectors and consumers all tolerate
  // an empty list because `loaded: false` triggers a loading state in the
  // UI rather than rendering the (now-empty) tree.
  projects: [],
  modules: [],
  collections: [],
  endpoints: [],
  environments: [],
  /** True once bootstrap() has resolved at least once. Idempotent. */
  loaded: false,

  activeProjectId: '',
  activeEnvironmentId: '',
  activeEndpointId: null,
  loading: false,

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
  // No pending close batch at boot. The DirtyCloseDialog component
  // only mounts when this becomes non-null.
  pendingDirtyClose: null,

  lastResponse: null,
  requestPending: false,

  theme: initialTheme,
  requestSettings: initialRequestSettings,

  // MCP daemon state — populated lazily by `startMcpPolling()`. The
  // initial `null` lets consumers render a "loading" affordance until
  // the first `refreshMcpStatus()` round-trip completes.
  mcpStatus: null,
  _mcpTimer: null,
  restartDialogSeen: null,

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

  // ---------- selection / expansion ----------

  setActiveProject: (id) =>
    set((s) => {
      const env =
        s.environments.find((e) => e.projectId === id && e.isActive) ??
        s.environments.find((e) => e.projectId === id);
      // Clear activeEnvironmentId to '' when the new project has no env
      // (sentinel for "none selected" — useActiveEnvironment() will then
      // return undefined and the TopBar trigger falls back to '—').
      return {
        activeProjectId: id,
        activeEnvironmentId: env?.id ?? '',
      };
    }),
  setActiveEnvironment: (id) => set({ activeEnvironmentId: id }),
  setActiveEndpoint: (id) =>
    set((s) => ({
      // If a tab for this endpoint is already open, just activate it. If
      // not, create one. Then keep `activeEndpointId` in sync so legacy
      // selectors (useActiveEndpoint) keep returning the right thing.
      activeEndpointId: id,
      lastResponse: null,
      openTabs:
        id === null
          ? s.openTabs
          : s.openTabs.some((t) => t.kind === 'endpoint' && t.id === id)
            ? s.openTabs
            : [...s.openTabs, { id, kind: 'endpoint' }],
      activeTabId: id === null ? s.activeTabId : id,
    })),
  toggleModuleExpanded: (id) =>
    set((s) => ({
      modules: s.modules.map((m) => (m.id === id ? { ...m, expanded: !m.expanded } : m)),
    })),
  toggleCollectionExpanded: (id) =>
    set((s) => ({
      collections: s.collections.map((c) => (c.id === id ? { ...c, expanded: !c.expanded } : c)),
    })),

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
        if (t.kind === 'module') return s.modules.some((m) => m.id === t.id);
        if (t.kind === 'collection') return s.collections.some((c) => c.id === t.id);
        return s.endpoints.some((e) => e.id === t.id);
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
    const targetIds = state.openTabs
      .filter((t) => t.id !== state.activeTabId)
      .map((t) => t.id);
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
      const patch: Partial<Endpoint> = {};
      if (draft.url !== undefined) patch.url = draft.url;
      if (draft.method !== undefined) patch.method = draft.method;
      if (Object.keys(patch).length > 0) state.updateEndpoint(tabId, patch);
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

  openSettingsTab: (section) => {
    const state = get();
    if (section) set({ activeSettingsSection: section });
    const existing = state.openTabs.find(
      (t) => t.kind === 'settings' && t.id === SETTINGS_TAB_ID,
    );
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

  // ---------- bootstrap (one-shot, called from TopBar mount) ----------

  bootstrap: async () => {
    if (get().loaded) return;
    // Fetch all top-level slices in parallel. List commands take no
    // filter args in this revision (Worker C's `commands.rs`); the
    // inner `catch`es degrade gracefully when the daemon isn't
    // reachable yet (e.g. dev without `tauri dev`) so the UI doesn't
    // crash — `loaded` simply stays false and the user sees the empty
    // state until the daemon comes online.
    const [projects, modules, collections, endpoints, environments] = await Promise.all([
      invoke<Project[]>(COMMANDS.projectsList).catch((e) => {
        console.error('[store] projects_list failed', e);
        return [] as Project[];
      }),
      invoke<Module[]>(COMMANDS.modulesList).catch((e) => {
        console.error('[store] modules_list failed', e);
        return [] as Module[];
      }),
      invoke<Collection[]>(COMMANDS.collectionsList).catch((e) => {
        console.error('[store] collections_list failed', e);
        return [] as Collection[];
      }),
      invoke<Endpoint[]>(COMMANDS.endpointsList, {}).catch((e) => {
        console.error('[store] endpoints_list failed', e);
        return [] as Endpoint[];
      }),
      invoke<Environment[]>(COMMANDS.environmentsList).catch((e) => {
        console.error('[store] environments_list failed', e);
        return [] as Environment[];
      }),
    ]);

    // Pick a sensible default selection now that the data is in.
    const firstProject = projects[0];
    const firstProjectEnv =
      firstProject
        ? environments.find((e) => e.projectId === firstProject.id && e.isActive) ??
          environments.find((e) => e.projectId === firstProject.id)
        : undefined;
    const firstEndpoint = endpoints[0];

    set({
      projects,
      modules,
      collections,
      endpoints,
      environments,
      loaded: true,
      activeProjectId: firstProject?.id ?? '',
      activeEnvironmentId: firstProjectEnv?.id ?? '',
      activeEndpointId: firstEndpoint?.id ?? null,
      // Open the first endpoint by default, mirroring the old mock seed.
      openTabs: firstEndpoint ? [{ id: firstEndpoint.id, kind: 'endpoint' as const }] : [],
      activeTabId: firstEndpoint?.id ?? null,
    });
  },

  // ---------- project CRUD (async — Tauri commands) ----------

  addProject: async (name, opts) => {
    try {
      const project = await invoke<Project>(COMMANDS.projectsCreate, {
        name,
        opts: opts ?? null,
      });
      set((s) => ({ projects: [...s.projects, project] }));
      return project.id;
    } catch (e) {
      console.error('[store] projects_create failed', e);
      return null;
    }
  },
  updateProject: async (id, patch) => {
    // Optimistic local update + background invoke; rollback on failure.
    const prev = get().projects.find((p) => p.id === id);
    if (!prev) return;
    const optimistic: Project = { ...prev, ...patch, updatedAt: Date.now() };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? optimistic : p)),
    }));
    try {
      await invoke<Project>(COMMANDS.projectsUpdate, { id, patch });
    } catch (e) {
      console.error('[store] projects_update failed', e);
      set((s) => ({
        projects: s.projects.map((p) => (p.id === id ? prev : p)),
      }));
    }
  },
  deleteProject: async (id) => {
    // Snapshot for rollback.
    const snapshot = {
      projects: get().projects,
      modules: get().modules,
      collections: get().collections,
      endpoints: get().endpoints,
      environments: get().environments,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeProjectId: get().activeProjectId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const moduleIds = s.modules.filter((m) => m.projectId === id).map((m) => m.id);
      const collectionIds = s.collections
        .filter((c) => moduleIds.includes(c.moduleId))
        .map((c) => c.id);
      const droppedEndpointIds = s.endpoints
        .filter((e) => collectionIds.includes(e.collectionId))
        .map((e) => e.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        idsToCloseForDelete('project', id, s),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && droppedEndpointIds.includes(s.activeEndpointId)
          ? null
          : s.activeEndpointId;
      return {
        projects: s.projects.filter((p) => p.id !== id),
        modules: s.modules.filter((m) => m.projectId !== id),
        collections: s.collections.filter((c) => !moduleIds.includes(c.moduleId)),
        endpoints: s.endpoints.filter((e) => !collectionIds.includes(e.collectionId)),
        environments: s.environments.filter((env) => env.projectId !== id),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeProjectId: s.activeProjectId === id ? (s.projects.find((p) => p.id !== id)?.id ?? s.activeProjectId) : s.activeProjectId,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invoke<void>(COMMANDS.projectsDelete, { id });
    } catch (e) {
      console.error('[store] projects_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- module CRUD (async — Tauri commands) ----------

  addModule: async (projectId, name, opts) => {
    try {
      const mod = await invoke<Module>(COMMANDS.modulesCreate, {
        projectId,
        name,
        opts: opts ?? null,
      });
      set((s) => ({ modules: [...s.modules, mod] }));
      return mod.id;
    } catch (e) {
      console.error('[store] modules_create failed', e);
      return null;
    }
  },
  updateModule: async (id, patch) => {
    const prev = get().modules.find((m) => m.id === id);
    if (!prev) return;
    const optimistic: Module = { ...prev, ...patch };
    set((s) => ({
      modules: s.modules.map((m) => (m.id === id ? optimistic : m)),
    }));
    try {
      await invoke<Module>(COMMANDS.modulesUpdate, { id, patch });
    } catch (e) {
      console.error('[store] modules_update failed', e);
      set((s) => ({
        modules: s.modules.map((m) => (m.id === id ? prev : m)),
      }));
    }
  },
  deleteModule: async (id) => {
    const snapshot = {
      modules: get().modules,
      collections: get().collections,
      endpoints: get().endpoints,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const collectionIds = s.collections.filter((c) => c.moduleId === id).map((c) => c.id);
      const droppedEndpointIds = s.endpoints
        .filter((e) => collectionIds.includes(e.collectionId))
        .map((e) => e.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        idsToCloseForDelete('module', id, s),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && droppedEndpointIds.includes(s.activeEndpointId)
          ? null
          : s.activeEndpointId;
      return {
        modules: s.modules.filter((m) => m.id !== id),
        collections: s.collections.filter((c) => c.moduleId !== id),
        endpoints: s.endpoints.filter((e) => !collectionIds.includes(e.collectionId)),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invoke<void>(COMMANDS.modulesDelete, { id });
    } catch (e) {
      console.error('[store] modules_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- collection CRUD ----------

  addCollection: async (moduleId, name, opts) => {
    try {
      const collection = await invoke<Collection>(COMMANDS.collectionsCreate, {
        moduleId,
        name,
        opts: opts ?? null,
      });
      set((s) => ({ collections: [...s.collections, collection] }));
      return collection.id;
    } catch (e) {
      console.error('[store] collections_create failed', e);
      return null;
    }
  },
  updateCollection: async (id, patch) => {
    const prev = get().collections.find((c) => c.id === id);
    if (!prev) return;
    const optimistic: Collection = { ...prev, ...patch };
    set((s) => ({
      collections: s.collections.map((c) => (c.id === id ? optimistic : c)),
    }));
    try {
      await invoke<Collection>(COMMANDS.collectionsUpdate, { id, patch });
    } catch (e) {
      console.error('[store] collections_update failed', e);
      set((s) => ({
        collections: s.collections.map((c) => (c.id === id ? prev : c)),
      }));
    }
  },
  deleteCollection: async (id) => {
    const snapshot = {
      collections: get().collections,
      endpoints: get().endpoints,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const droppedEndpointIds = s.endpoints
        .filter((e) => e.collectionId === id)
        .map((e) => e.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        idsToCloseForDelete('collection', id, s),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && droppedEndpointIds.includes(s.activeEndpointId)
          ? null
          : s.activeEndpointId;
      return {
        collections: s.collections.filter((c) => c.id !== id),
        endpoints: s.endpoints.filter((e) => e.collectionId !== id),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invoke<void>(COMMANDS.collectionsDelete, { id });
    } catch (e) {
      console.error('[store] collections_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- endpoint CRUD ----------

  addEndpoint: async (collectionId, name, opts) => {
    try {
      const endpoint = await invoke<Endpoint>(COMMANDS.endpointsCreate, {
        collectionId,
        name,
        opts: opts ?? null,
      });
      set((s) => ({ endpoints: [...s.endpoints, endpoint] }));
      return endpoint.id;
    } catch (e) {
      console.error('[store] endpoints_create failed', e);
      return null;
    }
  },
  updateEndpoint: async (id, patch) => {
    const prev = get().endpoints.find((e) => e.id === id);
    if (!prev) return;
    const optimistic: Endpoint = { ...prev, ...patch, updatedAt: Date.now() };
    set((s) => ({
      endpoints: s.endpoints.map((e) => (e.id === id ? optimistic : e)),
    }));
    try {
      await invoke<Endpoint>(COMMANDS.endpointsUpdate, { id, patch });
    } catch (e) {
      console.error('[store] endpoints_update failed', e);
      set((s) => ({
        endpoints: s.endpoints.map((e) => (e.id === id ? prev : e)),
      }));
    }
  },
  deleteEndpoint: async (id) => {
    const snapshot = {
      endpoints: get().endpoints,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      delete nextHistory[id];
      delete nextExamples[id];
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        idsToCloseForDelete('endpoint', id, s),
      );
      return {
        endpoints: s.endpoints.filter((e) => e.id !== id),
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeEndpointId: s.activeEndpointId === id ? null : s.activeEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invoke<void>(COMMANDS.endpointsDelete, { id });
    } catch (e) {
      console.error('[store] endpoints_delete failed', e);
      set(snapshot);
    }
  },
  /**
   * Optimistic duplicate + background invoke. Returns the temp id
   * synchronously so the caller (ProjectTree) can activate the new
   * tab immediately. On invoke success the temp row is replaced by
   * the persisted entity; on failure it's removed and we log.
   */
  duplicateEndpoint: (id) => {
    const original = get().endpoints.find((e) => e.id === id);
    if (!original) return null;
    const newId = uid();
    const now = Date.now();
    set((s) => {
      const maxOrder = s.endpoints
        .filter((e) => e.collectionId === original.collectionId)
        .reduce((acc, e) => Math.max(acc, e.sortOrder), -1);
      return {
        endpoints: [
          ...s.endpoints,
          {
            ...original,
            id: newId,
            name: `${original.name} (副本)`,
            sortOrder: maxOrder + 1,
            createdAt: now,
            updatedAt: now,
          },
        ],
      };
    });
    invoke<Endpoint>(COMMANDS.endpointsDuplicate, { id })
      .then((persisted) => {
        // Swap the temp row with the persisted one (server may assign
        // a different id or normalize fields).
        set((s) => ({
          endpoints: s.endpoints.map((e) => (e.id === newId ? persisted : e)),
        }));
      })
      .catch((e) => {
        console.error('[store] endpoints_duplicate failed', e);
        set((s) => ({
          endpoints: s.endpoints.filter((e) => e.id !== newId),
        }));
      });
    return newId;
  },

  // ---------- move (drag-and-drop) ----------

  moveModule: async (moduleId, targetIndex) => {
    const prevModules = get().modules;
    set((s) => {
      const mod = s.modules.find((m) => m.id === moduleId);
      if (!mod) return {};
      const inScope = s.modules
        .filter((m) => m.projectId === mod.projectId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const fromIndex = inScope.findIndex((m) => m.id === moduleId);
      if (fromIndex < 0) return {};
      const reordered = reorderByIndex(inScope, fromIndex, targetIndex);
      const outOfScope = s.modules.filter((m) => m.projectId !== mod.projectId);
      return { modules: [...outOfScope, ...reordered] };
    });
    try {
      await invoke<void>(COMMANDS.modulesMove, { moduleId, targetIndex });
    } catch (e) {
      console.error('[store] modules_move failed', e);
      set({ modules: prevModules });
    }
  },

  moveCollections: async (collectionIds, target) => {
    const prevCollections = get().collections;
    set((s) => {
      if (collectionIds.length === 0) return {};
      // Dedup + drop unknowns up-front so the rest of the logic can
      // assume every id is a real collection.
      const idSet = new Set(collectionIds);
      const sources = s.collections.filter((c) => idSet.has(c.id));
      if (sources.length === 0) return {};

      // Resolve the target parent + its owning module.
      let targetModuleId: string;
      if (target.parentKind === 'module') {
        const targetMod = s.modules.find((m) => m.id === target.parentId);
        if (!targetMod) return {};
        targetModuleId = targetMod.id;
      } else {
        const targetCol = s.collections.find((c) => c.id === target.parentId);
        if (!targetCol) return {};
        targetModuleId = targetCol.moduleId;
      }
      const targetParentId =
        target.parentKind === 'module' ? null : target.parentId;

      // Cycle guard: a collection cannot be moved under itself or any
      // descendant. Build the descendant set for every source once.
      const childrenIndex = indexCollectionChildren(s.collections);
      const allSubtrees = new Set<string>();
      for (const src of sources) {
        for (const id of collectCollectionSubtree(src.id, childrenIndex)) {
          allSubtrees.add(id);
        }
      }
      if (targetParentId !== null && allSubtrees.has(targetParentId)) {
        // Target is inside one of the moved subtrees — would create a
        // cycle. Silently no-op; the UI is expected to disable this
        // drop target, but we double-check here for safety.
        return {};
      }

      // Project-boundary guard: every source must belong to the same
      // project as the target. Since `moduleId` is denormalized on
      // collections, we just compare source.moduleId's project.
      const targetProjectId = s.modules.find((m) => m.id === targetModuleId)?.projectId;
      if (!targetProjectId) return {};
      for (const src of sources) {
        const srcMod = s.modules.find((m) => m.id === src.moduleId);
        if (!srcMod || srcMod.projectId !== targetProjectId) return {};
      }

      // Siblings inside the new parent, with sources removed (we
      // reinsert them at `target.index` below).
      const siblings = s.collections
        .filter(
          (c) =>
            c.moduleId === targetModuleId &&
            c.parentCollectionId === targetParentId &&
            !idSet.has(c.id),
        )
        .sort((a, b) => a.sortOrder - b.sortOrder);

      // The moved rows keep their relative order in `collectionIds`
      // (no stable-sort key — just preserve call order).
      const movedRows = sources
        .slice()
        .sort((a, b) => collectionIds.indexOf(a.id) - collectionIds.indexOf(b.id))
        .map((c) => ({
          ...c,
          moduleId: targetModuleId,
          parentCollectionId: targetParentId,
        }));

      // Splice into the sibling list at the requested index.
      const clampedIndex = Math.max(0, Math.min(target.index, siblings.length));
      const nextSiblings = [
        ...siblings.slice(0, clampedIndex),
        ...movedRows,
        ...siblings.slice(clampedIndex),
      ];
      const renumbered = renumber(nextSiblings);

      // Compose the new collections array: every collection either
      // stays as-is (outside the new parent's siblings), or is one
      // of the renamed rows, OR is a descendant of a moved row that
      // needs its `moduleId` updated (the parentCollectionId stays
      // the same — it still points to a collection inside the moved
      // subtree).
      const renumberedById = new Map(renumbered.map((c) => [c.id, c] as const));
      const movedIdSet = idSet;
      const next = s.collections.map((c) => {
        const renamed = renumberedById.get(c.id);
        if (renamed) return renamed;
        if (movedIdSet.has(c.id)) {
          // Should not happen — sources are deduped above and either
          // become a renamed row or stay untouched. Defensive.
          return c;
        }
        if (allSubtrees.has(c.id) && c.moduleId !== targetModuleId) {
          // Descendant of a moved subtree: keep parentCollectionId,
          // update moduleId to the new owning module.
          return { ...c, moduleId: targetModuleId };
        }
        return c;
      });

      return { collections: next };
    });
    try {
      await invoke<void>(COMMANDS.collectionsMove, { collectionIds, target });
    } catch (e) {
      console.error('[store] collections_move failed', e);
      set({ collections: prevCollections });
    }
  },

  moveEndpoints: async (endpointIds, target) => {
    const prevEndpoints = get().endpoints;
    set((s) => {
      if (endpointIds.length === 0) return {};
      const idSet = new Set(endpointIds);
      const targetCol = s.collections.find((c) => c.id === target.collectionId);
      if (!targetCol) return {};

      // Project-boundary guard: every source endpoint must be in a
      // collection whose module is in the same project as the target.
      const targetProjectId = s.modules.find((m) => m.id === targetCol.moduleId)
        ?.projectId;
      if (!targetProjectId) return {};
      for (const eid of endpointIds) {
        const src = s.endpoints.find((e) => e.id === eid);
        if (!src) return {};
        const srcCol = s.collections.find((c) => c.id === src.collectionId);
        if (!srcCol) return {};
        const srcProjectId = s.modules.find((m) => m.id === srcCol.moduleId)?.projectId;
        if (srcProjectId !== targetProjectId) return {};
      }

      // The destination siblings: everything in targetCol except
      // the moved ones.
      const destSiblings = s.endpoints
        .filter(
          (e) => e.collectionId === target.collectionId && !idSet.has(e.id),
        )
        .sort((a, b) => a.sortOrder - b.sortOrder);

      const movedRows = endpointIds
        .map((id) => s.endpoints.find((e) => e.id === id))
        .filter((e): e is Endpoint => Boolean(e))
        .map((e) => ({ ...e, collectionId: target.collectionId }));

      const clampedIndex = Math.max(0, Math.min(target.index, destSiblings.length));
      const nextDest = [
        ...destSiblings.slice(0, clampedIndex),
        ...movedRows,
        ...destSiblings.slice(clampedIndex),
      ];
      const renumberedDest = renumber(nextDest);
      const renumberedById = new Map(renumberedDest.map((e) => [e.id, e] as const));

      const next = s.endpoints.map((e) => {
        const renamed = renumberedById.get(e.id);
        if (renamed) return renamed;
        if (idSet.has(e.id)) {
          // Defensive: should already be in the renamed map.
          return { ...e, collectionId: target.collectionId };
        }
        return e;
      });

      return { endpoints: next };
    });
    try {
      await invoke<void>(COMMANDS.endpointsMove, { endpointIds, target });
    } catch (e) {
      console.error('[store] endpoints_move failed', e);
      set({ endpoints: prevEndpoints });
    }
  },

  // ---------- environment (async — Tauri commands) ----------

  setEnvironmentVar: async (envId, key, value) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const optimistic: Environment = {
      ...prev,
      variables: { ...prev.variables, [key]: value },
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    try {
      await invoke<Environment>(COMMANDS.environmentsSetVar, { envId, key, value });
    } catch (e) {
      console.error('[store] environments_set_var failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  addEnvironment: async (projectId, name) => {
    try {
      const env = await invoke<Environment>(COMMANDS.environmentsCreate, { projectId, name });
      set((s) => ({ environments: [...s.environments, env] }));
      return env.id;
    } catch (e) {
      console.error('[store] environments_create failed', e);
      return null;
    }
  },
  updateEnvironment: async (envId, patch) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const optimistic: Environment = { ...prev, ...patch };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    try {
      await invoke<Environment>(COMMANDS.environmentsUpdate, { envId, patch });
    } catch (e) {
      console.error('[store] environments_update failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  setEnvironmentBaseUrl: async (envId, defId, url) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const nextBaseUrls = { ...prev.baseUrls };
    if (url) nextBaseUrls[defId] = url;
    else delete nextBaseUrls[defId];
    const optimistic: Environment = { ...prev, baseUrls: nextBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    try {
      await invoke<Environment>(COMMANDS.environmentsSetBaseurl, { envId, defId, url });
    } catch (e) {
      console.error('[store] environments_set_baseurl failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  deleteEnvironment: async (envId) => {
    const snapshot = {
      environments: get().environments,
      endpoints: get().endpoints,
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      // Clear `environmentId` on any endpoint that pointed at the deleted
      // env so it falls back to the project default. We don't have a
      // selector that re-resolves baseUrl on the fly, so the explicit
      // null-out is what keeps the URL bar honest.
      return {
        environments: s.environments.filter((env) => env.id !== envId),
        endpoints: s.endpoints.map((ep) =>
          ep.environmentId === envId ? { ...ep, environmentId: null } : ep,
        ),
        // If the active environment was the one we just deleted, fall
        // back to the next sibling (or the project default).
        activeEnvironmentId:
          s.activeEnvironmentId === envId ? '' : s.activeEnvironmentId,
      };
    });
    try {
      await invoke<void>(COMMANDS.environmentsDelete, { envId });
    } catch (e) {
      console.error('[store] environments_delete failed', e);
      set(snapshot);
    }
  },
  setEnvironmentActive: async (envId) => {
    const prev = get().environments;
    set((s) => {
      const target = s.environments.find((env) => env.id === envId);
      if (!target) return {};
      return {
        activeEnvironmentId: envId,
        environments: s.environments.map((env) => {
          if (env.projectId !== target.projectId) return env;
          return { ...env, isActive: env.id === envId };
        }),
      };
    });
    try {
      await invoke<Environment>(COMMANDS.environmentsSetActive, { envId });
    } catch (e) {
      console.error('[store] environments_set_active failed', e);
      set({ environments: prev });
    }
  },

  // ---------- baseUrl definitions (project-scoped) ----------
  /**
   * Optimistic local add + background invoke. Returns the temp id
   * synchronously because EnvironmentSection passes it directly into
   * `setEnvironmentBaseUrl` to seed an empty row. On invoke success
   * the temp row is replaced with the persisted entity (server may
   * assign a different id); on failure the local row is removed.
   */
  addBaseUrlDefinition: (projectId, name) => {
    const defId = uid();
    set((s) => ({
      projects: s.projects.map((p) => {
        if (p.id !== projectId) return p;
        const wasEmpty = p.baseUrlDefinitions.length === 0;
        return {
          ...p,
          baseUrlDefinitions: [
            ...p.baseUrlDefinitions,
            { id: defId, name: name.trim() || '未命名' },
          ],
          // Convenience: when the project had no defs yet, the newly
          // added one becomes the default — otherwise the picker
          // would have nothing to show as "项目默认".
          defaultBaseUrlDefinitionId: wasEmpty
            ? defId
            : p.defaultBaseUrlDefinitionId,
        };
      }),
    }));
    invoke<BaseUrlDefinition>(COMMANDS.baseUrlDefsCreate, { projectId, name: name.trim() || '未命名' })
      .then((persisted) => {
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.map((d) =>
                d.id === defId ? persisted : d,
              ),
            };
          }),
        }));
      })
      .catch((e) => {
        console.error('[store] base_url_defs_create failed', e);
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
            };
          }),
        }));
      });
    return defId;
  },
  updateBaseUrlDefinition: async (projectId, defId, patch) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    const prevDef = prevProject?.baseUrlDefinitions.find((d) => d.id === defId);
    if (!prevProject || !prevDef) return;
    const optimistic: Project = {
      ...prevProject,
      baseUrlDefinitions: prevProject.baseUrlDefinitions.map((d) =>
        d.id === defId ? { ...d, ...patch } : d,
      ),
    };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    try {
      await invoke<BaseUrlDefinition>(COMMANDS.baseUrlDefsUpdate, { projectId, defId, patch });
    } catch (e) {
      console.error('[store] base_url_defs_update failed', e);
      set((s) => ({
        projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
      }));
    }
  },
  deleteBaseUrlDefinition: async (projectId, defId) => {
    const snapshot = {
      projects: get().projects,
      endpoints: get().endpoints,
    };
    set((s) => {
      // Cascade: any endpoint that pointed at this def falls back to
      // the project default. We don't bother removing the now-orphan
      // entry from each env's baseUrls map — the resolution function
      // never reads an entry under a dead defId, so the stale data
      // is harmless and self-cleans on next save.
      return {
        projects: s.projects.map((p) => {
          if (p.id !== projectId) return p;
          return {
            ...p,
            baseUrlDefinitions: p.baseUrlDefinitions.filter((d) => d.id !== defId),
            defaultBaseUrlDefinitionId:
              p.defaultBaseUrlDefinitionId === defId
                ? null
                : p.defaultBaseUrlDefinitionId,
          };
        }),
        endpoints: s.endpoints.map((e) => {
          if (e.baseUrlDefinitionId === defId) {
            return { ...e, baseUrlDefinitionId: null };
          }
          return e;
        }),
      };
    });
    try {
      await invoke<void>(COMMANDS.baseUrlDefsDelete, { projectId, defId });
    } catch (e) {
      console.error('[store] base_url_defs_delete failed', e);
      set(snapshot);
    }
  },
  setDefaultBaseUrlDefinition: async (projectId, defId) => {
    const prevProject = get().projects.find((p) => p.id === projectId);
    if (!prevProject) return;
    const optimistic: Project = { ...prevProject, defaultBaseUrlDefinitionId: defId };
    set((s) => ({
      projects: s.projects.map((p) => (p.id === projectId ? optimistic : p)),
    }));
    try {
      await invoke<Project>(COMMANDS.baseUrlDefsSetDefault, { projectId, defId });
    } catch (e) {
      console.error('[store] base_url_defs_set_default failed', e);
      set((s) => ({
        projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
      }));
    }
  },

  // ---------- response history & examples ----------

  addResponseHistory: (endpointId, response) =>
    set((s) => {
      const prev = s.responseHistory[endpointId] ?? [];
      // FIFO cap: drop the oldest entry when over the limit.
      const next = prev.length >= MAX_HISTORY_PER_ENDPOINT
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
      const id = await state.addModule(state.activeProjectId, name, {
        description: draft.description,
      });
      set({ lastCreatedId: id });
      return id;
    }

    if (draft.type === 'collection') {
      if (!draft.parentModuleId) return null;
      const mod = state.modules.find((m) => m.id === draft.parentModuleId);
      if (mod && !mod.expanded) state.toggleModuleExpanded(mod.id);
      const id = await state.addCollection(draft.parentModuleId, name, {
        description: draft.description,
      });
      set({ lastCreatedId: id });
      return id;
    }

    // draft.type === 'endpoint'
    if (!draft.parentCollectionId) return null;
    const col = state.collections.find((c) => c.id === draft.parentCollectionId);
    if (col) {
      if (!col.expanded) state.toggleCollectionExpanded(col.id);
      const mod = state.modules.find((m) => m.id === col.moduleId);
      if (mod && !mod.expanded) state.toggleModuleExpanded(mod.id);
    }
    const id = await state.addEndpoint(draft.parentCollectionId, name);
    if (id) state.setActiveEndpoint(id);
    set({ lastCreatedId: id });
    return id;
  },
  clearLastCreated: () => set({ lastCreatedId: null }),

  // ---------- MCP daemon polling ----------
  // Three collaborators participate in the singleton 3s tick:
  //   - `useMcpStatus` calls start()/stop() in its mount/cleanup
  //   - `McpStatusPanel` owns the `daemon://crashed` event listener,
  //     which force-refreshes via `refreshMcpStatus` on a crash.
  //   - `ConnectionSection` calls `refreshMcpStatus` after a manual
  //     restart so the badge flips back to `running` immediately.
  startMcpPolling: () => {
    const state = get();
    if (state._mcpTimer !== null) return; // idempotent — only one timer ever
    const tick = () => {
      void state.refreshMcpStatus();
    };
    const timer = window.setInterval(tick, MCP_REFRESH_INTERVAL_MS);
    set({ _mcpTimer: timer });
    // Kick off the first read immediately so the UI doesn't sit on
    // `mcpStatus: null` for up to 3s after the panel mounts.
    tick();

    // Pause while the tab is hidden — there's nothing to display and
    // the daemon doesn't change between wake-ups. The listener is
    // owned by the store (module-scoped handler ref) so every call to
    // start() doesn't add a new one and stop() can detach cleanly.
    if (typeof document !== 'undefined' && _getVisibilityHandler() === null) {
      const handler = () => {
        if (document.visibilityState === 'hidden') {
          // Pause: clear but keep the handler ref so start() can
          // resume from the visibilitychange→'visible' branch.
          const cur = get();
          if (cur._mcpTimer !== null) {
            window.clearInterval(cur._mcpTimer);
            set({ _mcpTimer: null });
          }
        } else if (document.visibilityState === 'visible') {
          // Resume — start() is idempotent so we can call it freely.
          get().startMcpPolling();
        }
      };
      document.addEventListener('visibilitychange', handler);
      _setVisibilityHandler(handler);
    }
  },
  stopMcpPolling: () => {
    const state = get();
    if (state._mcpTimer !== null) {
      window.clearInterval(state._mcpTimer);
    }
    // Detach visibility listener — only the last unmounted consumer
    // should tear it down, but it's idempotent so calling it here
    // is safe.
    if (typeof document !== 'undefined') {
      const handler = _getVisibilityHandler();
      if (handler) {
        document.removeEventListener('visibilitychange', handler);
        _setVisibilityHandler(null);
      }
    }
    set({ _mcpTimer: null });
  },
  refreshMcpStatus: async () => {
    try {
      const dto = await invoke<McpUiState>(COMMANDS.mcpStatus);
      set({ mcpStatus: dto });
    } catch (e) {
      // Daemon unreachable (cold start, dev mode without the daemon
      // supervisor, or an IPC race) — log and keep whatever the last
      // good state was. We deliberately do NOT clobber `mcpStatus`
      // with a "stopped" snapshot, because that would mask a real
      // daemon crash with a transient IPC failure.
      console.warn('[store] mcp_status poll failed', e);
    }
  },
  restartMcp: async () => {
    // The renderer's role here is to flip the badge to "restarting"
    // immediately, fire the command, then refresh so the new PID /
    // status is visible. The UI button's `disabled` state is the
    // caller's job (it tracks in-flight via local state because the
    // round-trip can be 100ms–2s and we want to disable the moment
    // the user clicks).
    const prev = get().mcpStatus;
    set({
      mcpStatus: prev
        ? { ...prev, status: 'restarting' }
        : {
            status: 'restarting',
            pid: null,
            port: null,
            uptimeMs: 0,
            requestsHandled: 0,
            currentConnections: 0,
            registeredTools: [],
          },
    });
    try {
      await invoke<void>(COMMANDS.mcpDaemonRestart);
      await get().refreshMcpStatus();
    } catch (e) {
      console.error('[store] mcp_daemon_restart failed', e);
      // Surface the failure on the badge — the dialog reads lastError.
      const cur = get().mcpStatus;
      const lastError = e instanceof Error ? e.message : String(e);
      set({
        mcpStatus: cur
          ? { ...cur, status: 'crashed', lastError }
          : {
              status: 'crashed',
              pid: null,
              port: null,
              uptimeMs: 0,
              requestsHandled: 0,
              currentConnections: 0,
              registeredTools: [],
              lastError,
            },
      });
      throw e;
    }
  },
  markRestartDialogSeen: (status) => {
    set({ restartDialogSeen: status });
  },
}));

// ---------- selectors ----------
// IMPORTANT: every selector below returns a raw state reference (or a single
// `.find()` result). Do not add selectors that allocate new arrays/objects —
// that will trigger infinite re-renders under Zustand v5.

/**
 * Currently active project, or `undefined` if `bootstrap()` hasn't run yet
 * (e.g. the very first render before the daemon data arrives). Callers
 * must tolerate `undefined` and render a loading state.
 */
export const useActiveProject = () =>
  useAppStore((s) => s.projects.find((p) => p.id === s.activeProjectId));

/**
 * The currently active workspace tab (raw reference), or `null` when no
 * tab is open. Returned as the `OpenTab` object so consumers can switch
 * on `kind` to pick the right renderer.
 */
export const useActiveTab = () =>
  useAppStore((s) => s.openTabs.find((t) => t.id === s.activeTabId) ?? null);

export const useActiveEnvironment = () =>
  useAppStore((s) => s.environments.find((e) => e.id === s.activeEnvironmentId));

export const useActiveEndpoint = () =>
  useAppStore((s) => {
    // If the active tab is an endpoint, prefer that id — it stays in sync
    // with `setActiveEndpoint` (see setActiveEndpoint below) and reflects
    // the user's actual selection even when there are stale references.
    const activeTab = s.openTabs.find((t) => t.id === s.activeTabId);
    const id = activeTab?.kind === 'endpoint' ? activeTab.id : s.activeEndpointId;
    return s.endpoints.find((e) => e.id === id) ?? null;
  });
