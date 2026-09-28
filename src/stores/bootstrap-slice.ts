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
import type { Project, Module, Collection, Endpoint, Environment } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from './app-store';

/** State portion owned by the bootstrap slice. */
export interface BootstrapSliceState {
  // data
  projects: Project[];
  modules: Module[];
  collections: Collection[];
  endpoints: Endpoint[];
  environments: Environment[];
  /** bootstrap 成功完成后为 true。 */
  loaded: boolean;

  // 当前选择与 bootstrap 数据紧密耦合，保留在此切片内。
  activeProjectId: string;
  activeEnvironmentId: string;
  activeEndpointId: string | null;
  /** bootstrap 请求进行中。 */
  loading: boolean;
  /** 最近一次启动加载错误；成功后清空。 */
  bootstrapError: string | null;
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

export const createBootstrapSlice: StateCreator<AppState, [], [], BootstrapSlice> = (set, get) => ({
  // v0.1 MCP rollout — empty at boot; `bootstrap()` populates from the
  // daemon (which reads SQLite). The selectors and consumers all tolerate
  // an empty list because `loaded: false` triggers a loading state in the
  // UI rather than rendering the (now-empty) tree.
  // 启动时为空；bootstrap() 成功后会一次性填入 SQLite 中的数据。
  projects: [],
  modules: [],
  collections: [],
  endpoints: [],
  environments: [],
  /** bootstrap 成功完成后为 true。 */
  loaded: false,

  activeProjectId: '',
  activeEnvironmentId: '',
  activeEndpointId: null,
  loading: false,
  bootstrapError: null,

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
    if (get().loaded || get().loading) return;
    set({ loading: true, bootstrapError: null });

    try {
      // 列表命令在没有过滤条件时返回全量数据，避免把 project_id 误传为空。
      const [projects, modules, collections, endpoints, environments] = await Promise.all([
        invokeCmd<Project[]>(COMMANDS.projectsList),
        invokeCmd<Module[]>(COMMANDS.modulesList),
        invokeCmd<Collection[]>(COMMANDS.collectionsList),
        invokeCmd<Endpoint[]>(COMMANDS.endpointsList),
        invokeCmd<Environment[]>(COMMANDS.environmentsList),
      ]);

      const firstProject = projects[0];
      const firstProjectEnv = firstProject
        ? (environments.find((e) => e.projectId === firstProject.id && e.isActive) ??
          environments.find((e) => e.projectId === firstProject.id))
        : undefined;
      const firstEndpoint = endpoints[0];

      set({
        projects,
        modules,
        collections,
        endpoints,
        environments,
        loaded: true,
        bootstrapError: null,
        loading: false,
        activeProjectId: firstProject?.id ?? '',
        activeEnvironmentId: firstProjectEnv?.id ?? '',
        activeEndpointId: firstEndpoint?.id ?? null,
      });

      if (firstEndpoint) {
        get().openTab('endpoint', firstEndpoint.id);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('[store] bootstrap failed', error);
      // 保留已有数据并暴露错误状态，不把任何列表失败伪装成空数组。
      set({ loading: false, bootstrapError: message });
    }
  },
});
