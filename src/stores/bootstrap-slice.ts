/**
 * Bootstrap 切片 —— 顶层数据（projects、资源树、environments）与启动时的一次性
 * `bootstrap()`，外加当前选择与展开状态。选择状态与这些数据同处一片是刻意的：
 * `bootstrap()` 要挑默认值，`setActive*` 要读刚加载的数据，两半一直在互相调用。
 *
 * 树是扁平的 `nodes` 数组 + 按 node id 索引的 `endpointDetails`，不是三条并行数组：
 * `method` / `url` / `tags` 对模块无意义，树也从不渲染它们。
 *
 * 增删改在 crud-slice；本切片除展开开关外只读。
 */
import type { StateCreator } from 'zustand';
import type { EndpointDetail, Environment, Project, ResourceNode } from '@/types/domain';
import { COMMANDS, invokeCmd } from '@/lib/tauri-commands';
import type { AppState } from './app-store';
import {
  loadRequestSettings,
  loadResponseHistoryKeep,
  readLastProjectId,
  saveLastProjectId,
} from './slices/ui-prefs';

/** 状态部分。 */
export interface BootstrapSliceState {
  // data
  projects: Project[];
  /**
   * 工作区全部节点，扁平。层级靠 `parentId`，树组件按它分桶而不读嵌套结构，
   * 这样拖拽重排就是一次数组写入。
   */
  nodes: ResourceNode[];
  /**
   * 接口请求配置，按 node id 索引。只有 `type: 'endpoint'` 的节点有条目。
   * 用 `Record` 而非数组：HTTP tester 每次按键重渲染都要读"这个接口的 url"。
   */
  endpointDetails: Record<string, EndpointDetail>;
  environments: Environment[];
  /** bootstrap 成功完成后为 true。 */
  loaded: boolean;
  /**
   * 第二层（树 + 接口配置 + 环境）是否已就绪。
   *
   * 与 `loaded` 刻意分开：欢迎页只需 `projects`，故 `loaded` 一为 true 就拆掉启动
   * 画面，不必等三个可能很大的列表。
   *
   * 消费方必须区分二者：`nodesLoaded === false` 窗口里的 `nodes: []` 是歧义的，
   * 此时渲染加载态而非"该项目还没有模块"。
   */
  nodesLoaded: boolean;
  /**
   * 第二层进行中。单独一个 flag 而不复用 `loading`：后者回答"欢迎页的数据还没到"，
   * 合并会让启动画面等上整个树。
   */
  nodesLoading: boolean;

  // 当前选择与 bootstrap 数据紧密耦合，保留在此切片内。
  /**
   * 当前项目 id。`''` = 尚未选择项目（刚启动，或已退出到欢迎页）。树的渲染范围、
   * 顶栏环境列表、新建模块/集合/接口的目标位置全以它为准。
   */
  activeProjectId: string;
  activeEnvironmentId: string;
  activeEndpointId: string | null;
  /** bootstrap 请求进行中。 */
  loading: boolean;
  /** 最近一次启动加载错误；成功后清空。 */
  bootstrapError: string | null;
  /**
   * 第二层的加载错误。与 `bootstrapError` 分开，否则"欢迎页正常但树拉取失败"
   * 会被误报成启动失败，把启动画面一直钉在屏幕上。
   */
  nodesError: string | null;
}

/** action 部分。 */
export interface BootstrapSliceActions {
  // actions: data — selection / expansion
  /**
   * 纯选择 setter：设 `activeProjectId` 并按该项目重解 `activeEnvironmentId`。
   * 不换标签条（那是 `openProject` 的职责）。响应历史也不在此加载 —— 它按
   * endpoint 作用域，由 `setActiveTab` 触发。
   */
  setActiveProject: (id: string) => void;
  /**
   * *进入* 项目：既选中，又把标签条换成该项目自己的一组。
   *
   * 这是"用户在项目 X 里工作"的唯一入口 —— 欢迎页选择器、顶栏选择器、命令面板、
   * HTTP tester 面包屑、新建项目流程都走它。单点才能让这些路径收敛到同一状态，
   * 而不是"树是 X、右栏还是 Y"。
   *
   * 换条即所有权转移：A 的标签存进 `projectTabs`，B 的取上来，切回时连未保存的
   * 编辑一并恢复（见 {@link ProjectTabSet}）。
   *
   * 与 `setActiveProject` 的分工：后者不换条，因为命令面板的"跳到另一项目的接口"
   * 紧接着就打开那个接口标签，换条会把它顶掉。
   *
   * 项目 id 被记进 localStorage 这件事由 store 的订阅负责（见 `app-store.ts`）——
   * `activeProjectId` 还有别的改法，在这里单独写会漏掉它们。
   */
  openProject: (id: string) => void;
  /**
   * 退出项目回欢迎页。`openProject` 的逆操作，也是现在唯一的回去路径 ——
   * 欢迎页是独立 surface，`App.tsx` 按 `activeProjectId === ''` 切换，没有第二个
   * "当前在哪个屏"的状态要同步。
   *
   * 刻意不关标签、不清树：它们是按项目的产物，用户可能马上点回来，`openProject`
   * 时 `nodesLoaded` 会短路重载。只重置选择 —— `activeEnvironmentId` 一并清空，
   * 因为无项目时环境无意义。
   *
   * 记住的项目 id 由 store 的订阅清掉（`activeProjectId` 变成空串）。
   */
  leaveProject: () => void;
  setActiveEnvironment: (id: string) => void;
  setActiveEndpoint: (id: string | null) => void;
  /** 展开 / 折叠一个节点。三种类型共用一个动作：展开位在节点上，只有树知道哪些类型渲染成可展开。 */
  toggleNodeExpanded: (id: string) => void;

  // actions: bootstrap (one-shot at app start)
  /**
   * 从 daemon 拉持久化数据镜像进 store。幂等。由 `main.tsx` 在 `createRoot` 之前
   * 触发，让 IPC 与 React 挂载重叠，而不是排在它后面。
   *
   * 两层：
   *
   *   1. `projects` —— await。翻 `loaded`，拆掉 HTML 启动闪屏，欢迎页开画。
   *   2. `nodes` / `endpointDetails` / `environments` —— 交给 `loadWorkspace()`，
   *      **不** await，大树不会拖慢首屏。
   *
   * 第 2 步立即开始，只是不在关键路径上。`openProject` 也会触发它，故在后台
   * 那一趟完成前就点开项目卡的用户仍能拿到完整的树。
   *
   * 第 1 层完成后若 localStorage 里记着上次停留的项目，本函数进入那个项目 ——
   * 刷新后回到原处，而不是回到欢迎页。项目已不存在时清掉那条记录并留在欢迎页。
   */
  bootstrap: () => Promise<void>;

  /**
   * 第二层 —— 资源树、每个接口的请求配置、环境。
   *
   * 任何时候、任何地方都可调：数据已就位或有加载在途时提前返回，故 `bootstrap`、
   * `openProject` 与重试按钮可以各自调用而无需互相协调。永不 reject，失败落
   * `nodesError`：第 1 层成功而第 2 层失败要显示成"欢迎页可用但树加载失败"，
   * 而不是死掉的启动。
   */
  loadWorkspace: () => Promise<void>;

  /**
   * 强制重跑第二层，绕过 `loadWorkspace` 那两条幂等早退。
   *
   * 导入一次会写进几十上百行，而它们**不是**逐条 `addNode` 进去的（落库在 Rust 侧一次
   * 事务完成，回来的只有 id 列表）。让 store 自己补齐等于把导入的写入顺序复制一份到
   * 前端；重取一趟树代价只是一次 IPC。
   *
   * 同样永不 reject：失败落 `nodesError`，已有的树保持可读，只是不更新。
   */
  reloadWorkspace: () => Promise<void>;

  /**
   * 关掉启动加载错误横幅（`features/top-bar/BootstrapErrorBanner`）而不重试。
   * 只清消息，`loaded` / `loading` 不动，故后续 `bootstrap()` 重试仍会跑。
   */
  clearBootstrapError: () => void;

  /**
   * 重新取一次项目清单，整份替换 `projects`。
   *
   * 项目行只在 bootstrap 第一层加载过一次——导入在服务端建了项目之后不刷新它，
   * 界面上就看不见那个新项目。
   */
  refreshProjects: () => Promise<void>;
}

export type BootstrapSlice = BootstrapSliceState & BootstrapSliceActions;

/**
 * 一次调用取回所有接口的请求配置。
 *
 * 刻意 fail soft 到 `{}`：缺这张表只降级为"HTTP tester 的 method / url 为空"，
 * 可恢复且树照常渲染；整体失败则只剩空工作区加一个重试按钮，不划算。
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

/**
 * 把旧节点的展开状态按 id 合进新取回的节点列表。
 *
 * `expanded` 只活在渲染器里（服务端不存它），故整份替换 `nodes` 会把用户打开的分支全收起。
 */
function mergeExpanded(
  prev: readonly ResourceNode[],
  next: readonly ResourceNode[],
): ResourceNode[] {
  const expanded = new Map(prev.map((n) => [n.id, n.expanded]));
  return next.map((n) => {
    const was = expanded.get(n.id);
    return was === undefined ? n : { ...n, expanded: was };
  });
}

export const createBootstrapSlice: StateCreator<AppState, [], [], BootstrapSlice> = (set, get) => {
  /**
   * 当前在途的第二层加载，或 `null`。
   *
   * 放在切片闭包而非 store state：它是 promise 不是数据，进 state 会让每个订阅者
   * 的 `Object.is` 每次写入都看到新引用。存在是为了让并发调用共享同一趟加载，
   * 而不是重复查询或提前 resolve。
   */
  let inFlight: Promise<void> | null = null;

  return {
    // 启动时为空；`bootstrap()` 从 daemon（读 SQLite）填入。空列表可接受：
    // `loaded: false` 时 UI 渲染加载态而非空树。
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
      // `setActiveProject` 只有在项目 id 真的变了才重解激活环境；重新选中当前项目
      // 不算变化。
      set((s) => {
        const env =
          s.environments.find((e) => e.projectId === id && e.isActive) ??
          s.environments.find((e) => e.projectId === id);
        // 新项目没有环境时这里把它清成 `''`（"未选"哨兵），`useActiveEnvironment()` 返回
        // undefined，顶栏触发器回落到 '—'。
        return {
          activeProjectId: id,
          activeEnvironmentId: env?.id ?? '',
        };
      });
      // 刻意不在此加载响应历史：它按 endpoint 而非按项目作用域，由 `setActiveTab`
      // —— 所有进入某接口的路径共享的事件 —— 触发。
    },
    setActiveEnvironment: (id) => set({ activeEnvironmentId: id }),
    openProject: (id) => {
      // 树是用户马上要看的东西，不等后台那一趟：在这里触发，让渲染落在
      // `nodesLoaded === false` 上（树显示加载态而非错误的"没有模块"空态）。
      void get().loadWorkspace();
      // 先换标签条再改 active project：判断该把哪些标签搁置，需要知道屏幕上每个
      // 标签属于谁，而这个答案只对当前在前的项目成立 —— 过了 `setActiveProject`
      // 就无从归属了。换条也顺带保证新项目一定拥有一个项目 tab，并决定用户落在它
      // 身上还是落在上次在这里打开的那一个。
      get().switchProjectTabs(id);
      get().setActiveProject(id);
      // 记住这个项目由 store 的订阅负责（见 `app-store.ts`）：`activeProjectId` 还有
      // 命令面板与删项目两条改法，在这里单独写会漏掉它们。
      // tester 正显示的接口属于被离开的项目；留着会让 `useActiveEndpoint` 和最后
      // 一条响应都指向新树里不存在的接口。
      set({ activeEndpointId: null });
      get().setLastResponse(null);
    },
    leaveProject: () => {
      // `leaveProject` 在已经不在项目里时空操作，欢迎页上误点标题不会扰动状态。
      if (get().activeProjectId === '') return;
      // 与 `openProject` 同样的换条、同样的顺序理由：`switchProjectTabs('')` 在项目
      // 仍是"在前的那个"时搁置标签，否则每个打开的标签都无人归属，标签条会丢。
      get().switchProjectTabs('');
      // 清掉记住的项目 id 由 store 的订阅负责（`activeProjectId` 变成空串）。
      set({ activeProjectId: '', activeEnvironmentId: '', activeEndpointId: null });
    },
    setActiveEndpoint: (id) => {
      // `activeEndpointId` 归本切片；标签侧的副作用（openTabs / activeTabId）归
      // ui-slice —— 走规范动作而不是跨切片写。`lastResponse: null` 同理归 ui-slice
      // （它是最近一次 HTTP 响应，不是选择缓存），故走 `setLastResponse`。
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
        // 第 1 层 —— 欢迎页唯一需要的查询：一次带索引的少量行读取，这一层一返回启动
        // 闪屏即拆下，用户看到项目选择器。
        //
        // 列表命令在没有过滤条件时返回全量数据，避免把 project_id 误传为空。
        const projects = await invokeCmd<Project[]>(COMMANDS.projectsList);

        // 请求旋钮也在第 1 层读：它存在后端的 `preferences` 表里，而设置面板在欢迎页
        // 上就能打开 —— 旋钮必须在那之前就位，否则面板先显示的是默认值，而用户在
        // 面板上改的值也会被随后返回的第二层覆盖掉。
        //
        // 顺序而非并发：并发时两条命令的发出次序不确定，测试与日志都难读。
        const requestSettings = await loadRequestSettings();
        // 历史保留条数同样在后端的 `preferences` 表里，且同样要在第 1 层读：它决定库里
        // 留几行历史，界面与库必须从一开始就是同一个数。
        const responseHistoryKeep = await loadResponseHistoryKeep();

        // 恢复上次停留的项目：刷新后回到它，而不是回到欢迎页。
        //
        // 项目可能已被删除，这时清掉记住的值并留在欢迎页 —— 否则每次刷新都要先试
        // 一次、进不去、再退回。这一处要自己写：`activeProjectId` 没有变（仍是空串），
        // store 的订阅不会替我们做这件事。
        const remembered = readLastProjectId();
        const rememberedExists = remembered !== '' && projects.some((p) => p.id === remembered);
        if (remembered !== '' && !rememberedExists) {
          saveLastProjectId('');
        }

        // 恢复发生在 `loaded` 翻真**之前**：`App.tsx` 按 `activeProjectId` 选界面，
        // 故先恢复再放行，欢迎页就不会先画一帧再被换成工作区。
        if (rememberedExists) {
          get().openProject(remembered);
        }

        // 刻意不选默认项目：`activeProjectId` 只由上面那次恢复或用户的 `openProject`
        // 决定；没有记住的项目时它保持 `''`，欢迎页选择器与树的"无项目"状态都以它为准。
        set({
          projects,
          requestSettings,
          responseHistoryKeep,
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

      // 第 2 层刻意不 await：await 会让启动画面多等三趟欢迎页根本不渲染的往返。
      void get().loadWorkspace();
    },

    loadWorkspace: async () => {
      // 两个 flag 都做幂等：三个调用方（`bootstrap`、`openProject`、重试）
      // 互不协调。
      //
      // 在途分支返回*未决*的 promise 而不是立即 resolve：这个区别必须成立 ——
      // 否则 await 此处、以便在树就绪后接续工作的调用方会立刻继续，看到加载前状态。
      if (get().nodesLoaded) return;
      if (inFlight) return inFlight;
      return get().reloadWorkspace();
    },

    // 加载本体。`loadWorkspace` 是它加两条幂等早退的包装：两者必须跑同一段代码，
    // 否则「先加载后重载」与「直接重载」会在 `nodesLoaded` 的置位时机上分叉。
    reloadWorkspace: async () => {
      // 每一趟持有自己的 promise：结束时**只有自己仍是当前那趟**才把槽位清掉。
      // 无条件清会让先结束的那趟把后面那一趟的在途标记抹掉——那一趟明明还在跑，
      // 下一个 `loadWorkspace` 却会以为没人占用槽位，再发一轮同样的 IPC。
      let local: Promise<void> | null = null;
      local = (async () => {
        set({ nodesLoading: true, nodesError: null });

        try {
          // 一次树读取覆盖全树；`endpointDetails` 是第二次调用，取树自身从不
          // 渲染的请求配置。
          const [nodes, endpointDetails, environments] = await Promise.all([
            invokeCmd<ResourceNode[]>(COMMANDS.nodesList),
            loadEndpointDetails(),
            invokeCmd<Environment[]>(COMMANDS.environmentsList),
          ]);

          // 提交时重解激活环境。用户在本趟仍在途时点项目卡，`setActiveProject` 会
          // 对着空列表解析并落到 `''`，顶栏就显示 "—"。这里再挑一次环境来补上这个
          // 窗口；id 已正确时是空操作。
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
              // 展开状态不在服务端，整份替换 `nodes` 会抹掉它。
              nodes: mergeExpanded(s.nodes, nodes),
              endpointDetails,
              environments,
              nodesLoaded: true,
              // 树已经读回来了，先前那一趟的失败消息已经过期，故清掉 `nodesError`。
              nodesError: null,
              ...(env ? { activeEnvironmentId: env.id } : {}),
            };
          });
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          console.error('[store] workspace load failed', error);
          // 只有当前那一趟才报它的失败：被后一趟取代的失败是过期消息，
          // 写上去会让「树已读回来」与「读取失败」同时挂在屏幕上。
          if (inFlight === local) {
            set({ nodesError: message });
          }
        } finally {
          // 加载标志与槽位都归「当前那一趟」所有：不是它就别动，否则会把还在跑的
          // 那一趟说成已经结束。
          if (inFlight === local) {
            set({ nodesLoading: false });
            // `inFlight` 最后清：每个 awaiter 都已拿到这个 promise，且从 `nodesError`
            // 横幅发起的重试必须能开新一趟，而不是并入已结算的那趟。
            inFlight = null;
          }
        }
      })();
      inFlight = local;

      return local;
    },

    clearBootstrapError: () => set({ bootstrapError: null }),

    refreshProjects: async () => {
      try {
        const projects = await invokeCmd<Project[]>(COMMANDS.projectsList);
        // 整份替换 `projects`：项目名与颜色都在项目行上。
        set({ projects });
      } catch (e) {
        // `refreshProjects` 不抛出：调用方是「导入已经落库」之后才来刷新的，一次刷新失败
        // 不该把导入报成失败。界面上顶多仍旧看不见新项目，刷新一次就好。
        console.error('[store] projects refresh failed', e);
      }
    },
  };
};
