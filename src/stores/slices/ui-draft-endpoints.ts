/**
 * ui-draft-endpoints — the endpoint that exists only in the renderer.
 *
 * WHY A DRAFT IS NOT A NODE
 * -------------------------
 * Creating an endpoint used to write a `nodes` row immediately and open its
 * tab. That made "新建" a commit: a user who clicked it by accident, or who
 * opened one only to poke a URL and press Send, left a permanent `未命名接口`
 * row behind — and the tree gave no way to tell those apart from the ones
 * they meant to keep.
 *
 * A draft reverses that. Nothing reaches SQLite until `saveDraftEndpoint`
 * runs, so the whole create flow (name it, type a URL, Send it, discard it)
 * costs zero writes and is free to abandon. The price is that a draft is not
 * a tree row: it is not in `nodes`, it has no `endpoint_details` row, and no
 * amount of reloading the workspace will produce it. Anything that walks the
 * tree has nothing to walk.
 *
 * THE TWO HALVES OF A DRAFT
 * -------------------------
 * The fields live in two places, and that split is not an accident:
 *
 *   draftEndpoints[id]   whole-document fields — name / method / url / auth /
 *                        environmentId / baseUrl* / tags / docs. They are read
 *                        as a document because a draft has no stored row to
 *                        patch against, so "unset" would otherwise be
 *                        indistinguishable from "unset by default".
 *   endpointDrafts[id]   params / headers / body — the request-config
 *                        editors write here, and they are patch-semantics
 *                        because they are the *same* editors that write a
 *                        saved endpoint's edits.
 *
 * Reusing `endpointDrafts` is what lets the HTTP tester treat a draft and a
 * saved endpoint with one code path: the overlay it draws is identical, so
 * an unsaved endpoint is debuggable with the same UI as a saved one. The
 * save merges the two halves into one `detail` write.
 *
 * FAILURE IS NOT DESTRUCTIVE
 * --------------------------
 * `saveDraftEndpoint` keeps the draft (and its tab, and its pending edits)
 * when the write fails and returns `null`, because the alternative — dropping
 * an endpoint the user may have spent minutes on because one IPC call failed
 * — is the worst outcome available. `addNode` already logs the underlying
 * error and swallows it into `null`, so the message here is necessarily
 * generic; `draftSaveError` exists so the UI can *show* something and offer a
 * retry rather than appearing to do nothing at all.
 */
import type { StateCreator } from 'zustand';
import type { EndpointDetail } from '@/types/domain';
import { uid } from '@/lib/utils';
import type { AppState } from '../app-store';
import { DRAFT_ID_PREFIX, UNTITLED_ENDPOINT_NAME } from '../store-types';
import type { DraftEndpoint } from '../store-types';
import { draftToPatch } from './crud-helpers';

export interface UiDraftEndpointsState {
  /**
   * Every open draft endpoint, keyed by its `draft:`-prefixed id — which is
   * also its tab id, so "the endpoint this tab shows" is one map read.
   */
  draftEndpoints: Record<string, DraftEndpoint>;
  /**
   * Why the last `saveDraftEndpoint` failed, or `null`. Separate from
   * `bootstrapError` / `nodesError` for the same reason those are separate:
   * a failed save must not read as a failed startup or a broken tree.
   */
  draftSaveError: string | null;
}

export interface UiDraftEndpointsActions {
  /**
   * Create a draft endpoint under `parentId` and open its tab. Returns the
   * draft id. Writes nothing — not to SQLite, not to `nodes`.
   *
   * Opening the tab here rather than in the caller is deliberate: a draft
   * only exists to be debugged, and every entry point (the tree menu, the
   * command palette, `createEndpointAt`) wants the same thing from it. The
   * alternative is three call sites each remembering to open one, which is
   * how a draft ends up invisible in the tab strip.
   *
   * The parent is *not* validated here, unlike `createEndpointAt`: this is
   * pure memory, so an unknown parent costs the user nothing until they
   * press Save — at which point the write fails and the draft survives.
   * Validating at the entry point that knows the user's intent is enough.
   */
  createEndpointDraft: (parentId: string) => string;
  /** Patch a draft in place (rename, new url, …). Unknown ids are a no-op. */
  updateDraftEndpoint: (
    draftId: string,
    patch: Partial<Omit<DraftEndpoint, 'id' | 'parentId' | 'createdAt'>>,
  ) => void;
  /**
   * Drop a draft and every trace of its pending edits. Never touches the
   * database — there is nothing there to touch.
   */
  discardDraftEndpoint: (draftId: string) => void;
  /**
   * Persist a draft as a real endpoint. Returns the new endpoint id, or
   * `null` if the draft is gone or the write failed (the draft is kept
   * either way, so the caller can show an error and let the user retry).
   */
  saveDraftEndpoint: (draftId: string) => Promise<string | null>;
}

export type UiDraftEndpoints = UiDraftEndpointsState & UiDraftEndpointsActions;

export const createUiDraftEndpoints: StateCreator<AppState, [], [], UiDraftEndpoints> = (
  set,
  get,
) => ({
  draftEndpoints: {},
  draftSaveError: null,

  createEndpointDraft: (parentId) => {
    const state = get();
    const parent = state.nodes.find((n) => n.id === parentId);
    const id = `${DRAFT_ID_PREFIX}${uid()}`;
    const draft: DraftEndpoint = {
      id,
      parentId,
      // Fall back to the active project so a draft created under a parent
      // the store has not loaded yet still knows where it will land.
      rootId: parent?.rootId ?? state.activeProjectId,
      name: UNTITLED_ENDPOINT_NAME,
      description: '',
      // Same defaults `blankDetail` would have written, so a draft that is
      // saved without a single edit produces a byte-identical endpoint.
      method: 'GET',
      url: '',
      auth: { kind: 'none' },
      environmentId: null,
      baseUrlDefinitionId: null,
      baseUrlOverride: null,
      tags: [],
      docs: '',
      createdAt: Date.now(),
    };
    set((s) => ({
      draftEndpoints: { ...s.draftEndpoints, [id]: draft },
      // Dirty from the moment it exists. A draft has no row in the
      // database and cannot be recovered from any other place, so
      // leaving it clean would let "关闭已保存" — an action whose whole
      // promise is that it only closes what has nothing to lose — throw
      // away work the user cannot get back. The dot is also the honest
      // description: nothing about this tab is saved yet.
      dirtyTabIds: new Set(s.dirtyTabIds).add(id),
    }));
    state.openTab('endpoint-draft', id);
    return id;
  },

  updateDraftEndpoint: (draftId, patch) =>
    set((s) => {
      const current = s.draftEndpoints[draftId];
      // Unknown id: silent no-op. An editor can fire a patch after the tab
      // closed (debounce, blur, an async save landing) and throwing here
      // would take down the tab it was meant to update.
      if (!current) return {};
      return { draftEndpoints: { ...s.draftEndpoints, [draftId]: { ...current, ...patch } } };
    }),

  discardDraftEndpoint: (draftId) =>
    set((s) => {
      const hasDraft = draftId in s.draftEndpoints;
      const hasPatch = draftId in s.endpointDrafts;
      const isDirty = s.dirtyTabIds.has(draftId);
      if (!hasDraft && !hasPatch && !isDirty) return {};
      const draftEndpoints = { ...s.draftEndpoints };
      delete draftEndpoints[draftId];
      // The request-config edits live under the draft id too, and the ids
      // are never reused — leaving them behind is a leak, not a cache.
      const endpointDrafts = { ...s.endpointDrafts };
      delete endpointDrafts[draftId];
      const dirtyTabIds = new Set(s.dirtyTabIds);
      dirtyTabIds.delete(draftId);
      return { draftEndpoints, endpointDrafts, dirtyTabIds };
    }),

  saveDraftEndpoint: async (draftId) => {
    const state = get();
    const draft = state.draftEndpoints[draftId];
    if (!draft) return null;

    // Read the pending request-config edits BEFORE discarding: they are
    // keyed by the draft id and `discardDraftEndpoint` drops them.
    const pending = state.endpointDrafts[draftId];

    // Document fields first, then the patch on top. The patch wins where
    // they overlap (url / method / auth / docs / tags) because those are
    // what the tester has been writing while the document copy went stale,
    // and it is the only source of params / headers / body at all.
    const detail: Partial<Omit<EndpointDetail, 'nodeId'>> = {
      method: draft.method,
      url: draft.url,
      auth: draft.auth,
      environmentId: draft.environmentId,
      baseUrlDefinitionId: draft.baseUrlDefinitionId,
      baseUrlOverride: draft.baseUrlOverride,
      tags: draft.tags,
      docs: draft.docs,
      ...(pending ? draftToPatch(pending) : {}),
    };

    set({ draftSaveError: null });
    // Deliberately no `rootId`: `nodes_create` derives it from the parent,
    // and `db::nodes::create` *rejects* one that disagrees with the
    // parent's. Replaying the snapshot taken when the draft was born would
    // therefore turn "the parent was moved while I was editing" — a real
    // thing to do in a drag-and-drop tree — into a hard validation
    // failure. The derived answer is the correct one by construction.
    const endpointId = await state.addNode({
      parentId: draft.parentId,
      kind: 'endpoint',
      // A draft the user never named must not persist as an empty name.
      name: draft.name.trim() || UNTITLED_ENDPOINT_NAME,
      description: draft.description,
      detail,
    });

    if (!endpointId) {
      // Everything stays: draft, tab, pending edits. `addNode` has already
      // logged the real error — it returns null rather than rethrowing.
      set({ draftSaveError: '接口草稿保存失败，请重试' });
      return null;
    }

    // Order matters. Discard first so the draft is gone even if a later
    // step throws; then swap the tab over. Opening the endpoint tab last
    // means the draft tab is the one `closeTab` retires, and its
    // neighbour-fallback for `activeTabId` is immediately overwritten.
    //
    // `pending` is deliberately NOT carried across. It was already merged
    // into `detail` above, which is the whole content of the write, so
    // re-seeding it under the new id would recreate the very state Save is
    // supposed to end: a `•` on the tab, "关闭已保存" declining to close
    // it, and a second Save that writes nothing new. A saved endpoint's
    // tab is clean — see `HttpTester`'s own `handleSave`, which calls
    // `clearEndpointDraft` for the same reason.
    get().discardDraftEndpoint(draftId);
    get().closeTab(draftId);
    get().openTab('endpoint', endpointId);
    get().setActiveTab(endpointId);
    return endpointId;
  },
});
