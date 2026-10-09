/**
 * Bootstrap slice — owns the top-level data (projects, the resource tree,
 * environments) and the one-shot `bootstrap()` action that hydrates them
 * from the Rust daemon on app start.
 *
 * The tree is one `nodes` array plus an `endpointDetails` map keyed by
 * node id, replacing the three parallel `modules` / `collections` /
 * `endpoints` arrays. `nodes` carries structure — id, owner, parent,
 * type, name, order — and `endpointDetails` carries the request
 * configuration that only an endpoint has. Keeping them apart is not
 * tidiness: `method` / `url` / `tags` mean nothing for a module, and the
 * tree never renders them.
 *
 * Also owns the "active selection" state (`activeProjectId`,
 * `activeEnvironmentId`, `activeEndpointId`) plus the per-node expansion
 * flag (`ResourceNode.expanded`). Co-locating them is intentional:
 * bootstrap() picks defaults, and the set-active* actions consume the
 * data the slice just loaded, so the two halves talk to each other
 * constantly.
 *
 * CRUD-style mutations (add/update/delete) live in the crud-slice.
 * This slice is read-only besides the expansion toggle.
 */
import type { StateCreator } from 'zustand';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from './app-store';

/** State portion owned by the bootstrap slice. */
export interface BootstrapSliceState {
  // data
  projects: Project[];
  /**
   * Every node in the workspace, flat. Hierarchy is `parentId`; the tree
   * component buckets by it rather than reading a nested shape, which
   * keeps a drag-and-drop reorder a single array write.
   */
  nodes: ResourceNode[];
  /**
   * Endpoint request configuration, keyed by node id. Only `type:
   * 'endpoint'` nodes have an entry. A `Record` rather than an array so
   * "this endpoint's url" is a map read instead of a scan — the HTTP
   * tester does it on every keystroke's re-render.
   */
  endpointDetails: Record<string, EndpointDetail>;
  environments: Environment[];
  /** bootstrap 成功完成后为 true。 */
  loaded: boolean;

  // 当前选择与 bootstrap 数据紧密耦合，保留在此切片内。
  /**
   * 当前项目 id。`''` = 尚未选择项目 —— 刚启动（bootstrap 不会预选）
   * 或用户主动退出到欢迎页。树的渲染范围、TopBar 的环境列表、
   * 新建模块/集合/接口的落点全部以它为准。
   */
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
  /**
   * *Enter* a project: select it **and** make its tab the active one.
   *
   * This is the single entry point for "the user is now working in
   * project X", and every surface that lands on a project goes through
   * it — the welcome picker, the top-bar selector, the command palette,
   * the HTTP tester's breadcrumb, and the create-project flow. Having
   * one action is what keeps those paths converging on the same state
   * instead of drifting into "tree shows X, right pane still shows Y".
   *
   * Distinct from `setActiveProject`, which is a pure selection setter:
   * the palette's "jump to an endpoint in another project" needs to
   * change the owner without inserting a project tab the user did not
   * ask for, because the endpoint tab it opens immediately after is
   * what they want to see.
   */
  openProject: (id: string) => void;
  setActiveEnvironment: (id: string) => void;
  setActiveEndpoint: (id: string | null) => void;
  /**
   * Expand / collapse one node. One action for all three types, where
   * there used to be `toggleModuleExpanded` and `toggleCollectionExpanded`
   * — the flag is on the node either way, and only the tree knows which
   * types it renders as expandable.
   */
  toggleNodeExpanded: (id: string) => void;

  // actions: bootstrap (one-shot at app start)
  /**
   * Fetch all persistent data from the SQLite daemon and mirror into
   * the store. Idempotent — a second call after the first is a no-op.
   * Triggered from TopBar's useEffect on mount.
   */
  bootstrap: () => Promise<void>;

  /**
   * Dismiss the startup-load error banner without retrying.
   *
   * `bootstrapError` used to be write-only state: `bootstrap()` set it on
   * failure and nothing ever read it, so a failed startup looked
   * identical to an empty database — every list rendered blank with no
   * indication that the backend was unreachable. The banner in
   * `features/top-bar/BootstrapErrorBanner` is the reader; this action is
   * its dismiss affordance.
   *
   * Only clears the message — `loaded` / `loading` are untouched, so a
   * subsequent `bootstrap()` retry still runs.
   */
  clearBootstrapError: () => void;
}

export type BootstrapSlice = BootstrapSliceState & BootstrapSliceActions;

/**
 * Fetch every endpoint's request configuration in one call.
 *
 * Fails soft to `{}` on purpose. A missing configuration map degrades to
 * "the HTTP tester shows blank method / url" — recoverable, and the tree
 * still renders. Failing the whole bootstrap instead would leave the user
 * with an empty workspace and a retry button, which is a far worse
 * outcome for a partial read.
 */
async function loadEndpointDetails(): Promise<Record<string, EndpointDetail>> {
  const rows = await invokeCmd<EndpointDetail[]>(COMMANDS.endpointDetailsList).catch(
    (e: unknown) => {
      console.warn('[store] endpoint details unavailable', e);
      return [] as EndpointDetail[];
    },
  );
  return Object.fromEntries(rows.map((d) => [d.nodeId, d]));
}

export const createBootstrapSlice: StateCreator<AppState, [], [], BootstrapSlice> = (set, get) => ({
  // v0.1 MCP rollout — empty at boot; `bootstrap()` populates from the
  // daemon (which reads SQLite). The selectors and consumers all tolerate
  // an empty list because `loaded: false` triggers a loading state in the
  // UI rather than rendering the (now-empty) tree.
  // 启动时为空；bootstrap() 成功后会一次性填入 SQLite 中的数据。
  projects: [],
  nodes: [],
  endpointDetails: {},
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
  openProject: (id) => {
    get().setActiveProject(id);
    get().openTab('project', id);
  },
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
  toggleNodeExpanded: (id) =>
    set((s) => ({
      nodes: s.nodes.map((n) => (n.id === id ? { ...n, expanded: !n.expanded } : n)),
    })),

  // ---------- bootstrap (one-shot, called from TopBar mount) ----------

  bootstrap: async () => {
    if (get().loaded || get().loading) return;
    set({ loading: true, bootstrapError: null });

    try {
      // 列表命令在没有过滤条件时返回全量数据，避免把 project_id 误传为空。
      // One tree read replaces the three unfiltered list calls the old
      // layout issued; `endpointDetails` is then a second call for the
      // configuration the tree itself never renders.
      const [projects, nodes, endpointDetails, environments] = await Promise.all([
        invokeCmd<Project[]>(COMMANDS.projectsList),
        invokeCmd<ResourceNode[]>(COMMANDS.nodesList),
        loadEndpointDetails(),
        invokeCmd<Environment[]>(COMMANDS.environmentsList),
      ]);

      // No selection is made here, deliberately. Bootstrap used to pick
      // `projects[0]`, resolve its active environment, and open the
      // first endpoint it contained — which meant the app never once
      // showed the welcome screen on start, and a workspace with
      // several projects silently dropped the user into whichever one
      // happened to sort first. `activeProjectId` stays `''`, which is
      // what the welcome picker and the tree's "no project" state both
      // test; `openProject` is how a project gets entered.
      set({
        projects,
        nodes,
        endpointDetails,
        environments,
        loaded: true,
        bootstrapError: null,
        loading: false,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error('[store] bootstrap failed', error);
      // 保留已有数据并暴露错误状态，不把任何列表失败伪装成空数组。
      set({ loading: false, bootstrapError: message });
    }
  },

  clearBootstrapError: () => set({ bootstrapError: null }),
});
