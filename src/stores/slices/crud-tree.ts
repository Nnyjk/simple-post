/**
 * CRUD sub-factory — the resource tree.
 *
 * One of five sub-factories composed by `createCrudSlice` (see
 * `../crud-slice.ts` for the slice-wide contract: optimistic `set()` →
 * `invokeCmd` → snapshot-and-rollback).
 *
 * The tree is one `nodes` array plus an `endpointDetails` map, so the
 * three action families the old three-table layout needed collapse into
 * three actions total:
 *
 *   addNode / updateNode / deleteNode  (one cascade delete, not three)
 *   updateEndpointDetail               (configuration, kept separate)
 *
 * A delete no longer needs to know what it is deleting: `nodes.parent_id`
 * cascades in the database, and the local write removes the subtree the
 * same way. A move (see `./crud-move.ts`) no longer needs a
 * `parentKind`, because the parent's own `type` is what decides where a
 * node may land.
 */
import type { StateCreator } from 'zustand';
import type { ResourceNode, ResourceNodeType, EndpointDetail } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import { indexByParent } from '@/lib/tree-index';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import {
  blankDetail,
  collectSubtree,
  filterTabsAfterRemoval,
  tabKeysForSubtree,
} from './crud-helpers';

/** What a caller may set when creating a node. The id is ours to mint. */
export interface NewNodeInput {
  /** Owning project. Required for a module; derived from the parent otherwise. */
  rootId?: string;
  /** `null` (or omitted) puts a module at the top of its project. */
  parentId?: string | null;
  kind: ResourceNodeType;
  name: string;
  description?: string;
  /** Endpoint-only, and only meaningful when `kind` is `'endpoint'`. */
  detail?: Partial<Omit<EndpointDetail, 'nodeId'>>;
}

/** Resource-tree actions owned by the CRUD slice. */
export interface TreeCrudActions {
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
}

export const createTreeCrud: StateCreator<AppState, [], [], TreeCrudActions> = (set, get) => ({
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
        kind: input.kind,
        name: input.name,
        description: input.description,
        sortOrder: 0,
        ...(input.detail ? { detail: input.detail } : {}),
      });
      set((s) => ({
        nodes: [...s.nodes, node],
        // Only an endpoint has a detail row; writing an empty one for a
        // module would make `Object.keys(endpointDetails)` lie.
        ...(input.kind === 'endpoint'
          ? { endpointDetails: { ...s.endpointDetails, [id]: blankDetail(id, input.detail) } }
          : {}),
      }));
      return node.id;
    } catch (e) {
      console.error(`[store] nodes_create failed (${input.kind})`, e);
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
        .filter((n) => n.kind === 'endpoint' && removed.has(n.id))
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
});
