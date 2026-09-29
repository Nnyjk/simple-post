/**
 * CRUD slice — every Tauri-mediated write that mutates projects, the
 * resource tree, environments, and baseUrl definitions.
 *
 * The tree is one `nodes` array plus an `endpointDetails` map, so the
 * three action families the old three-table layout needed collapse into
 * three actions total:
 *
 *   addNode / updateNode / deleteNode  (one cascade delete, not three)
 *   moveNodes                          (one move, not three)
 *   updateEndpointDetail               (configuration, kept separate)
 *
 * A delete no longer needs to know what it is deleting: `nodes.parent_id`
 * cascades in the database, and the local write removes the subtree the
 * same way. A move no longer needs a `parentKind`, because the parent's
 * own `type` is what decides where a node may land.
 *
 * All write actions follow the same shape:
 *   1. Optimistic local mutation via `set()` so the UI updates
 *      immediately.
 *   2. Fire-and-forget (or awaited) `invokeCmd<...>(COMMANDS.*)`.
 *   3. On failure, snapshot+rollback the local state and log.
 *
 * The slice reads `loaded`, `setActive*`, etc. from the rest of the
 * store via `get()` — that's fine because every slice shares the
 * same Zustand backing store. We do NOT import any other slice
 * module directly (the `AppState` type is the only coupling).
 */
import type { StateCreator } from 'zustand';
import type {
  Project,
  ResourceNode,
  ResourceNodeType,
  EndpointDetail,
  Environment,
  BaseUrlDefinition,
} from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { indexByParent } from '@/lib/tree-index';
import { uid } from '@/lib/utils';
import type { OpenTab } from './store-types';
import type { AppState } from './app-store';

/** State portion owned by the CRUD slice — empty; all state is in bootstrap/ui/dirty/mcp. */
export interface CrudSliceState {
  // Intentionally empty. CRUD slice only contributes actions. The
  // type alias exists so that `AppState & CrudSlice` still resolves.
  _crudSliceMarker?: never;
}

/** What a caller may set when creating a node. The id is ours to mint. */
export interface NewNodeInput {
  /** Owning project. Required for a module; derived from the parent otherwise. */
  rootId?: string;
  /** `null` (or omitted) puts a module at the top of its project. */
  parentId?: string | null;
  type: ResourceNodeType;
  name: string;
  description?: string;
  /** Endpoint-only, and only meaningful when `type` is `'endpoint'`. */
  detail?: Partial<Omit<EndpointDetail, 'nodeId'>>;
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

  /**
   * Create a tree node of any type. `detail` rides along for an
   * endpoint: the node and its configuration are written in one
   * transaction, so an endpoint can never exist without a url.
   */
  addNode: (input: NewNodeInput) => Promise<string | null>;
  /**
   * Rename / re-describe / reorder one node. Moving is `moveNodes` —
   * it has to keep the type matrix and the subtree's `rootId` honest,
   * which a plain field write cannot.
   */
  updateNode: (
    id: string,
    patch: Partial<Pick<ResourceNode, 'name' | 'description' | 'sortOrder'>>,
  ) => Promise<void>;
  /**
   * Delete a node and everything under it. One action for all three
   * types: the cascade lives in the schema, and the local write mirrors
   * it by dropping the same subtree.
   */
  deleteNode: (id: string) => Promise<void>;

  /** Patch an endpoint's request configuration. */
  updateEndpointDetail: (
    nodeId: string,
    patch: Partial<Omit<EndpointDetail, 'nodeId'>>,
  ) => Promise<void>;

  /**
   * Duplicate an endpoint. Returns the new id **synchronously** because
   * the caller (ProjectTree) immediately activates the new tab — making
   * this async would break the existing UX. Implementation: optimistic
   * local copy first, then background invoke to persist; on invoke
   * success the temp id is replaced with the persisted node, on failure
   * the optimistic copy is removed.
   */
  duplicateEndpoint: (id: string) => string | null;

  // actions: data — move (drag-and-drop reorder + cross-container moves)
  /**
   * Move one or many nodes under `parentId` at sibling slot `index`.
   *
   * `parentId: null` means "top of the project" and is legal only for a
   * module. `nodeIds` is in the order the nodes should land, so a
   * multi-select drag keeps the user's pick order.
   */
  moveNodes: (nodeIds: string[], parentId: string | null, index: number) => Promise<void>;

  // actions: environment (async — Tauri commands)
  setEnvironmentVar: (envId: string, key: string, value: string | null) => Promise<void>;
  renameEnvironmentVar: (envId: string, fromKey: string, toKey: string) => Promise<void>;
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

/**
 * A node and every descendant beneath it.
 *
 * BFS over `parentId` with a visited set. The set is not defensive
 * padding: a cycle in the data would otherwise spin here forever, and
 * the UI is the only thing standing between the store and a
 * hand-edited `store.db`.
 */
function collectSubtree(
  rootId: string,
  childrenIndex: Map<string | null, ResourceNode[]>,
): Set<string> {
  const out = new Set<string>();
  const queue: string[] = [rootId];
  while (queue.length > 0) {
    const id = queue.shift()!;
    if (out.has(id)) continue;
    out.add(id);
    for (const kid of childrenIndex.get(id) ?? []) queue.push(kid.id);
  }
  return out;
}

/**
 * Drop tabs whose underlying entity was removed. Returns the new tab list,
 * the new active tab id (picking the previous neighbor if the active one
 * was removed), and a flag indicating whether anything changed.
 */
function filterTabsAfterRemoval(
  tabs: readonly OpenTab[],
  activeId: string | null,
  closeSet: ReadonlySet<string>,
): { tabs: OpenTab[]; activeId: string | null; changed: boolean } {
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

/**
 * The tab keys to close when a subtree is deleted.
 *
 * A tab is keyed by the id of whatever it shows, and a `module` /
 * `collection` / `endpoint` tab's id is its node id — so one pass over
 * the removed ids answers all three at once. Projects are separate:
 * they are not nodes, so they are closed by the project action.
 */
function tabKeysForSubtree(
  nodes: readonly ResourceNode[],
  removed: ReadonlySet<string>,
  scope: 'project' | 'node',
  targetId: string,
): Set<string> {
  const close = new Set<string>();
  for (const n of nodes) {
    if (removed.has(n.id)) close.add(`${n.type}:${n.id}`);
  }
  if (scope === 'project') close.add(`project:${targetId}`);
  return close;
}

/** The `EndpointDetail` an optimistic endpoint create starts from. */
function blankDetail(
  nodeId: string,
  partial?: Partial<Omit<EndpointDetail, 'nodeId'>>,
): EndpointDetail {
  return {
    nodeId,
    method: 'GET',
    url: '',
    environmentId: null,
    baseUrlDefinitionId: null,
    baseUrlOverride: null,
    params: [],
    headers: [],
    body: { mode: 'none', content: '' },
    auth: { type: 'none' },
    tags: [],
    ...partial,
  };
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
      nodes: get().nodes,
      endpointDetails: get().endpointDetails,
      environments: get().environments,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeProjectId: get().activeProjectId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      // `rootId` is denormalised onto every node precisely so "what
      // belongs to this project" is one filter rather than a walk down
      // module → collection → endpoint.
      const removed = new Set(s.nodes.filter((n) => n.rootId === id).map((n) => n.id));
      const droppedEndpointIds = s.nodes
        .filter((n) => n.type === 'endpoint' && removed.has(n.id))
        .map((n) => n.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const nextDetails = { ...s.endpointDetails };
      for (const eid of droppedEndpointIds) delete nextDetails[eid];
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        tabKeysForSubtree(s.nodes, removed, 'project', id),
      );
      const nextActiveEndpointId =
        s.activeEndpointId && removed.has(s.activeEndpointId) ? null : s.activeEndpointId;
      return {
        projects: s.projects.filter((p) => p.id !== id),
        nodes: s.nodes.filter((n) => !removed.has(n.id)),
        endpointDetails: nextDetails,
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

  // ---------- resource tree (async — Tauri commands) ----------

  addNode: async (input) => {
    // Minted here rather than server-side so the Tauri and REST shapes
    // agree: the REST body needs the id up front to key the endpoint's
    // `detail.nodeId`, and letting the two paths differ would put the
    // divergence in the one operation that writes two tables.
    const id = uid();
    try {
      const node = await invokeCmd<ResourceNode>(COMMANDS.nodesCreate, {
        id,
        rootId: input.rootId ?? null,
        parentId: input.parentId ?? null,
        type: input.type,
        name: input.name,
        description: input.description,
        sortOrder: 0,
        ...(input.detail ? { detail: input.detail } : {}),
      });
      set((s) => ({
        nodes: [...s.nodes, node],
        // Only an endpoint has a detail row; writing an empty one for a
        // module would make `Object.keys(endpointDetails)` lie.
        ...(input.type === 'endpoint'
          ? { endpointDetails: { ...s.endpointDetails, [id]: blankDetail(id, input.detail) } }
          : {}),
      }));
      return node.id;
    } catch (e) {
      console.error(`[store] nodes_create failed (${input.type})`, e);
      return null;
    }
  },

  updateNode: async (id, patch) => {
    const prev = get().nodes.find((n) => n.id === id);
    if (!prev) return;
    const optimistic: ResourceNode = { ...prev, ...patch };
    set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? optimistic : n)) }));
    try {
      await invokeCmd<ResourceNode>(COMMANDS.nodesUpdate, { id, patch });
    } catch (e) {
      console.error('[store] nodes_update failed', e);
      set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? prev : n)) }));
    }
  },

  deleteNode: async (id) => {
    // One delete for all three types. `nodes.parent_id` cascades in the
    // database; the local write drops the same subtree so the tree does
    // not flicker back to a deleted node's children until the next
    // reload.
    const snapshot = {
      nodes: get().nodes,
      endpointDetails: get().endpointDetails,
      responseHistory: get().responseHistory,
      responseExamples: get().responseExamples,
      openTabs: get().openTabs,
      activeTabId: get().activeTabId,
      activeEndpointId: get().activeEndpointId,
    };
    set((s) => {
      const removed = collectSubtree(id, indexByParent(s.nodes));
      const droppedEndpointIds = s.nodes
        .filter((n) => n.type === 'endpoint' && removed.has(n.id))
        .map((n) => n.id);
      const nextHistory = { ...s.responseHistory };
      const nextExamples = { ...s.responseExamples };
      for (const eid of droppedEndpointIds) {
        delete nextHistory[eid];
        delete nextExamples[eid];
      }
      const nextDetails = { ...s.endpointDetails };
      for (const eid of droppedEndpointIds) delete nextDetails[eid];
      const tabs = filterTabsAfterRemoval(
        s.openTabs,
        s.activeTabId,
        tabKeysForSubtree(s.nodes, removed, 'node', id),
      );
      return {
        nodes: s.nodes.filter((n) => !removed.has(n.id)),
        endpointDetails: nextDetails,
        responseHistory: nextHistory,
        responseExamples: nextExamples,
        activeEndpointId:
          s.activeEndpointId && removed.has(s.activeEndpointId) ? null : s.activeEndpointId,
        openTabs: tabs.tabs,
        activeTabId: tabs.activeId,
      };
    });
    try {
      await invokeCmd<void>(COMMANDS.nodesDelete, { id });
    } catch (e) {
      console.error('[store] nodes_delete failed', e);
      set(snapshot);
    }
  },

  updateEndpointDetail: async (nodeId, patch) => {
    const prev = get().endpointDetails[nodeId];
    if (!prev) return;
    const optimistic: EndpointDetail = { ...prev, ...patch };
    set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: optimistic } }));
    try {
      await invokeCmd<EndpointDetail>(COMMANDS.endpointDetailUpdate, { nodeId, patch });
    } catch (e) {
      console.error('[store] endpoint_detail_update failed', e);
      set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: prev } }));
    }
  },

  /**
   * Optimistic duplicate + background invoke. Returns the temp id
   * synchronously so the caller (ProjectTree) can activate the new
   * tab immediately. On invoke success the temp rows are replaced by
   * the persisted ones; on failure they're removed and we log.
   */
  duplicateEndpoint: (id) => {
    const state = get();
    const original = state.nodes.find((n) => n.id === id);
    const originalDetail = state.endpointDetails[id];
    if (!original || !originalDetail) return null;
    const newId = uid();
    const now = Date.now();
    set((s) => {
      // Beside the original, not at the end of the collection: a copy
      // the user cannot see next to what they copied is easy to miss.
      // This matches what `db::nodes::duplicate` does server-side.
      const siblings = s.nodes
        .filter((n) => n.parentId === original.parentId)
        .sort((a, b) => a.sortOrder - b.sortOrder);
      const at = siblings.findIndex((n) => n.id === id);
      const shifted = s.nodes.map((n) =>
        n.parentId === original.parentId && n.sortOrder > original.sortOrder
          ? { ...n, sortOrder: n.sortOrder + 1 }
          : n,
      );
      return {
        nodes: [
          ...shifted,
          {
            ...original,
            id: newId,
            name: `${original.name} (副本)`,
            sortOrder: at + 1,
            createdAt: now,
            updatedAt: now,
          },
        ],
        endpointDetails: {
          ...s.endpointDetails,
          [newId]: { ...originalDetail, nodeId: newId },
        },
      };
    });
    invokeCmd<ResourceNode>(COMMANDS.nodesDuplicate, { id })
      .then((persisted) => {
        // Swap the temp rows with the persisted ones (the server may
        // assign a different id or normalise fields).
        set((s) => {
          const nextNodes = s.nodes.map((n) => (n.id === newId ? persisted : n));
          const nextDetails = { ...s.endpointDetails };
          delete nextDetails[newId];
          nextDetails[persisted.id] = { ...originalDetail, nodeId: persisted.id };
          return { nodes: nextNodes, endpointDetails: nextDetails };
        });
      })
      .catch((e) => {
        console.error('[store] nodes_duplicate failed', e);
        set((s) => {
          const nextDetails = { ...s.endpointDetails };
          delete nextDetails[newId];
          return { nodes: s.nodes.filter((n) => n.id !== newId), endpointDetails: nextDetails };
        });
      });
    return newId;
  },

  // ---------- move (drag-and-drop) ----------

  moveNodes: async (nodeIds, parentId, index) => {
    const prevNodes = get().nodes;
    set((s) => {
      if (nodeIds.length === 0) return {};
      const idSet = new Set(nodeIds);
      const childrenIndex = indexByParent(s.nodes);
      const sources = s.nodes.filter((n) => idSet.has(n.id));
      if (sources.length === 0) return {};

      // The destination parent, and the project it puts the nodes in.
      // `rootId` rides on every node, so the project-boundary check is a
      // string compare instead of the old walk up to the module.
      let targetRootId: string;
      if (parentId === null) {
        // "Top of the project" — legal only for a module.
        if (sources.some((n) => n.type !== 'module')) return {};
        targetRootId = sources[0].rootId;
      } else {
        const targetParent = s.nodes.find((n) => n.id === parentId);
        if (!targetParent) return {};
        targetRootId = targetParent.rootId;
      }

      // Cycle guard: a node cannot be moved under itself or any
      // descendant. The UI disables those drop targets, but a stale
      // drag payload can still name one.
      const movedSubtrees = new Set<string>();
      for (const src of sources) {
        for (const nid of collectSubtree(src.id, childrenIndex)) movedSubtrees.add(nid);
      }
      if (parentId !== null && movedSubtrees.has(parentId)) return {};

      // Every source must already belong to the destination's project.
      // The server refuses the move otherwise; refusing locally keeps
      // the optimistic state from showing something that will 400.
      for (const src of sources) {
        if (src.rootId !== targetRootId) return {};
      }

      // Destination siblings with the movers removed — they are
      // reinserted at `index` below.
      //
      // For a top-level drop the siblings are the destination project's
      // modules, which are the nodes with no parent; scoping by
      // `parentId === null` alone would sweep in every project's
      // top level at once.
      const siblings =
        parentId === null
          ? s.nodes
              .filter((n) => n.parentId === null && n.rootId === targetRootId && !idSet.has(n.id))
              .sort((a, b) => a.sortOrder - b.sortOrder)
          : (childrenIndex.get(parentId) ?? []).filter((n) => !idSet.has(n.id));

      // The moved rows keep the caller's order, so a multi-select drag
      // lands the way the user picked them.
      const movedRows = sources
        .slice()
        .sort((a, b) => nodeIds.indexOf(a.id) - nodeIds.indexOf(b.id))
        .map((n) => ({ ...n, parentId, rootId: targetRootId }));

      const clampedIndex = Math.max(0, Math.min(index, siblings.length));
      const renumbered = renumber([
        ...siblings.slice(0, clampedIndex),
        ...movedRows,
        ...siblings.slice(clampedIndex),
      ]);
      const renumberedById = new Map(renumbered.map((n) => [n.id, n] as const));

      // Every node either keeps its slot (outside the destination), is
      // one of the renumbered rows, or is a descendant of a moved row
      // whose `rootId` has to follow. The last case is the one that used
      // to drift: the old store fixed a collection's `moduleId` here
      // while the database left the descendants alone.
      const next = s.nodes.map((n) => {
        const renamed = renumberedById.get(n.id);
        if (renamed) return renamed;
        if (movedSubtrees.has(n.id) && n.rootId !== targetRootId) {
          return { ...n, rootId: targetRootId };
        }
        return n;
      });

      return { nodes: next };
    });
    try {
      await invokeCmd<void>(COMMANDS.nodesMove, { nodeIds, parentId, index });
    } catch (e) {
      console.error('[store] nodes_move failed', e);
      set({ nodes: prevNodes });
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
  renameEnvironmentVar: async (envId, fromKey, toKey) => {
    const prev = get().environments.find((e) => e.id === envId);
    if (!prev) return;
    if (fromKey === toKey) return;

    // Optimistic single-step swap: the old key disappears and the new
    // one appears in the same state write, so no intermediate render
    // ever shows the variable missing. The value moves with the key.
    const { [fromKey]: value, ...rest } = prev.variables;
    if (value === undefined) return;
    const optimistic: Environment = {
      ...prev,
      variables: { ...rest, [toKey]: value },
    };
    set((s) => ({
      environments: s.environments.map((env) => (env.id === envId ? optimistic : env)),
    }));

    try {
      // One command, one transaction. The previous shape — two
      // `environments_set_var` calls — left the variable deleted for
      // the window between them, and if the second call failed the
      // rollback below restored only the UI while the backend had
      // already committed the delete.
      const updated = await invokeCmd<Environment>(COMMANDS.environmentsRenameVar, {
        envId,
        fromKey,
        toKey,
      });
      // The backend echoes the authoritative post-rename env, so a
      // concurrent edit from another window isn't clobbered by the
      // optimistic guess.
      set((s) => ({
        environments: s.environments.map((env) => (env.id === envId ? updated : env)),
      }));
    } catch (e) {
      console.error('[store] environments_rename_var failed', e);
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
      endpointDetails: get().endpointDetails,
      activeEnvironmentId: get().activeEnvironmentId,
    };
    set((s) => {
      // Clear `environmentId` on any endpoint that pointed at the deleted
      // env so it falls back to the project default. We don't have a
      // selector that re-resolves baseUrl on the fly, so the explicit
      // null-out is what keeps the URL bar honest.
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.environmentId === envId ? { ...d, environmentId: null } : d;
      }
      return {
        environments: s.environments.filter((env) => env.id !== envId),
        endpointDetails: nextDetails,
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
      endpointDetails: get().endpointDetails,
    };
    set((s) => {
      // Cascade: any endpoint that pointed at this def falls back to
      // the project default. We don't bother removing the now-orphan
      // entry from each env's baseUrls map — the resolution function
      // never reads an entry under a dead defId, so the stale data
      // is harmless and self-cleans on next save.
      const nextDetails: Record<string, EndpointDetail> = {};
      for (const [id, d] of Object.entries(s.endpointDetails)) {
        nextDetails[id] = d.baseUrlDefinitionId === defId ? { ...d, baseUrlDefinitionId: null } : d;
      }
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
        endpointDetails: nextDetails,
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
