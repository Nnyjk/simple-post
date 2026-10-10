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
  invokeWithRollback,
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
   * this async would break the existing UX. Implementation: mint the id
   * here, write the copy into the store optimistically, then persist it
   * with that same id; on invoke failure the optimistic copy is removed.
   */
  duplicateEndpoint: (id: string) => string | null;
}

export const createTreeCrud: StateCreator<AppState, [], [], TreeCrudActions> = (set, get) => ({
  addNode: async (input) => {
    // Offered to the backend so the REST path can key the endpoint's
    // `detail.nodeId` off it — but *not* treated as authoritative: see
    // `createdId` below. The earlier comment here claimed both shapes
    // minted their ids here, which was never true of the Tauri command
    // and is what hid this bug.
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
      // Everything downstream keys off the id the *server* handed back,
      // not the one we offered. The REST path honours ours
      // (`ensure_server_id` only fills a missing id) but the Tauri
      // command mints its own `uid()` and ignores the argument, so the
      // two shapes return different ids for the same call. Keying the
      // detail map by our local `id` therefore wrote it under a key no
      // row has: `EndpointList` then skipped the endpoint for want of a
      // detail and the tab resolved to the "已删除" descriptor, while
      // the module's count — which counts nodes, not details — still
      // showed the endpoint. Trusting `node.id` makes both transports
      // agree without teaching the store which one it is talking to.
      const createdId = node.id;
      set((s) => ({
        nodes: [...s.nodes, node],
        // Only an endpoint has a detail row; writing an empty one for a
        // module would make `Object.keys(endpointDetails)` lie.
        ...(input.kind === 'endpoint'
          ? {
              endpointDetails: {
                ...s.endpointDetails,
                [createdId]: blankDetail(createdId, input.detail),
              },
            }
          : {}),
      }));
      return createdId;
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
    await invokeWithRollback(
      'nodes_update',
      () => invokeCmd<ResourceNode>(COMMANDS.nodesUpdate, { id, patch }),
      () => set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? prev : n)) })),
    );
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
    await invokeWithRollback(
      'nodes_delete',
      () => invokeCmd<void>(COMMANDS.nodesDelete, { id }),
      () => set(snapshot),
    );
  },

  updateEndpointDetail: async (nodeId, patch) => {
    const prev = get().endpointDetails[nodeId];
    if (!prev) return;
    const optimistic: EndpointDetail = { ...prev, ...patch };
    set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: optimistic } }));
    await invokeWithRollback(
      'endpoint_detail_update',
      () => invokeCmd<EndpointDetail>(COMMANDS.endpointDetailUpdate, { nodeId, patch }),
      () => set((s) => ({ endpointDetails: { ...s.endpointDetails, [nodeId]: prev } })),
    );
  },

  /**
   * Optimistic duplicate + background invoke. Returns the id
   * synchronously so the caller (ProjectTree) can activate the new tab
   * and start renaming it immediately.
   *
   * The id is minted *here* and sent as `newId`, and the backend adopts
   * it (`db::nodes::duplicate` no longer mints its own). That is the whole
   * contract: when the server used to answer with a different id, the tab
   * strip, `activeEndpointId` and the in-progress rename were all left
   * pointing at the optimistic id — a row nothing owned — and the tab
   * rendered as "接口已删除". `addNode` solves the same problem by
   * trusting the returned id, but a duplicate cannot: its caller has
   * already acted on the id before this call returns.
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
    // `newId` is not a hint — it is the row's id, and the server adopts it
    // (`db::nodes::duplicate` no longer mints its own). So there is nothing
    // to reconcile when this resolves: the optimistic row *is* the row that
    // was written. Swapping in the server's copy used to be the id fix,
    // and once it stopped being one it only overwrote whatever the user
    // had typed into the copy in the meantime. Only a failure has work to
    // do — drop the copy that no longer exists anywhere else.
    invokeCmd<ResourceNode>(COMMANDS.nodesDuplicate, { id, newId }).catch((e) => {
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
