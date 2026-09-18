/**
 * Bootstrap slice — owns the four top-level data arrays (projects,
 * modules, collections, endpoints, environments) and the one-shot
 * `bootstrap()` action that hydrates them from the Rust daemon on
 * app start.
 *
 * Also owns the "active selection" state (`activeProjectId`,
 * `activeEnvironmentId`, `activeEndpointId`) plus the per-entity
 * expansion flags (`Module.expanded`, `Collection.expanded` are
 * toggled here). Co-locating them is intentional: bootstrap() picks
 * defaults, and the set-active* actions consume the data the slice
 * just loaded, so the two halves talk to each other constantly.
 *
 * CRUD-style mutations (add/update/delete) live in the crud-slice.
 * This slice is read-only besides the expansion toggles.
 */
import type { StateCreator } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  Project,
  Module,
  Collection,
  Endpoint,
  Environment,
} from '@/types/domain';
import { COMMANDS } from '@/lib/tauri-commands';
import type { AppState } from './app-store';

/** State portion owned by the bootstrap slice. */
export interface BootstrapSliceState {
  // data
  projects: Project[];
  modules: Module[];
  collections: Collection[];
  endpoints: Endpoint[];
  environments: Environment[];
  /** True once bootstrap() has resolved at least once. */
  loaded: boolean;

  // active selection (kept here because it's tightly coupled to the
  // data arrays — setActiveProject chooses a default env from
  // `environments`, setActiveEndpoint opens a tab in ui-slice state).
  activeProjectId: string;
  activeEnvironmentId: string;
  activeEndpointId: string | null;
  loading: boolean;
}

/** Action portion owned by the bootstrap slice. */
export interface BootstrapSliceActions {
  // actions: data — selection / expansion
  setActiveProject: (id: string) => void;
  setActiveEnvironment: (id: string) => void;
  setActiveEndpoint: (id: string | null) => void;
  toggleModuleExpanded: (id: string) => void;
  toggleCollectionExpanded: (id: string) => void;

  // actions: bootstrap (one-shot at app start)
  /**
   * Fetch all persistent data from the SQLite daemon and mirror into
   * the store. Idempotent — a second call after the first is a no-op.
   * Triggered from TopBar's useEffect on mount.
   */
  bootstrap: () => Promise<void>;
}

export type BootstrapSlice = BootstrapSliceState & BootstrapSliceActions;

export const createBootstrapSlice: StateCreator<AppState, [], [], BootstrapSlice> = (
  set,
  get,
) => ({
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
  setActiveEndpoint: (id) => {
    // `activeEndpointId` is owned by this slice; the workspace-tab side
    // effects (openTabs / activeTabId) belong to ui-slice — route them
    // through the canonical actions instead of writing across slice
    // boundaries here. `lastResponse: null` likewise is owned by
    // ui-slice (it's the last HTTP response, not the active-selection
    // cache), so it goes through `setLastResponse`.
    set({ activeEndpointId: id });
    if (id !== null) {
      get().openTab('endpoint', id);
    }
    get().setLastResponse(null);
  },
  toggleModuleExpanded: (id) =>
    set((s) => ({
      modules: s.modules.map((m) => (m.id === id ? { ...m, expanded: !m.expanded } : m)),
    })),
  toggleCollectionExpanded: (id) =>
    set((s) => ({
      collections: s.collections.map((c) => (c.id === id ? { ...c, expanded: !c.expanded } : c)),
    })),

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
    });
    // Open the first endpoint by default, mirroring the old mock seed.
    // Routed through ui-slice's `openTab` action so the workspace-tab
    // invariants (idempotent re-open, single-source `activeTabId`) live
    // in one place. On a fresh install with no endpoints yet, the
    // initial `openTabs: []` / `activeTabId: null` from the slice
    // factory is already correct, so we skip the call.
    if (firstEndpoint) {
      get().openTab('endpoint', firstEndpoint.id);
    }
  },
});