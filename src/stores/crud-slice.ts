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
 *
 * LAYOUT
 * ------
 * This file is the slice's public surface only. The implementations live
 * in one sub-factory per concern, under `./slices/`:
 *
 *   slices/crud-helpers.ts      pure helpers (no store access)
 *   slices/crud-project.ts      addProject / updateProject / deleteProject
 *   slices/crud-tree.ts         addNode / updateNode / deleteNode /
 *                               updateEndpointDetail / duplicateEndpoint
 *   slices/crud-move.ts         moveNodes
 *   slices/crud-environment.ts  the 7 environment actions
 *   slices/crud-baseurl.ts      the 4 baseUrl definition actions
 *
 * The sub-factories are composed HERE, not registered as separate slices
 * in `app-store.ts`, and that is the whole point of the split. Several
 * actions deliberately write `set()` keys owned by other slices in the
 * same atomic write — `deleteProject` writes `openTabs` / `activeTabId` /
 * `activeProjectId` / `activeEndpointId`, `deleteNode` writes `openTabs` /
 * `activeTabId` / `activeEndpointId`, `setEnvironmentActive` writes
 * `activeEnvironmentId`, `deleteEnvironment` writes
 * `activeEnvironmentId` + `endpointDetails`, `deleteBaseUrlDefinition`
 * writes `endpointDetails`. Rewriting any of those as a sequence of
 * `get().closeTab()`-style calls would turn one `set()` into several and
 * change what the IPC-failure `set(snapshot)` restores. So they stay
 * whole, and the files stay grouped by concern instead of by slice.
 *
 * The topology is a star: sub-factories may import `./crud-helpers`, and
 * they never import each other or this file. Each takes the standard
 * `(set, get)` Zustand pair and returns only its own action object.
 */
import type { StateCreator } from 'zustand';
import type { AppState } from './app-store';
import { createBaseUrlCrud } from './slices/crud-baseurl';
import type { BaseUrlCrudActions } from './slices/crud-baseurl';
import { createEnvironmentCrud } from './slices/crud-environment';
import type { EnvironmentCrudActions } from './slices/crud-environment';
import { createMoveCrud } from './slices/crud-move';
import type { MoveCrudActions } from './slices/crud-move';
import { createProjectCrud } from './slices/crud-project';
import type { ProjectCrudActions } from './slices/crud-project';
import { createTreeCrud } from './slices/crud-tree';
import type { NewNodeInput, TreeCrudActions } from './slices/crud-tree';

/** State portion owned by the CRUD slice — empty; all state is in bootstrap/ui/dirty/mcp. */
export interface CrudSliceState {
  // Intentionally empty. CRUD slice only contributes actions. The
  // type alias exists so that `AppState & CrudSlice` still resolves.
  _crudSliceMarker?: never;
}

/**
 * Action portion owned by the CRUD slice — the union of every
 * sub-factory's action interface. The members are declared next to their
 * implementations in `./slices/`; this is the single place that names
 * them all.
 */
export interface CrudSliceActions
  extends
    ProjectCrudActions,
    TreeCrudActions,
    MoveCrudActions,
    EnvironmentCrudActions,
    BaseUrlCrudActions {}

export type CrudSlice = CrudSliceState & CrudSliceActions;

/** Re-exported so `addNode`'s input type stays importable from here. */
export type { NewNodeInput };

export const createCrudSlice: StateCreator<AppState, [], [], CrudSlice> = (set, get, store) => ({
  ...createProjectCrud(set, get, store),
  ...createTreeCrud(set, get, store),
  ...createMoveCrud(set, get, store),
  ...createEnvironmentCrud(set, get, store),
  ...createBaseUrlCrud(set, get, store),
});
