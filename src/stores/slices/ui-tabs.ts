import type { StateCreator } from 'zustand';
import type { AppState } from '../app-store';
import { SETTINGS_TAB_ID } from '../store-types';
import type {
  DraftEndpoint,
  EndpointDraft,
  OpenTab,
  ProjectTabSet,
  SettingsCategoryId,
  TabKind,
} from '../store-types';

/**
 * ui-tabs —— 工作区标签条与每个 endpoint 的编辑草稿。
 *
 * 为什么合成一个关注点：`endpointDrafts` 与 `openTabs` 是同一件事的两面：
 * 草稿是标签变脏的**原因**，脏点是标签报告自己有草稿的**方式**。本文件里
 * 每个动 `openTabs` 的动作都同步维护 `dirtyTabIds`，每个动 `endpointDrafts`
 * 的动作也镜像进 `dirtyTabIds`；拆到两个文件会把这一条不变量摊开。
 *
 * `dirty-tabs-slice` 是另一回事：它管的是**关闭**时的守卫（未保存改动询问
 * 用户的那个队列），不是脏标记本身，通过共享的 `AppState` 读 `dirtyTabIds`。
 *
 * 按关注点而非按 slice 分文件，是因为下面多个动作会写别人的 state
 * （`activeTabId` 的切换必须与它所指的标签表原子完成），见 `../ui-slice.ts`。
 */

export interface UiTabsState {
  /**
   * 右栏工作区的已打开标签。标签按底层实体 id 标识，所以对同一实体重复
   * `openTab` 只会激活已有标签，不会新开一个。
   */
  openTabs: OpenTab[];
  activeTabId: string | null;
  /**
   * 调用方标记为"脏"（有未保存改动）的标签 id。WorkspaceTabs 读它画"•"，
   * "关闭已保存"右键项用它决定保留哪些标签。
   *
   * 脏的来源：HttpTester 在 `endpointDrafts[id]` 非空时把 id 推进这个集合；
   * EntitySettings 为每个在途的 `InlineEditableName`（标题或子列表改名）
   * 递增计数，并对其父标签调 `markTabDirty`。
   */
  dirtyTabIds: Set<string>;
  /**
   * 每个 endpoint 未保存的 URL / method 草稿。放在 store 而不是 HttpTester
   * 的局部 state，切走标签再切回来才留得住用户正在输入的内容——否则在 A 上
   * 输入的 URL 会在点开 B 的瞬间消失。这里的 key 也正是 WorkspaceTabs
   * 判定该标签为脏的依据。
   */
  endpointDrafts: Record<string, EndpointDraft | undefined>;
  /**
   * 设置标签内部当前的分节（左栏选中项）。存进 store 是为了让 TopBar 的
   * 深链（环境切换器里的"新建环境"）在设置标签尚未打开时也能直接落到对应分节。
   */
  activeSettingsSection: SettingsCategoryId;
  /**
   * 每个项目各自的标签条，在本次运行内为"用户正在别处干活"而暂存起来。
   *
   * 标签条一次只显示一个项目的标签，其余在这里等着。未保存的工作因此天然
   * 扛过一次切换，无需另行拷贝，原因见 {@link ProjectTabSet}。
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
   * 保存一个 endpoint 的未保存编辑。传空 patch 是空操作；传 `null` 显式清空
   * 草稿。脏标记自动镜像——空草稿即干净，非空即脏。所有请求配置编辑器
   * （URL 栏、method 选择、params、headers、body、auth、文档、tags）都走这里。
   */
  setEndpointDraft: (endpointId: string, draft: Partial<EndpointDraft> | null) => void;
  /** 丢掉一个 endpoint 的草稿（保存时调用）。 */
  clearEndpointDraft: (endpointId: string) => void;
  /**
   * 内部使用：丢掉底层实体已不存在（或祖先已被删）的标签。由各 delete* 动作
   * 调用，避免 UI 停在悬空标签上。暴露成动作是为了覆盖 WorkspaceTabs 的清理
   * useEffect 可能漏掉的边界。
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
   * 由 `openProject` / `leaveProject` 在活动项目**改变之前**调用，因为"暂存什么"
   * 要先回答"我现在显示的每个标签属于哪个项目"，而这个答案只有对着当前在前的
   * 项目才有意义。
   *
   * 传 `''`（退回欢迎页）会暂存并清空标签条。设置标签永远不暂存：它是全局单例
   * 面板，不是项目 A 借给项目 B 又不能让 B 看见的东西。
   */
  switchProjectTabs: (projectId: string) => void;
  /** 打开（单例的）设置标签并选中某一节。幂等——已打开就只切换活动节并激活标签。 */
  openSettingsTab: (section?: SettingsCategoryId) => void;
  /** 更新设置标签的活动分节。 */
  setActiveSettingsSection: (section: SettingsCategoryId) => void;
}

export type UiTabs = UiTabsState & UiTabsActions;

export const createUiTabs: StateCreator<AppState, [], [], UiTabs> = (set, get) => ({
  // 启动时没有标签——bootstrap 序列会在后端返回数据后激活第一个 endpoint。
  openTabs: [],
  activeTabId: null,
  // 初始为空：第一个 endpoint 没有任何未保存内容，标签条不该给它画脏点。
  // 子组件在草稿产生时（如 URL 栏编辑、行内标题改名）推进这个集合。
  dirtyTabIds: new Set<string>(),
  // 启动时为空。HttpTester 每次编辑 URL / method 都写入这张表，切标签时保留，
  // 所以在 A 上输入的草稿回到 A 时还在。
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
      if (s.activeTabId === tabId) {
        // 先选前一个，再选后一个，最后 null。多数 IDE 标签条（Chrome、VS Code）都是这个次序。
        const fallback = s.openTabs[idx - 1] ?? s.openTabs[idx + 1] ?? null;
        nextActive = fallback ? fallback.id : null;
      }
      // 关掉 endpoint 标签时一并丢掉它的草稿与脏标记，否则草稿永远留着。
      // draft endpoint 标签同样适用：它的 id 不在 `nodes` 里，这里是唯一能回收
      // 该 `endpointDrafts` 条目的地方。关标签本身仍不写数据库，丢弃
      // `DraftEndpoint` 是 `discardDraftEndpoint` 的事。
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
        // draft endpoint 只活在 `draftEndpoints` 里，关掉它的标签就是回收那一行的唯一
        // 时刻。只删上面的 `endpointDrafts` 会把文档字段留在这个再没人引用的 id
        // 下——草稿活得比标签还久、已经存不下去，而该 id 一旦被复用它就会诈尸。
        // 在同一次 `set` 里回收，而不是事后补一次 `discardDraftEndpoint`，
        // 这样失败也不会让这张表与标签对不上。
        draftEndpoints = { ...draftEndpoints };
        delete draftEndpoints[closing.id];
      }
      return {
        openTabs: nextTabs,
        activeTabId: nextActive,
        endpointDrafts,
        dirtyTabIds,
        draftEndpoints,
      };
    }),
  setActiveTab: (tabId) =>
    set((s) => {
      if (tabId === null) {
        return { activeTabId: null };
      }
      // 拒绝未知 id——不能让 UI 停在悬空标签上。
      if (!s.openTabs.some((t) => t.id === tabId)) return {};
      return { activeTabId: tabId };
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
        // 显式清空。
        next = undefined;
      } else {
        // patch 与现有草稿合并；全为 undefined 时收成 undefined，脏标记才能翻回干净。
        const merged: EndpointDraft = { ...current, ...draft };
        // 用通用的"是否为空"判断而不是手写字段表：新增草稿字段不该顺带要求改这一行，
        // 否则那个字段会静悄悄地永远不把标签标脏。
        next = Object.values(merged).some((v) => v !== undefined) ? merged : undefined;
      }
      // 新草稿与现有相同时是空操作。返回 `{}` 而非 `s` 引用，是为了让 Zustand
      // 的浅比较保持稳定——返回 `s` 语义上没错，但会触发部分消费者依赖的
      // 严格模式下引用相等检查。
      if (next === undefined && !(endpointId in s.endpointDrafts)) return {};
      if (next !== undefined && sameDraft(current, next)) return {};
      // 同步镜像到 dirtyTabIds，标签条与"关闭已保存"才看得见。
      const nextDirty = new Set(s.dirtyTabIds);
      if (next) nextDirty.add(endpointId);
      else nextDirty.delete(endpointId);
      // 以不可变方式重建 endpointDrafts。
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
      // 第二层 bootstrap 还没落地，`s.nodes` 是空的——这读起来和"所有树节点都被删了"
      // 一模一样。此刻跑过滤会把用户所有 module / collection / endpoint 标签关掉。
      // 直接返回；调用它的 effect 会在 `nodesLoaded` 翻成 true 时重跑，
      // 那是答案第一次可信的时刻。
      if (!s.nodesLoaded) return {};
      const valid = s.openTabs.filter((t) => {
        if (t.kind === 'settings') return true; // 单例，永不失效
        // draft endpoint 按定义就没有 `nodes` 行——那正是它的意义——
        // 下面的查找会读成"已删除"，于是标签一开就被关掉。保存（会开出真正的
        // `endpoint` 标签）或丢弃之前一律豁免。
        if (t.kind === 'endpoint-draft') return true;
        if (t.kind === 'project') return s.projects.some((p) => p.id === t.id);
        // 动态变量是**项目级**的页面，标签 id 就是项目 id——项目还在，页面就还有内容。
        if (t.kind === 'dynamic-variables') return s.projects.some((p) => p.id === t.id);
        // 其余标签都是树节点，且标签的 `kind` 就是节点的 `type`——一次查找同时
        // 覆盖 module、collection、endpoint。连 type 一起比，标签才不会因为别的
        // 节点恰好同 id 而侥幸存活。
        return s.nodes.some((n) => n.id === t.id && n.kind === t.kind);
      });
      if (valid.length === s.openTabs.length) return {};
      let nextActive = s.activeTabId;
      if (nextActive !== null && !valid.some((t) => t.id === nextActive)) {
        nextActive = valid[valid.length - 1]?.id ?? null;
      }
      // 同时丢掉已不存在标签的脏标记——否则这个集合会随删除操作无限膨胀。
      const nextDirty = new Set<string>();
      for (const t of valid) {
        if (s.dirtyTabIds.has(t.id)) nextDirty.add(t.id);
      }
      // 同时丢掉 endpoint 已消失的草稿。draft endpoint 自己的 id 不在 `nodes` 里，
      // 所以按标签种类而不是按查找保留——否则用户仍开着的标签上编辑的内容会凭空消失。
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
      };
    }),
  closeAllTabs: () =>
    set({
      openTabs: [],
      activeTabId: null,
      dirtyTabIds: new Set(),
      endpointDrafts: {},
      // 与上面同一条理由、上一级：关掉的 draft 标签就是一份再也存不下去的草稿，
      // 所以"全关"必须把文档一起带走。
      draftEndpoints: {},
    }),
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
      // 丢掉被关掉的 endpoint 标签的草稿。draft-endpoint 标签也算：它们就是普通标签，
      // 标签关掉时那个 id 下的编辑也该一起走。
      const nextDrafts: Record<string, EndpointDraft | undefined> = {};
      // ……以及它们的文档，同一条理由：标签没了的草稿永远存不下去。这次清扫确实
      // 会发生——draft 标签在创建时就被标脏所以本动作会绕开它，但用户把编辑清回
      // 存储默认值时草稿也会变干净。
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
      };
    });
    return toClose.length;
  },

  switchProjectTabs: (projectId) =>
    set((s) => {
      const current = s.activeProjectId;
      // 重新进入已经在前的项目不算切换。没有这道守卫，把同一个项目开两次就会
      // 先暂存它的标签、再从缓存取回来——空转一圈还会抖遍所有引用，可能丢掉活动标签。
      if (current === projectId) return {};

      const ownerOf = (t: OpenTab) => tabOwningProject(t, s);
      // `current !== ''` 正是让设置单例在退回欢迎页时不被暂存的条件：
      // 没有项目在前时，它的 owner 会匹配到空字符串。
      const stashed = s.openTabs.filter((t) => current !== '' && ownerOf(t) === current);
      const staying = s.openTabs.filter((t) => !(current !== '' && ownerOf(t) === current));

      const projectTabs = { ...s.projectTabs };
      if (current !== '' && stashed.length > 0) {
        projectTabs[current] = {
          tabs: stashed,
          // 只记住确实被暂存的那个活动标签——否则取回时标签条会选中一个
          // 切换前就已经关掉的标签。
          activeTabId:
            s.activeTabId && stashed.some((t) => t.id === s.activeTabId) ? s.activeTabId : null,
        };
      }

      if (projectId === '') {
        // 退回欢迎页：标签条清空，暂存的份原样留着。
        return { projectTabs, openTabs: staying, activeTabId: null };
      }

      const remembered = projectTabs[projectId];
      const tabs = [...staying, ...(remembered?.tabs ?? [])];
      // 每个项目都得给标签条留一个自己的标签——这样"我在哪个项目"才能一眼从
      // 标签条读出来。没有缓存的项目现在就补一个；回来的项目保留原有那个。
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
});

/**
 * 该种类的标签是否拥有 `endpointDrafts` 里的一条目。
 *
 * `endpoint` 与 `endpoint-draft` 都为真：draft endpoint 走的是与已存 endpoint
 * **同一套**请求配置编辑器，`endpointDrafts` 两种情况都按标签 id 作键，
 * 区别只在 `nodes` 里有没有行。合成一个 helper，是为了让重建那张表的三次清扫
 * 不会各自走偏——任何一次悄悄漏掉 draft 种类，都会在树刷新时丢掉用户正在输入的编辑。
 */
function carriesEndpointDraft(kind: TabKind): boolean {
  return kind === 'endpoint' || kind === 'endpoint-draft';
}

/**
 * 不属于任何项目的标签所用的 owner 标记。
 *
 * 与 `''`（欢迎页、未开项目）区分开。混为一谈会让设置标签变成可暂存的：
 * 退出时它的 owner 会等于空项目 id，于是被扫进某个项目缓存，用户正开着的面板被关掉。
 */
const GLOBAL_TAB_OWNER = '*';

/**
 * 一个标签属于哪个项目。
 *
 * 凡是背后有实体的，都通过树已经反规范化到每一行上的那个键解析：`rootId`。
 * draft endpoint 没有行可读，所以在 `DraftEndpoint` 上带着同一个键——出生时
 * 快照，选哪条标签条够用，而且不会因为要紧的理由变陈旧（父节点搬走了的草稿
 * 仍属于它创建时所在的项目）。
 *
 * 找不到的节点返回 `''`。实际上 `pruneStaleTabs` 会先退役这样的标签，所以这
 * 最多是瞬时答案——而把"未知"当成"不属于任何人"，可以防止半加载的树把另一个
 * 项目的标签一起带走。
 */
function tabOwningProject(tab: OpenTab, state: AppState): string {
  if (tab.kind === 'settings') return GLOBAL_TAB_OWNER;
  if (tab.kind === 'project') return tab.id;
  // 动态变量页面是项目级的，标签 id 就是项目 id。
  if (tab.kind === 'dynamic-variables') return tab.id;
  if (tab.kind === 'endpoint-draft') return state.draftEndpoints[tab.id]?.rootId ?? '';
  return state.nodes.find((n) => n.id === tab.id)?.rootId ?? '';
}

/**
 * 逐字段比较草稿是否相等。
 *
 * 这里引用比较就是诚实的信号：编辑器只在用户真的改了东西时才交出新的数组 /
 * 对象，所以结构化遍历在每次击键上花的钱多于它买的东西。字段表从 `next` 读出
 * 而非硬编码，和上面"是否为空"的通用判断是同一个理由——新增草稿字段不该
 * 要求再改本文件一处来避免无操作的抖动。
 */
function sameDraft(a: EndpointDraft | undefined, b: EndpointDraft): boolean {
  if (!a) return false;
  return (Object.keys(b) as (keyof EndpointDraft)[]).every((k) => a[k] === b[k]);
}
