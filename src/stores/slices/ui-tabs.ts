import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import { SETTINGS_TAB_ID, dynamicTabId, dynamicTabProjectId } from '../store-types';
import type {
  DraftEndpoint,
  EndpointDraft,
  OpenTab,
  ProjectTabSet,
  SettingsCategoryId,
  TabKind,
} from '../store-types';
import type { EndpointResponseState } from './ui-response';

/**
 * ui-tabs —— 工作区标签条与每个 endpoint 的编辑草稿。
 *
 * 为什么合成一个关注点：`endpointDrafts` 与 `openTabs` 是同一件事的两面：草稿是标签变脏的
 * **原因**，脏点是标签报告自己有草稿的**方式**。本文件里每个动 `openTabs` 的动作都同步维护
 * `dirtyTabIds`，每个动 `endpointDrafts` 的动作也镜像进 `dirtyTabIds`。
 *
 * `dirty-tabs-slice` 是另一回事：它管的是**关闭**时的守卫（未保存改动询问用户的那个队列），
 * 不是脏标记本身，通过共享的 `AppState` 读 `dirtyTabIds`。
 *
 * 按关注点而非按 slice 分文件，是因为下面多个动作会写别人的 state（`activeTabId` 的切换
 * 必须与它所指的标签表原子完成），见 `../ui-slice.ts`。
 */

export interface UiTabsState {
  /**
   * 右栏工作区的已打开标签。标签按底层实体 id 标识，故对同一实体重复 `openTab`
   * 只会激活已有标签，不会新开一个。
   */
  openTabs: OpenTab[];
  activeTabId: string | null;
  /**
   * 调用方标记为「脏」（有未保存改动）的标签 id。WorkspaceTabs 读它画「•」，
   * 「关闭已保存」右键项用它决定保留哪些标签。
   *
   * 脏的来源：HttpTester 在 `endpointDrafts[id]` 非空时把 id 推进这个集合；EntitySettings
   * 为每个在途的 `InlineEditableName`（标题或子列表改名）递增计数，并对其父标签调 `markTabDirty`。
   */
  dirtyTabIds: Set<string>;
  /**
   * 每个 endpoint 未保存的 URL / method 草稿。放在 store 而不是 HttpTester 的局部 state，
   * 切走标签再切回来才留得住用户正在输入的内容。这里的 key 也正是 WorkspaceTabs 判定
   * 该标签为脏的依据。
   */
  endpointDrafts: Record<string, EndpointDraft | undefined>;
  /**
   * 设置标签内部当前的分节（左栏选中项）。存进 store 是为了让 TopBar 的深链
   * （环境切换器里的「新建环境」）在设置标签尚未打开时也能直接落到对应分节。
   */
  activeSettingsSection: SettingsCategoryId;
  /**
   * 每个项目各自的标签条，在本次运行内为「用户正在别处干活」而暂存起来。标签条一次只显示
   * 一个项目的标签，其余在这里等着。未保存的工作因此在切换项目后原样保留，原因见
   * {@link ProjectTabSet}。
   */
  projectTabs: Record<string, ProjectTabSet>;
}

export interface UiTabsActions {
  /**
   * 为给定实体打开标签（或激活已有的）。当前没有任何标签时，新标签自动成为
   * 活动标签。
   */
  openTab: (kind: TabKind, refId: string) => void;
  /** 关闭一个标签。若它是活动标签，则回退到前一个标签或 null。 */
  closeTab: (tabId: string) => void;
  /** 切换右栏显示的标签。未知 id 是空操作。 */
  setActiveTab: (tabId: string | null) => void;
  /** 标记标签有无未保存改动，驱动标签条的脏指示与"关闭已保存"动作。 */
  markTabDirty: (tabId: string, dirty: boolean) => void;
  /**
   * 保存一个 endpoint 的未保存编辑。传空 patch 是空操作；传 `null` 显式清空草稿。脏标记自动
   * 镜像——空草稿即干净，非空即脏。所有请求配置编辑器（URL 栏、method 选择、params、
   * headers、body、auth、文档、tags）都走这里。
   */
  setEndpointDraft: (endpointId: string, draft: Partial<EndpointDraft> | null) => void;
  /** 丢掉一个 endpoint 的草稿（保存时调用）。 */
  clearEndpointDraft: (endpointId: string) => void;
  /**
   * 内部使用：丢掉底层实体已不存在（或祖先已被删）的标签。由各 delete* 动作调用，避免 UI
   * 停在悬空标签上。暴露成动作是为了覆盖 WorkspaceTabs 的清理 useEffect 可能漏掉的边界。
   */
  pruneStaleTabs: () => void;
  /** 一次关掉所有已打开标签。右键菜单使用。 */
  closeAllTabs: () => void;
  /**
   * 关掉所有**不在** `dirtyTabIds` 里的（即所有"已保存"的）标签。若唯一的
   * 标签是脏的，它会留下。返回关掉的标签数，调用方可用它出 toast。
   */
  closeSavedTabs: () => number;
  /**
   * 让 `projectId` 的标签集成为可见的，并把当前项目的那份暂存进 `projectTabs`。
   *
   * 由 `openProject` / `leaveProject` 在活动项目**改变之前**调用，因为「暂存什么」要先回答
   * 「我现在显示的每个标签属于哪个项目」，而这个答案只有对着当前在前的项目才有意义。
   *
   * 传 `''`（退回欢迎页）会暂存并清空标签条。设置标签永远不暂存：它是全局单例面板，
   * 不是项目 A 借给项目 B 又不能让 B 看见的东西。
   */
  switchProjectTabs: (projectId: string) => void;
  /** 打开（单例的）设置标签并选中某一节。幂等——已打开就只切换活动节并激活标签。 */
  openSettingsTab: (section?: SettingsCategoryId) => void;
  /**
   * 打开当前项目的动态变量标签。幂等——已打开就只激活它。
   *
   * 动态变量挂在项目里，故这个标签是**项目内**的标签页：每个项目有自己的一条，标签 id 里
   * 带着项目 id。没有活动项目时开不了（欢迎页只提示「先打开一个项目」）。
   */
  openDynamicVariablesTab: () => void;
  /** 更新设置标签的活动分节。 */
  setActiveSettingsSection: (section: SettingsCategoryId) => void;
}

export type UiTabs = UiTabsState & UiTabsActions;

export const createUiTabs: StateCreator<AppState, [], [], UiTabs> = (set, get) => ({
  // 启动时没有标签——bootstrap 序列会在后端返回数据后激活第一个 endpoint。
  openTabs: [],
  activeTabId: null,
  // 初始为空：第一个 endpoint 没有任何未保存内容，标签条不该给它画脏点。子组件在草稿产生时
  // （如 URL 栏编辑、行内标题改名）推进这个集合。
  dirtyTabIds: new Set<string>(),
  // 启动时为空。HttpTester 每次编辑 URL / method 都写入这张表，切标签时保留，故在 A 上输入的草稿回到 A 时还在。
  endpointDrafts: {},
  // 设置标签默认落在"环境"——多数用户点顶栏设置按钮时最期待这里。
  activeSettingsSection: 'env',
  projectTabs: {},

  openTab: (kind, refId) =>
    set((s) => {
      if (s.openTabs.some((t) => t.kind === kind && t.id === refId)) {
        return { activeTabId: refId };
      }
      return {
        openTabs: [...s.openTabs, { id: refId, kind }],
        activeTabId: refId,
      };
    }),
  closeTab: (tabId) =>
    set((s) => {
      const idx = s.openTabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return {};
      const nextTabs = s.openTabs.filter((t) => t.id !== tabId);
      let nextActive = s.activeTabId;
      // 标签条换了活动标签，树里的高亮也要跟着换。回退到的标签是接口时，高亮落在它那一行；
      // 回退到别的种类时，高亮保持原样。
      let selection: Selection = {};
      if (s.activeTabId === tabId) {
        // `nextActive` 先取前一个，再取后一个，最后 null。多数 IDE 标签条（Chrome、VS Code）都是这个次序。
        const fallback = s.openTabs[idx - 1] ?? s.openTabs[idx + 1] ?? null;
        nextActive = fallback ? fallback.id : null;
        selection = fallback ? selectionFor(fallback) : {};
      }
      // `closeTab` 关掉 endpoint 标签时一并丢掉它的草稿与脏标记，否则草稿永远留着。draft
      // endpoint 标签同样适用：它的 id 不在 `nodes` 里，这里是唯一能回收该 `endpointDrafts`
      // 条目的地方。关标签本身仍不写数据库，丢弃 `DraftEndpoint` 是 `discardDraftEndpoint` 的事。
      const closing = s.openTabs[idx];
      let endpointDrafts = s.endpointDrafts;
      let dirtyTabIds = s.dirtyTabIds;
      let draftEndpoints = s.draftEndpoints;
      if (closing && carriesEndpointDraft(closing.kind)) {
        if (s.endpointDrafts[closing.id]) {
          endpointDrafts = { ...s.endpointDrafts };
          delete endpointDrafts[closing.id];
        }
        if (s.dirtyTabIds.has(closing.id)) {
          dirtyTabIds = new Set(s.dirtyTabIds);
          dirtyTabIds.delete(closing.id);
        }
      }
      if (closing && closing.kind === 'endpoint-draft' && closing.id in draftEndpoints) {
        // draft endpoint 只活在 `draftEndpoints` 里，关掉它的标签就是回收那一行的唯一时刻。
        // 只删上面的 `endpointDrafts` 会把文档字段留在这个再没人引用的 id 下——该 id 一旦被
        // 复用，那份草稿就会出现在复用它的标签下。在同一次 `set` 里回收，而不是事后补一次
        // `discardDraftEndpoint`，这样失败也不会让这张表与标签对不上。
        draftEndpoints = { ...draftEndpoints };
        delete draftEndpoints[closing.id];
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        ...selection,
        endpointDrafts,
        dirtyTabIds,
        draftEndpoints,
        // 响应是挂在标签上的，故关标签要把它带走——同一套规矩的第四样东西。
        lastResponses: withoutResponses(s.lastResponses, [tabId]),
      };
    }),
  setActiveTab: (tabId) =>
    set((s) => {
      if (tabId === null) {
        return { activeTabId: null };
      }
      // `setActiveTab` 拒绝未知 id——不能让 UI 停在悬空标签上。
      const target = s.openTabs.find((t) => t.id === tabId);
      if (!target) return {};
      // 标签条与资源树要指向同一个接口：标签条读 `activeTabId`，树里的高亮读
      // `activeEndpointId`。只写前者时，用户点标签条切到另一个接口，树里的高亮仍停在原来
      // 那一行——两边说的不是同一个接口。
      return { activeTabId: tabId, ...selectionFor(target) };
    }),
  markTabDirty: (tabId, dirty) =>
    set((s) => {
      const isDirty = s.dirtyTabIds.has(tabId);
      if (dirty && !isDirty) {
        const next = new Set(s.dirtyTabIds);
        next.add(tabId);
        return { dirtyTabIds: next };
      }
      if (!dirty && isDirty) {
        const next = new Set(s.dirtyTabIds);
        next.delete(tabId);
        return { dirtyTabIds: next };
      }
      return {};
    }),
  setEndpointDraft: (endpointId, draft) =>
    set((s) => {
      const current = s.endpointDrafts[endpointId];
      let next: EndpointDraft | undefined;
      if (draft === null) {
        next = undefined;
      } else {
        // 这里把 patch 与现有草稿合并；全为 undefined 时收成 undefined，脏标记才能翻回干净。
        const merged: EndpointDraft = { ...current, ...draft };
        // 用通用的「是否为空」判断而不是手写字段表：新增草稿字段不该顺带要求改这一行，
        // 否则那个字段会一直不把标签标脏。
        next = Object.values(merged).some((v) => v !== undefined) ? merged : undefined;
      }
      // `next` 与现有草稿相同时是空操作。返回 `{}` 而非 `s` 引用，是为了让 Zustand 的浅比较
      // 保持稳定——返回 `s` 语义上没错，但会触发部分消费者依赖的严格模式下引用相等检查。
      if (next === undefined && !(endpointId in s.endpointDrafts)) return {};
      if (next !== undefined && sameDraft(current, next)) return {};
      // 这里把结果同步镜像到 `dirtyTabIds`，标签条与「关闭已保存」才看得见。
      const nextDirty = new Set(s.dirtyTabIds);
      if (next) nextDirty.add(endpointId);
      else nextDirty.delete(endpointId);
      const nextDrafts: Record<string, EndpointDraft | undefined> = { ...s.endpointDrafts };
      if (next) nextDrafts[endpointId] = next;
      else delete nextDrafts[endpointId];
      return { endpointDrafts: nextDrafts, dirtyTabIds: nextDirty };
    }),
  clearEndpointDraft: (endpointId) =>
    set((s) => {
      if (!(endpointId in s.endpointDrafts)) return {};
      const nextDrafts: Record<string, EndpointDraft | undefined> = { ...s.endpointDrafts };
      delete nextDrafts[endpointId];
      const nextDirty = new Set(s.dirtyTabIds);
      nextDirty.delete(endpointId);
      return { endpointDrafts: nextDrafts, dirtyTabIds: nextDirty };
    }),
  pruneStaleTabs: () =>
    set((s) => {
      // 第二层 bootstrap 还没返回，`s.nodes` 是空的——这读起来和「所有树节点都被删了」一模一样。
      // 此刻跑过滤会把用户所有 module / collection / endpoint 标签关掉，故这里直接返回；
      // 调用它的 effect 会在 `nodesLoaded` 翻成 true 时重跑，那是答案第一次可信的时刻。
      if (!s.nodesLoaded) return {};
      const valid = s.openTabs.filter((t) => {
        if (t.kind === 'settings') return true; // 单例，永不失效
        // 动态变量标签属于项目：项目还在它就还在，项目没了它跟着走。
        if (t.kind === 'dynamic-variables') {
          const projectId = dynamicTabProjectId(t.id);
          return projectId !== '' && s.projects.some((p) => p.id === projectId);
        }
        // draft endpoint 按定义就没有 `nodes` 行——那正是它的意义——下面的查找会读成「已删除」，
        // 于是标签一开就被关掉。保存（会开出真正的 `endpoint` 标签）或丢弃之前一律豁免。
        if (t.kind === 'endpoint-draft') return true;
        if (t.kind === 'project') return s.projects.some((p) => p.id === t.id);
        // 其余标签都是树节点，且标签的 `kind` 就是节点的 `type`——一次查找同时覆盖 module、
        // collection、endpoint。连 type 一起比，标签才不会因为别的节点恰好同 id 而侥幸存活。
        return s.nodes.some((n) => n.id === t.id && n.kind === t.kind);
      });
      if (valid.length === s.openTabs.length) return {};
      let nextActive = s.activeTabId;
      if (nextActive !== null && !valid.some((t) => t.id === nextActive)) {
        nextActive = valid[valid.length - 1]?.id ?? null;
      }
      // 这里同时丢掉已不存在标签的脏标记——否则这个集合会随删除操作无限膨胀。
      const nextDirty = new Set<string>();
      for (const t of valid) {
        if (s.dirtyTabIds.has(t.id)) nextDirty.add(t.id);
      }
      // 这里同时丢掉 endpoint 已消失的草稿。draft endpoint 自己的 id 不在 `nodes` 里，故按标签
      // 种类而不是按查找保留——否则用户仍开着的标签上编辑的内容会凭空消失。
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      for (const t of valid) {
        if (carriesEndpointDraft(t.kind) && s.endpointDrafts[t.id]) {
          nextDrafts[t.id] = s.endpointDrafts[t.id];
        }
      }
      return {
        openTabs: valid,
        activeTabId: nextActive,
        dirtyTabIds: nextDirty,
        endpointDrafts: nextDrafts,
        // 被清扫掉的标签（底层实体已经没了）带走的那份响应。留下的标签继续用自己那份，故这里只删被移除的那些。
        lastResponses: withoutResponses(
          s.lastResponses,
          s.openTabs.filter((t) => !valid.some((v) => v.id === t.id)).map((t) => t.id),
        ),
      };
    }),
  closeAllTabs: () =>
    set((s) => ({
      openTabs: [],
      activeTabId: null,
      dirtyTabIds: new Set(),
      endpointDrafts: {},
      // 与上面同一条理由、上一级：关掉的 draft 标签就是一份再也存不下去的草稿，故「全关」必须把文档一起带走。
      draftEndpoints: {},
      // 全关 ⇒ 所有标签都没了 ⇒ 所有响应一起走。
      lastResponses: withoutResponses(s.lastResponses, Object.keys(s.lastResponses)),
    })),
  closeSavedTabs: () => {
    const state = get();
    const toClose = state.openTabs.filter((t) => !state.dirtyTabIds.has(t.id));
    if (toClose.length === 0) return 0;
    set((s) => {
      const closeSet = new Set(toClose.map((t) => t.id));
      const nextTabs = s.openTabs.filter((t) => !closeSet.has(t.id));
      let nextActive = s.activeTabId;
      if (nextActive !== null && closeSet.has(nextActive)) {
        nextActive = nextTabs[nextTabs.length - 1]?.id ?? null;
      }
      // 这里丢掉被关掉的 endpoint 标签的草稿。draft-endpoint 标签也算：它们就是普通标签，
      // 标签关掉时那个 id 下的编辑也该一起走。
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      // ……以及它们的文档，同一条理由：标签没了的草稿永远存不下去。这次清扫确实会发生——
      // draft 标签在创建时就被标脏所以本动作会绕开它，但用户把编辑清回存储默认值时草稿也会变干净。
      const nextDraftEndpoints: Record<string, DraftEndpoint> = {};
      for (const t of nextTabs) {
        if (carriesEndpointDraft(t.kind) && s.endpointDrafts[t.id]) {
          nextDrafts[t.id] = s.endpointDrafts[t.id];
        }
        if (t.kind === 'endpoint-draft' && s.draftEndpoints[t.id]) {
          nextDraftEndpoints[t.id] = s.draftEndpoints[t.id];
        }
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        endpointDrafts: nextDrafts,
        draftEndpoints: nextDraftEndpoints,
        // 被关掉的标签带走它的响应——与草稿、文档同一条理由。
        lastResponses: withoutResponses(
          s.lastResponses,
          toClose.map((t) => t.id),
        ),
      };
    });
    return toClose.length;
  },

  switchProjectTabs: (projectId) =>
    set((s) => {
      const current = s.activeProjectId;
      // 这道守卫让重新进入已经在前的项目不算切换。没有它，把同一个项目开两次就会先暂存它的
      // 标签、再从缓存取回来——空转一圈还会抖遍所有引用，可能丢掉活动标签。
      if (current === projectId) return {};

      const ownerOf = (t: OpenTab) => tabOwningProject(t, s);
      // `current !== ''` 正是让设置单例在退回欢迎页时不被暂存的条件：没有项目在前时，它的 owner 会匹配到空字符串。
      const stashed = s.openTabs.filter((t) => current !== '' && ownerOf(t) === current);
      const staying = s.openTabs.filter((t) => !(current !== '' && ownerOf(t) === current));

      const projectTabs = { ...s.projectTabs };
      if (current !== '' && stashed.length > 0) {
        projectTabs[current] = {
          tabs: stashed,
          // 这里只记住确实被暂存的那个活动标签——否则取回时标签条会选中一个切换前就已经关掉的标签。
          activeTabId:
            s.activeTabId && stashed.some((t) => t.id === s.activeTabId) ? s.activeTabId : null,
        };
      }

      if (projectId === '') {
        // 退回欢迎页时标签条清空，暂存的份原样留着。
        return { projectTabs, openTabs: staying, activeTabId: null };
      }

      const remembered = projectTabs[projectId];
      const tabs = [...staying, ...(remembered?.tabs ?? [])];
      // 每个项目都得给标签条留一个自己的标签——这样「我在哪个项目」才能一眼从标签条读出来。
      // 没有缓存的项目现在就补一个；回来的项目保留原有那个。
      if (!tabs.some((t) => t.kind === 'project' && t.id === projectId)) {
        tabs.push({ id: projectId, kind: 'project' });
      }
      // 回到项目应该落在离开时的位置，而不是它的项目卡片上——否则切换会显得失忆。
      const restored = remembered?.activeTabId ?? null;
      const activeTabId = restored && tabs.some((t) => t.id === restored) ? restored : projectId;

      return { projectTabs, openTabs: tabs, activeTabId };
    }),

  openSettingsTab: (section) => {
    const state = get();
    if (section) set({ activeSettingsSection: section });
    const existing = state.openTabs.find((t) => t.kind === 'settings' && t.id === SETTINGS_TAB_ID);
    if (existing) {
      set({ activeTabId: SETTINGS_TAB_ID });
      return;
    }
    set({
      openTabs: [...state.openTabs, { id: SETTINGS_TAB_ID, kind: 'settings' }],
      activeTabId: SETTINGS_TAB_ID,
    });
  },

  setActiveSettingsSection: (section) => set({ activeSettingsSection: section }),

  openDynamicVariablesTab: () => {
    const state = get();
    // 没有活动项目就没有可看的动态变量：定义挂在项目里。
    const projectId = state.activeProjectId;
    if (!projectId) return;
    const tabId = dynamicTabId(projectId);
    const existing = state.openTabs.find((t) => t.id === tabId);
    if (existing) {
      set({ activeTabId: existing.id });
      return;
    }
    set({
      openTabs: [...state.openTabs, { id: tabId, kind: 'dynamic-variables' }],
      activeTabId: tabId,
    });
  },
});

/**
 * 资源树高亮要跟着标签条走时写进 state 的那一个字段。空对象表示「不动高亮」。
 */
type Selection = { activeEndpointId?: string | null };

/**
 * 该种类的标签是否拥有 `endpointDrafts` 里的一条目。`endpoint` 与 `endpoint-draft` 都为真：
 * draft endpoint 走的是与已存 endpoint **同一套**请求配置编辑器，`endpointDrafts` 两种情况都
 * 按标签 id 作键，区别只在 `nodes` 里有没有行。
 *
 * 合成一个 helper，是为了让重建那张表的三次清扫不会各自走偏——任何一次漏掉 draft
 * 种类，都会在树刷新时丢掉用户正在输入的编辑。
 */
function carriesEndpointDraft(kind: TabKind): boolean {
  return kind === 'endpoint' || kind === 'endpoint-draft';
}

/**
 * 某个标签成为活动标签时，资源树里的高亮该落在哪一行。
 *
 * 接口标签落在它自己那一行；草稿标签在树里没有行，故把高亮清空——树里没有可高亮的行时，
 * 让高亮停在另一个接口上，用户看到的就是两个地方各说各话。模块、集合、项目与设置那几类
 * 标签不是接口，故不动高亮。
 */
function selectionFor(tab: OpenTab): Selection {
  if (tab.kind === 'endpoint') return { activeEndpointId: tab.id };
  if (tab.kind === 'endpoint-draft') return { activeEndpointId: null };
  return {};
}

/**
 * 丢掉这些标签带走的**响应状态**。响应是挂在标签上的（见 `slices/ui-response.ts`），故移除
 * 标签的每条路径（关标签、清扫失效标签、全关、删接口、删项目）都要带走那份响应：标签 id
 * 会被复用（同一个接口重新打开就是同一个 id），漏掉一处，新开的标签就会看到上一条命途的响应。
 *
 * 没有要清的键时返回**原引用**：这样这次 `set()` 不会因为一张没变过的表而多出新对象，
 * 订阅它的组件也就不会白白重渲染。
 */
function withoutResponses(
  responses: Record<string, EndpointResponseState>,
  removedTabIds: Iterable<string>,
): Record<string, EndpointResponseState> {
  const next = { ...responses };
  let changed = false;
  for (const id of removedTabIds) {
    if (id in next) {
      delete next[id];
      changed = true;
    }
  }
  return changed ? next : responses;
}

/**
 * 不属于任何项目的标签所用的 owner 标记。与 `''`（欢迎页、未开项目）区分开：混为一谈会让
 * 设置标签变成可暂存的——退出时它的 owner 会等于空项目 id，于是被扫进某个项目缓存。
 */
const GLOBAL_TAB_OWNER = '*';

/**
 * 一个标签属于哪个项目。凡是背后有实体的，都通过树已经反规范化到每一行上的那个键解析：
 * `rootId`。draft endpoint 没有行可读，所以在 `DraftEndpoint` 上带着同一个键——出生时快照，
 * 父节点搬走了的草稿仍属于它创建时所在的项目。
 *
 * 找不到的节点返回 `''`。实际上 `pruneStaleTabs` 会先退役这样的标签，故这最多是瞬时答案——
 * 把「未知」当成「不属于任何人」，可以防止半加载的树把另一个项目的标签一起带走。
 */
function tabOwningProject(tab: OpenTab, state: AppState): string {
  if (tab.kind === 'settings') return GLOBAL_TAB_OWNER;
  // 动态变量标签是项目内的：项目 id 就在标签 id 里。
  if (tab.kind === 'dynamic-variables') return dynamicTabProjectId(tab.id);
  if (tab.kind === 'project') return tab.id;
  if (tab.kind === 'endpoint-draft') return state.draftEndpoints[tab.id]?.rootId ?? '';
  return state.nodes.find((n) => n.id === tab.id)?.rootId ?? '';
}

/**
 * 逐字段比较草稿是否相等。这里引用比较就是诚实的信号：编辑器只在用户真的改了东西时才交出
 * 新的数组 / 对象，故结构化遍历在每次击键上花的钱多于它买的东西。字段表从 `next` 读出而非
 * 硬编码，和上面「是否为空」的通用判断是同一个理由——新增草稿字段不该要求再改本文件一处。
 */
function sameDraft(a: EndpointDraft | undefined, b: EndpointDraft): boolean {
  if (!a) return false;
  return (Object.keys(b) as (keyof EndpointDraft)[]).every((k) => a[k] === b[k]);
}
