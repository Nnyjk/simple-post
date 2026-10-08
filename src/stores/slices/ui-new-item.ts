import type { StateCreator } from 'zustand';
import type { ResourceNode } from '@/types/domain';
import { PROJECT_COLOR_DEFAULT } from '@/lib/palette';
import type { AppState } from '../app-store';
import type { NewItemDraft, NewType } from '../store-types';

/**
 * ui-new-item — the "create a thing" modal's draft and its commit.
 *
 * Pure UI state plus one action that reaches out to the CRUD slice. The
 * split extracts the *four creation paths* into named helpers but keeps
 * `submitNewItem` itself as a single `await` + `set()` sequence per
 * branch, because each branch's `set({ lastCreatedId })` has to land
 * after the create resolves and the callers (`ProjectTree`) watch that
 * id to enter rename mode on the new row. Splitting the branches
 * further would put `lastCreatedId` updates behind an extra promise hop
 * for no gain.
 */

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

export type UiNewItem = UiNewItemState & UiNewItemActions;

/** Fallback name per entity kind when the user leaves the field blank. */
const DEFAULT_NAME: Record<NewType, string> = {
  project: '新项目',
  module: '新模块',
  collection: '新集合',
  endpoint: '新接口',
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

    const name = draft.name.trim() || DEFAULT_NAME[draft.type];

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
    // The parent is a collection, or the module itself when the endpoint
    // needs no folder — an empty `parentCollectionId` is that second
    // case, not a missing field. The module is what `addNode` derives
    // `rootId` from either way.
    const parentId = draft.parentCollectionId || draft.parentModuleId;
    if (!parentId) return null;
    // Expand every ancestor so the new endpoint is visible where it
    // landed. With one node type this is a walk up `parentId` instead of
    // the old "open the collection, then find its module" two-step, and
    // starting from the module covers the folderless case unchanged.
    let ancestor: ResourceNode | undefined = state.nodes.find((n) => n.id === parentId);
    while (ancestor) {
      if (!ancestor.expanded) state.toggleNodeExpanded(ancestor.id);
      ancestor = ancestor.parentId
        ? state.nodes.find((n) => n.id === ancestor!.parentId)
        : undefined;
    }
    const id = await state.addNode({
      parentId,
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
