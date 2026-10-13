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
  /**
   * 第二层（树 + 接口配置 + 环境）是否已就绪。
   *
   * 与 `loaded` 刻意分开：欢迎页只需要 `projects`，所以 `loaded` 一旦
   * 项目列表落地就翻 true，启动画面随即被 `BootSplashGate` 拆掉 ——
   * 用户看到欢迎页不必等三个可能很大的列表。剩下的数据只对"已经进入
   * 某个项目"有意义，由 `loadWorkspace()` 在后台（或用户进入项目时按需）
   * 补齐。
   *
   * 消费方必须区分二者：把 `nodes: []` 当成"该项目没有模块"会在这段
   * 窗口里显示一句彻底错误的空状态。
   */
  nodesLoaded: boolean;
  /**
   * 第二层加载进行中。单独一个 flag 而不是复用 `loading`，因为两者回答
   * 不同问题：`loading` 是"欢迎页的数据还没到"，这个是"树还在拉"。
   * 合并会让启动画面等上整个树。
   */
  nodesLoading: boolean;

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
  /**
   * 第二层（树 / 接口配置 / 环境）的加载错误。与 `bootstrapError` 分开，
   * 这样"欢迎页正常但树拉取失败"不会被误报成"启动失败"——后者会把启动
   * 画面一直钉在屏幕上。
   */
  nodesError: string | null;
}

/** Action portion owned by the bootstrap slice. */
export interface BootstrapSliceActions {
  // actions: data — selection / expansion
  /**
   * Select a project. The canonical "the user is now working in project
   * X" event — see `openProject` below for the distinction — and so also
   * where the per-project response history is (re)loaded from, because
   * `responseHistory` is a whole-project map that a cross-project jump
   * would otherwise leave pointing at the old project.
   */
  setActiveProject: (id: string) => void;
  /**
   * *Enter* a project: select it **and** swap the tab strip to that
   * project's own set.
   *
   * This is the single entry point for "the user is now working in
   * project X", and every surface that lands on a project goes through
   * it — the welcome picker, the top-bar selector, the command palette,
   * the HTTP tester's breadcrumb, and the create-project flow. Having
   * one action is what keeps those paths converging on the same state
   * instead of drifting into "tree shows X, right pane still shows Y".
   *
   * The strip swap is the point of the ownership change: leaving project
   * A for project B sets A's tabs aside in `projectTabs` and brings up
   * B's, so an endpoint tab from one project can never be sitting on
   * screen while the user works in the other. Coming back restores them,
   * unsaved edits included — see {@link ProjectTabSet}.
   *
   * Distinct from `setActiveProject`, which is a pure selection setter:
   * the palette's "jump to an endpoint in another project" needs to
   * change the owner without swapping the strip, because it opens the
   * endpoint tab immediately after and that is what they want to see.
   */
  openProject: (id: string) => void;
  /**
   * Leave the current project and return to the welcome screen.
   *
   * The inverse of `openProject`, and the only way back now that the
   * welcome page is a standalone surface rather than a panel inside the
   * workspace frame. `App.tsx` switches on `activeProjectId === ''`, so
   * clearing it is what swaps the surfaces — there is no separate
   * "which screen am I on" state to keep in sync.
   *
   * Deliberately does NOT close tabs or clear the loaded tree. Those are
   * per-project artifacts the user may want back the moment they click
   * the project again, and re-entering via `openProject` restores them
   * immediately since `nodesLoaded` short-circuits the reload. Only the
   * selection is reset — `activeEnvironmentId` along with it, since an
   * environment belonging to the abandoned project is meaningless while
   * no project is active.
   */
  leaveProject: () => void;
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
   * Fetch persistent data from the SQLite daemon and mirror into the
   * store. Idempotent — a second call after the first is a no-op.
   * Fired from `main.tsx`, before `createRoot`, so the IPC overlaps
   * React mounting instead of queueing behind it.
   *
   * Two layers:
   *
   *   1. `projects` — awaited. Flips `loaded`, which tears down the HTML
   *      boot splash and lets the welcome screen paint.
   *   2. `nodes` / `endpointDetails` / `environments` — delegated to
   *      `loadWorkspace()` and *not* awaited, so a large tree never
   *      delays the first screen.
   *
   * Step 2 still starts immediately; it just stops being on the critical
   * path. `openProject` also kicks it, so a user who clicks a project
   * card before the background pass lands still gets a complete tree.
   */
  bootstrap: () => Promise<void>;

  /**
   * Layer 2 — the resource tree, every endpoint's request configuration,
   * and the environments.
   *
   * Safe to call at any time and from anywhere: it early-returns when the
   * data is already in place or a load is in flight, so `bootstrap`,
   * `openProject` and the retry button can all reach for it without
   * coordinating. Never rejects — failures land in `nodesError`, which is
   * a *different* flag from `bootstrapError` on purpose: layer 1 can
   * succeed while layer 2 fails, and that shows up as "the welcome page
   * works but the tree failed to load", not as a dead startup.
   */
  loadWorkspace: () => Promise<void>;

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
 * still renders. Failing the whole load instead would leave the user
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

export const createBootstrapSlice: StateCreator<AppState, [], [], BootstrapSlice> = (set, get) => {
  /**
   * The layer-2 pass currently in flight, or `null`.
   *
   * Lives in the slice closure rather than in store state on purpose: it
   * is a promise, not data, and putting it in state would make every
   * subscriber's `Object.is` comparison see a new reference on each write.
   * It exists so concurrent callers of `loadWorkspace()` share one pass
   * instead of either duplicating the queries or resolving early.
   */
  let inFlight: Promise<void> | null = null;

  return {
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
    nodesLoaded: false,
    nodesLoading: false,

    activeProjectId: '',
    activeEnvironmentId: '',
    activeEndpointId: null,
    loading: false,
    bootstrapError: null,
    nodesError: null,

    // ---------- selection / expansion ----------

    setActiveProject: (id) => {
      // Only a real change re-reads per-project data. Re-selecting the
      // project already in front is not an event, and the strip swap /
      // tree load in `openProject` must not be duplicated by it.
      const changed = get().activeProjectId !== id;
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
      });
      // Per-project response history hangs off this action rather than
      // `openProject` (which calls it) or `loadWorkspace` (which
      // short-circuits on `nodesLoaded`, and at boot `activeProjectId`
      // is `''` anyway): the command palette's "jump to an endpoint in
      // another project" deliberately bypasses `openProject`, so this is
      // the only event every path into a project shares.
      //
      // Fire-and-forget, like layer 2 of `bootstrap()` — the history strip
      // is not on the critical path into a project, and its failure lands
      // nowhere near `bootstrapError`.
      if (changed) void get().loadResponseHistory(id);
    },
    setActiveEnvironment: (id) => set({ activeEnvironmentId: id }),
    openProject: (id) => {
      // The tree is what the user is about to look at, so don't wait for
      // the background pass to finish — kick it here and let the render
      // land on `nodesLoaded === false` (the tree shows a loading state
      // rather than a wrong "no modules" empty state).
      void get().loadWorkspace();
      // Swap the strip BEFORE the active project changes. Deciding what
      // to set aside means asking which project owns each tab on screen,
      // and that answer is only meaningful against the project currently
      // in front — after `setActiveProject` there would be no "currently
      // in front" left to attribute them to.
      //
      // This also replaces the old `openTab('project', id)`: the switch
      // guarantees the incoming project owns a project tab, and decides
      // whether the user lands on it or on whatever they had open here
      // last time.
      get().switchProjectTabs(id);
      get().setActiveProject(id);
      // The endpoint the tester was showing belongs to the project being
      // left. Left in place, `useActiveEndpoint` would answer with an
      // endpoint the new tree does not contain, and so would the last
      // response — both of which describe somewhere the user no longer is.
      set({ activeEndpointId: null });
      get().setLastResponse(null);
    },
    leaveProject: () => {
      // No-op when nothing is entered, so a stray click on the title while
      // already on the welcome screen cannot disturb state.
      if (get().activeProjectId === '') return;
      // Same swap as `openProject`, in the other direction, and for the
      // same ordering reason: stash while the project is still the one in
      // front. Getting this after the `set` below would attribute every
      // open tab to nobody and lose the strip.
      get().switchProjectTabs('');
      set({ activeProjectId: '', activeEnvironmentId: '', activeEndpointId: null });
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

    // ---------- bootstrap (one-shot, called from main.tsx) ----------

    bootstrap: async () => {
      if (get().loaded) return;
      set({ loading: true, bootstrapError: null });

      try {
        // Layer 1 — the only query the welcome screen needs. One indexed
        // read of a handful of rows, and the moment it resolves the boot
        // splash comes down and the user sees the project picker.
        //
        // 列表命令在没有过滤条件时返回全量数据，避免把 project_id 误传为空。
        const projects = await invokeCmd<Project[]>(COMMANDS.projectsList);

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
          loaded: true,
          bootstrapError: null,
          loading: false,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        console.error('[store] bootstrap failed', error);
        // 保留已有数据并暴露错误状态，不把任何列表失败伪装成空数组。
        set({ loading: false, bootstrapError: message });
        return;
      }

      // Layer 2 — deliberately NOT awaited. Awaiting it would keep the boot
      // splash on screen for three extra round-trips that the welcome
      // screen never renders.
      void get().loadWorkspace();
    },

    loadWorkspace: async () => {
      // Idempotent on both flags — three call sites reach for this
      // (`bootstrap`, `openProject`, and a retry) and none of them
      // coordinate with each other.
      //
      // The in-flight branch returns the *pending* promise rather than
      // resolving immediately. That distinction is load-bearing: a caller
      // that awaits this to sequence its own work after the tree is ready
      // would otherwise continue at once and observe the pre-load state.
      if (get().nodesLoaded) return;
      if (inFlight) return inFlight;

      inFlight = (async () => {
        set({ nodesLoading: true, nodesError: null });

        try {
          // One tree read replaces the three unfiltered list calls the old
          // layout issued; `endpointDetails` is then a second call for the
          // configuration the tree itself never renders.
          const [nodes, endpointDetails, environments] = await Promise.all([
            invokeCmd<ResourceNode[]>(COMMANDS.nodesList),
            loadEndpointDetails(),
            invokeCmd<Environment[]>(COMMANDS.environmentsList),
          ]);

          // Re-resolve the active environment on commit. `setActiveProject`
          // picks the project's active env from `s.environments`, but a
          // user who clicks a project card while this pass is still in
          // flight resolves against an empty list and lands on `''` — which
          // the TopBar renders as "—". Doing the pick here as well closes
          // that window, and is a no-op when the id is already correct.
          set((s) => {
            const current = s.activeEnvironmentId;
            const stillValid = environments.some(
              (e) => e.id === current && e.projectId === s.activeProjectId,
            );
            const env = stillValid
              ? undefined
              : (environments.find((e) => e.projectId === s.activeProjectId && e.isActive) ??
                environments.find((e) => e.projectId === s.activeProjectId));
            return {
              nodes,
              endpointDetails,
              environments,
              nodesLoaded: true,
              ...(env ? { activeEnvironmentId: env.id } : {}),
            };
          });
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          console.error('[store] workspace load failed', error);
          set({ nodesError: message });
        } finally {
          set({ nodesLoading: false });
          // Cleared last: every awaiter has already been handed this
          // promise, and a retry issued from the `nodesError` banner below
          // must be able to start a fresh pass rather than join a settled
          // one.
          inFlight = null;
        }
      })();

      return inFlight;
    },

    clearBootstrapError: () => set({ bootstrapError: null }),
  };
};
