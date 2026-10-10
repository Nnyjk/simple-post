/**
 * ui-new-item — the create-a-thing dialogs' draft, their commit, and the
 * one creation path that has no dialog at all.
 *
 * WHY ENDPOINTS AREN'T IN THE DRAFT
 * ---------------------------------
 * Creating an endpoint no longer asks anything: the click itself creates a
 * named-placeholder **draft** (`未命名接口`), opens its tab in the workspace
 * and drops it straight into rename. A dialog for it would be three questions
 * (name / module / collection) where two of them are already answered by
 * *where you clicked* — so the draft only ever holds the three kinds that
 * genuinely need a form: `NewDraftKind` is project | module | collection,
 * and there is no code path that can put `'endpoint'` into it.
 *
 * `createEndpointAt` is therefore not "the endpoint branch of
 * `submitNewItem`" — it takes an explicit parent node id and never reads
 * the draft, and it writes nothing: the row it stands for is a frontend
 * draft owned by `slices/ui-draft-endpoints.ts`, which only reaches SQLite
 * when the user presses Save. Both paths still share the same first
 * invariant — expand every ancestor so the new row is visible where it
 * landed — but only the dialog path still ends on `setActiveEndpoint`,
 * because only it has a persisted row to activate.
 *
 * The three dialog kinds stay on one `submitNewItem` rather than three
 * separate actions: `lastCreatedId` has to land *after* the create
 * resolves, because the tree watches that id to enter rename mode on the
 * new row. Splitting the branches would put `lastCreatedId` updates
 * behind an extra promise hop for no gain.
 */

import type { StateCreator } from 'zustand';
import type { ResourceNode } from '@/types/domain';
import { PROJECT_COLOR_DEFAULT } from '@/lib/palette';
import type { AppState } from '../app-store';
import type { NewItemDraft, NewDraftKind } from '../store-types';

/**
 * The name an endpoint is born with. Deliberately *not* "新接口": that
 * reads as a final name, while this row exists to be renamed the moment
 * it appears (the tree puts it into inline rename, and the workspace
 * title is editable too).
 */
export const UNTITLED_ENDPOINT_NAME = '未命名接口';

const emptyDraft = (): NewItemDraft => ({
  open: false,
  type: null,
  parentModuleId: '',
  parentCollectionId: '',
  name: '',
  description: '',
  baseUrl: '',
  color: PROJECT_COLOR_DEFAULT,
});

export interface UiNewItemState {
  newItem: NewItemDraft;
  /**
   * Set by `submitNewItem` after a successful create. Consumers (e.g.
   * ProjectTree) watch this via useEffect to drive their own side-effects
   * (e.g. enter inline-rename mode for the freshly created row) and then
   * call `clearLastCreated()` to acknowledge.
   */
  lastCreatedId: string | null;
}

export interface UiNewItemActions {
  openNewItem: (opts?: {
    type?: NewDraftKind;
    parentModuleId?: string;
    parentCollectionId?: string;
  }) => void;
  closeNewItem: () => void;
  setNewItemType: (type: NewDraftKind) => void;
  setNewItemName: (n: string) => void;
  setNewItemDescription: (d: string) => void;
  setNewItemBaseUrl: (u: string) => void;
  setNewItemColor: (c: string) => void;
  setNewItemParentModule: (id: string) => void;
  setNewItemParentCollection: (id: string) => void;
  /**
   * Commit the in-memory `newItem` draft by calling the corresponding
   * store action. Async because the underlying add* calls hit the Tauri
   * daemon. The create dialogs call this fire-and-forget — they don't
   * need the id synchronously.
   */
  submitNewItem: () => Promise<string | null>;
  /**
   * Create an endpoint under `parentId` — a module or a collection —
   * without a dialog, and open it in the workspace.
   *
   * `parentId` is the node you clicked, so no parent *choice* is left to
   * make: an endpoint may sit directly under a module, which makes the
   * module a complete parent on its own.
   *
   * CONTRACT CHANGE — the draft is not in `nodes`.
   * This used to `addNode` immediately and return a real row id. It now
   * builds a frontend draft (`createEndpointDraft` → `slices/
   * ui-draft-endpoints.ts`) and returns the `draft:`-prefixed id; nothing
   * reaches SQLite until `saveDraftEndpoint`. Consequences the tree side
   * has to own:
   *   - The returned id is **not** a `nodes` row, so there is nothing to
   *     enter inline rename on, and nothing to select. Renaming happens on
   *     the draft (`updateDraftEndpoint`) or in the tab's title.
   *   - `lastCreatedId` is written with the draft id, so the tree's
   *     `useNewItemFlow` watcher sees the same id it used to see. It must
   *     recognise a `draft:` id rather than hunting for a row.
   *   - A draft does not appear in the tree at all until Save.
   * The parent's existence is still validated here — a stale id from a
   * menu built before a delete should resolve to "nothing happened"
   * instead of a draft that can never be saved.
   */
  createEndpointAt: (parentId: string) => Promise<string | null>;
  clearLastCreated: () => void;
}

export type UiNewItem = UiNewItemState & UiNewItemActions;

/** Fallback name per dialog kind when the user leaves the field blank. */
const DEFAULT_NAME: Record<NewDraftKind, string> = {
  project: '新项目',
  module: '新模块',
  collection: '新集合',
};

export const createUiNewItem: StateCreator<AppState, [], [], UiNewItem> = (set, get) => ({
  newItem: emptyDraft(),
  lastCreatedId: null,

  openNewItem: (opts) => {
    set({
      newItem: {
        ...emptyDraft(),
        open: true,
        type: opts?.type ?? null,
        parentModuleId: opts?.parentModuleId ?? '',
        parentCollectionId: opts?.parentCollectionId ?? '',
      },
    });
  },
  closeNewItem: () => {
    set({ newItem: emptyDraft() });
  },
  setNewItemType: (type) => set((s) => ({ newItem: { ...s.newItem, type } })),
  setNewItemName: (n) => set((s) => ({ newItem: { ...s.newItem, name: n } })),
  setNewItemDescription: (d) => set((s) => ({ newItem: { ...s.newItem, description: d } })),
  setNewItemBaseUrl: (u) => set((s) => ({ newItem: { ...s.newItem, baseUrl: u } })),
  setNewItemColor: (c) => set((s) => ({ newItem: { ...s.newItem, color: c } })),
  setNewItemParentModule: (id) =>
    set((s) => ({
      // Resetting parentCollectionId when the module changes is safe for
      // the collection dialog: a collection cannot hang off a collection
      // of a module the user has just moved away from. (`createEndpointAt`
      // never carries a collection, so it has no equivalent state.)
      newItem: { ...s.newItem, parentModuleId: id, parentCollectionId: '' },
    })),
  setNewItemParentCollection: (id) =>
    set((s) => ({ newItem: { ...s.newItem, parentCollectionId: id } })),

  submitNewItem: async () => {
    const state = get();
    const draft = state.newItem;
    if (!draft.type) return null;

    const name = draft.name.trim() || DEFAULT_NAME[draft.type];

    if (draft.type === 'project') {
      const id = await state.addProject(name, {
        description: draft.description,
        baseUrl: draft.baseUrl,
        color: draft.color,
      });
      // Enter the new project: `openProject` selects it *and* activates
      // its tab, so the tree immediately scopes to the fresh (empty)
      // project and the right pane shows its card — where the name can
      // still be edited. The project is no longer a tree row, so this is
      // the only thing that makes the create feel like it landed.
      if (id) state.openProject(id);
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

    // draft.type === 'collection'
    // The parent is whichever one the dialog settled on: a collection when
    // the user asked from a collection row (so "新建子集合" nests properly
    // instead of silently landing beside it), otherwise the module chosen
    // in the dialog.
    const parentId = draft.parentCollectionId || draft.parentModuleId;
    if (!parentId) return null;
    const parent = state.nodes.find((n) => n.id === parentId);
    if (!parent || (parent.kind !== 'module' && parent.kind !== 'collection')) return null;
    expandAncestors(state, parentId);
    const id = await state.addNode({
      // `rootId` is derived from the parent server-side; naming it
      // here as well would be a second source of truth for a value the
      // parent already determines.
      parentId,
      kind: 'collection',
      name,
      description: draft.description,
    });
    set({ lastCreatedId: id });
    return id;
  },

  createEndpointAt: async (parentId) => {
    if (!parentId) return null;
    const state = get();
    // The parent has to exist and be able to hold an endpoint. A module
    // can; a collection can; the project cannot (it is not a tree node).
    // Checking rather than trusting the caller means a stale id from a
    // menu built before a delete resolves to "nothing happened" instead
    // of a draft whose Save can never succeed.
    const parent = state.nodes.find((n) => n.id === parentId);
    if (!parent || (parent.kind !== 'module' && parent.kind !== 'collection')) return null;

    expandAncestors(state, parentId);

    // `createEndpointDraft` also opens and activates the draft's tab, so
    // there is no `setActiveEndpoint` here any more: that action routes
    // through `openTab('endpoint', id)`, which would hand the workspace a
    // tab for a row that does not exist.
    const draftId = state.createEndpointDraft(parentId);
    // Published for the tree, same as the dialog kinds' rows. See the
    // contract note above: the consumer must treat a `draft:` id as "a
    // draft appeared", not "find this row and rename it".
    set({ lastCreatedId: draftId });
    return draftId;
  },

  clearLastCreated: () => set({ lastCreatedId: null }),
});

/**
 * Expand `nodeId` and every ancestor above it, nearest first.
 *
 * With one node type this is a walk up `parentId` — the old
 * "open the collection, then find its module" two-step is gone. Starting
 * *at* the parent (not above it) is deliberate: creating a collection
 * inside a collapsed collection must reveal that collection too.
 */
function expandAncestors(state: AppState, nodeId: string): void {
  let cur: ResourceNode | undefined = state.nodes.find((n) => n.id === nodeId);
  while (cur) {
    if (!cur.expanded) state.toggleNodeExpanded(cur.id);
    cur = cur.parentId ? state.nodes.find((n) => n.id === cur!.parentId) : undefined;
  }
}
