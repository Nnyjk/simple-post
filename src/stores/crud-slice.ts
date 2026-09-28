/**
 * CRUD slice — every Tauri-mediated write that mutates projects,
 * modules, collections, endpoints, environments, and baseUrl
 * definitions.
 *
 * All write actions follow the same shape:
 *   1. Optimistic local mutation via `set()` so the UI updates
 *      immediately.
 *   2. Fire-and-forget (or awaited) `invokeCmd<...>(COMMANDS.*)`.
 *   3. On failure, snapshot+rollback the local state and log.
 *
 * The slice also owns the small pure helpers that the move/delete
 * actions share (`renumber`, `reorderByIndex`,
 * `indexCollectionChildren`, `collectCollectionSubtree`,
 * `idsToCloseForDelete`, `filterTabsAfterRemoval`). They live here
 * because nothing outside this slice needs them — putting them in a
 * shared "utils" file would be premature factoring.
 *
 * The slice reads `loaded`, `setActive*`, etc. from the rest of the
 * store via `get()` — that's fine because every slice shares the
 * same Zustand backing store. We do NOT import any other slice
 * module directly (the `AppState` type is the only coupling).
 */
import type { StateCreator } from 'zustand';
import type {
  Project,
  Module,
  Collection,
  Endpoint,
  Environment,
  HttpMethod,
  BaseUrlDefinition,
} from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { uid } from '@/lib/utils';
import type { OpenTab } from './store-types';
import type { AppState } from './app-store';

/** State portion owned by the CRUD slice — empty; all state is in bootstrap/ui/dirty/mcp. */
export interface CrudSliceState {
  // Intentionally empty. CRUD slice only contributes actions. The
  // type alias exists so that `AppState & CrudSlice` still resolves.
  _crudSliceMarker?: never;
}

/** Action portion owned by the CRUD slice. */
export interface CrudSliceActions {
  // actions: data — CRUD (async — all hit Tauri commands, mirror to local state)
  addProject: (
    name: string,
    opts?: { description?: string; baseUrl?: string; color?: string },
  ) => Promise<string | null>;
  updateProject: (id: string, patch: Partial<Omit<Project, 'id' | 'createdAt'>>) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;

  addModule: (
    projectId: string,
    name: string,
    opts?: { description?: string },
  ) => Promise<string | null>;
  updateModule: (id: string, patch: Partial<Omit<Module, 'id' | 'projectId'>>) => Promise<void>;
  deleteModule: (id: string) => Promise<void>;

  addCollection: (
    moduleId: string,
    name: string,
    opts?: { description?: string; parentCollectionId?: string | null },
  ) => Promise<string | null>;
  updateCollection: (
    id: string,
    patch: Partial<Omit<Collection, 'id' | 'moduleId' | 'parentCollectionId'>>,
  ) => Promise<void>;
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
  moveModule: (moduleId: string, targetIndex: number) => Promise<void>;
  moveCollections: (
    collectionIds: string[],
    target: {
      parentKind: 'module' | 'collection';
      parentId: string;
      index: number;
    },
  ) => Promise<void>;
  moveEndpoints: (
    endpointIds: string[],
    target: { collectionId: string; index: number },
  ) => Promise<void>;

  // actions: environment (async — Tauri commands)
  setEnvironmentVar: (envId: string, key: string, value: string | null) => Promise<void>;
  addEnvironment: (projectId: string, name: string) => Promise<string | null>;
  updateEnvironment: (envId: string, patch: Partial<Pick<Environment, 'name'>>) => Promise<void>;
  setEnvironmentBaseUrl: (envId: string, defId: string, url: string | null) => Promise<void>;
  deleteEnvironment: (envId: string) => Promise<void>;
  setEnvironmentActive: (envId: string) => Promise<void>;

  // actions: baseUrl definitions (project-scoped)
  addBaseUrlDefinition: (projectId: string, name: string) => string;
  updateBaseUrlDefinition: (
    projectId: string,
    defId: string,
    patch: Partial<Pick<BaseUrlDefinition, 'name' | 'description'>>,
  ) => Promise<void>;
  deleteBaseUrlDefinition: (projectId: string, defId: string) => Promise<void>;
  setDefaultBaseUrlDefinition: (projectId: string, defId: string | null) => Promise<void>;
}

export type CrudSlice = CrudSliceState & CrudSliceActions;

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
function indexCollectionChildren(
  collections: readonly Collection[],
): Map<string | null, Collection[]> {
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
    return {
      project: [targetId],
      module: moduleIds,
      collection: collectionIds,
      endpoint: endpointIds,
    };
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
    const fallback = tabs[oldIdx - 1] ?? tabs[oldIdx + 1] ?? nextTabs[nextTabs.length - 1] ?? null;
    nextActive = fallback ? fallback.id : null;
  }
  return { tabs: nextTabs, activeId: nextActive, changed: true };
}

export const createCrudSlice: StateCreator<AppState, [], [], CrudSlice> = (set, get) => ({
  // ---------- project CRUD (async — Tauri commands) ----------

  addProject: async (name, opts) => {
    try {
      const project = await invokeCmd<Project>(COMMANDS.projectsCreate, {
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
      await invokeCmd<Project>(COMMANDS.projectsUpdate, { id, patch });
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
        activeProjectId:
          s.activeProjectId === id
            ? (s.projects.find((p) => p.id !== id)?.id ?? s.activeProjectId)
            : s.activeProjectId,
        activeEndpointId: nextActiveEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invokeCmd<void>(COMMANDS.projectsDelete, { id });
    } catch (e) {
      console.error('[store] projects_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- module CRUD (async — Tauri commands) ----------

  addModule: async (projectId, name, opts) => {
    try {
      const mod = await invokeCmd<Module>(COMMANDS.modulesCreate, {
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
      await invokeCmd<Module>(COMMANDS.modulesUpdate, { id, patch });
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
      await invokeCmd<void>(COMMANDS.modulesDelete, { id });
    } catch (e) {
      console.error('[store] modules_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- collection CRUD ----------

  addCollection: async (moduleId, name, opts) => {
    try {
      const collection = await invokeCmd<Collection>(COMMANDS.collectionsCreate, {
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
      await invokeCmd<Collection>(COMMANDS.collectionsUpdate, { id, patch });
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
      const droppedEndpointIds = s.endpoints.filter((e) => e.collectionId === id).map((e) => e.id);
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
      await invokeCmd<void>(COMMANDS.collectionsDelete, { id });
    } catch (e) {
      console.error('[store] collections_delete failed', e);
      set(snapshot);
    }
  },

  // ---------- endpoint CRUD ----------

  addEndpoint: async (collectionId, name, opts) => {
    try {
      const endpoint = await invokeCmd<Endpoint>(COMMANDS.endpointsCreate, {
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
      await invokeCmd<Endpoint>(COMMANDS.endpointsUpdate, { id, patch });
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
      await invokeCmd<void>(COMMANDS.endpointsDelete, { id });
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
    invokeCmd<Endpoint>(COMMANDS.endpointsDuplicate, { id })
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
      await invokeCmd<void>(COMMANDS.modulesMove, { moduleId, targetIndex });
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
      const targetParentId = target.parentKind === 'module' ? null : target.parentId;

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
      await invokeCmd<void>(COMMANDS.collectionsMove, { collectionIds, target });
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
      const targetProjectId = s.modules.find((m) => m.id === targetCol.moduleId)?.projectId;
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
        .filter((e) => e.collectionId === target.collectionId && !idSet.has(e.id))
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
      await invokeCmd<void>(COMMANDS.endpointsMove, { endpointIds, target });
    } catch (e) {
      console.error('[store] endpoints_move failed', e);
      set({ endpoints: prevEndpoints });
    }
  },

  // ---------- environment (async — Tauri commands) ----------

  setEnvironmentVar: async (envId, key, value) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    const variables = { ...prev.variables };
    if (value === null) delete variables[key];
    else variables[key] = value;
    const optimistic: Environment = {
      ...prev,
      variables,
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    try {
      await invokeCmd<Environment>(COMMANDS.environmentsSetVar, { envId, key, value });
    } catch (e) {
      console.error('[store] environments_set_var failed', e);
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? prev : env)),
      }));
    }
  },
  addEnvironment: async (projectId, name) => {
    try {
      const env = await invokeCmd<Environment>(COMMANDS.environmentsCreate, { projectId, name });
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
      await invokeCmd<Environment>(COMMANDS.environmentsUpdate, { id: envId, patch });
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
    if (url === null) delete nextBaseUrls[defId];
    else nextBaseUrls[defId] = url;
    const optimistic: Environment = { ...prev, baseUrls: nextBaseUrls };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));
    try {
      await invokeCmd<Environment>(COMMANDS.environmentsSetBaseurl, { envId, defId, url });
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
        activeEnvironmentId: s.activeEnvironmentId === envId ? '' : s.activeEnvironmentId,
      };
    });
    try {
      await invokeCmd<void>(COMMANDS.environmentsDelete, { id: envId });
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
      await invokeCmd<Environment>(COMMANDS.environmentsSetActive, { envId });
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
          defaultBaseUrlDefinitionId: wasEmpty ? defId : p.defaultBaseUrlDefinitionId,
        };
      }),
    }));
    invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsCreate, {
      projectId,
      name: name.trim() || '未命名',
    })
      .then((persisted) => {
        set((s) => ({
          projects: s.projects.map((p) => {
            if (p.id !== projectId) return p;
            return {
              ...p,
              baseUrlDefinitions: p.baseUrlDefinitions.map((d) => (d.id === defId ? persisted : d)),
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
      await invokeCmd<BaseUrlDefinition>(COMMANDS.baseUrlDefsUpdate, { id: defId, patch });
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
              p.defaultBaseUrlDefinitionId === defId ? null : p.defaultBaseUrlDefinitionId,
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
      await invokeCmd<void>(COMMANDS.baseUrlDefsDelete, { id: defId });
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
      await invokeCmd<Project>(COMMANDS.baseUrlDefsSetDefault, { projectId, defId });
    } catch (e) {
      console.error('[store] base_url_defs_set_default failed', e);
      set((s) => ({
        projects: s.projects.map((p) => (p.id === projectId ? prevProject : p)),
      }));
    }
  },
});
