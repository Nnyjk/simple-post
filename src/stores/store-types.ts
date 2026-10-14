/**
 * 跨多个切片模块共用的类型与常量声明。
 *
 * 单独成文件（而不是每个切片各声明一遍）是因为好几个切片必须对同一形状达成
 * 一致——例如 UiSlice 的 `openTabs` 与 DirtyTabsSlice 的 `pendingDirtyClose`
 * 都引用 `OpenTab`。规范声明放这里，各切片的类型文件就能保持极小，
 * 避免"两份拷贝各自漂移"这种失败模式。
 *
 * 本文件有意只含类型加几个小的运行时常量。切片工厂只 import，从不调用它；
 * 运行时状态留在切片模块自己那里。
 */
import type { AuthConfig, HttpMethod, KeyValue, RequestBody } from '@/types/domain';

/** 视觉主题预设。驱动 document 根节点上的 CSS 变量。 */
export type Theme = 'dark' | 'light' | 'system';

/** HTTP 执行器的可调项。落 localStorage，免得每次会话都要重设。 */
export interface RequestSettings {
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
}

/**
 * 仍然会弹出**新建对话框**的种类。endpoint 被刻意排除：新建它不需要问任何
 * 问题（你点的节点就是父节点），所以它直接写进前端草稿并落到重命名，
 * 见 `slices/ui-new-item.ts::createEndpointAt`。
 *
 * 注意：`ProjectTree` 为自己的 hook 绑定镜像了同一形状，两者有意保持一致，
 * 树菜单项才能按 store 的动作签名来定类型。
 */
export type NewDraftKind = 'project' | 'module' | 'collection';

/**
 * 右栏工作区能显示的标签种类。
 *
 * `endpoint-draft` 是唯一 id **不是** `nodes` 行的种类：它是渲染侧铸造的
 * `DraftEndpoint` id，只有用户按 Save 时才落库（见
 * `slices/ui-draft-endpoints.ts`）。它是独立种类而不是"少一行的 endpoint"，
 * 是刻意的——每一处"这个实体还在吗？"的清扫（`pruneStaleTabs`、级联删除、
 * 脏关闭队列）都得能区分"已删除"与"还没写入"，混同两者会让新建的草稿标签
 * 在树一刷新的瞬间就被关掉。
 */
export type TabKind =
  'project' | 'module' | 'collection' | 'endpoint' | 'endpoint-draft' | 'settings';

/**
 * 只存在于内存的东西所用的 id 前缀。真实节点 id 是裸 `uid()`，所以 `draft:`
 * 不可能与它相撞——正是这一点让草稿标签、草稿 endpoint 和真实 endpoint
 * 共用同一个 `openTabs` 数组，而不必靠"猜"来判断持有的是哪一种的查找。
 */
export const DRAFT_ID_PREFIX = 'draft:';

/**
 * 只存在于渲染侧的 endpoint。
 *
 * 语义是**整份文档**，不同于 `EndpointDraft`（后者是对已存 `EndpointDetail`
 * 的补丁）。草稿没有可供补丁所依附的存储行，所以保存时要写的每个字段都在这里，
 * 从 `blankDetail` 本该提供的默认值起步。
 *
 * 这个划分正是设计的要点：params / headers / body **不是**这里的字段，因为它们
 * 由请求配置编辑器送来，而那些编辑器对草稿和已存 endpoint 都一样写进
 * `endpointDrafts`。`saveDraftEndpoint` 合并两半，见 `slices/ui-draft-endpoints.ts`。
 */
export interface DraftEndpoint {
  /** 本地铸造的唯一 id，同时用作标签 id。总以 `DRAFT_ID_PREFIX` 开头。 */
  id: string;
  /** endpoint 将被写入的父节点（一个 module 或一个 collection）。 */
  parentId: string;
  /** 所属项目。UI 用它界定草稿范围；不重放的原因见保存路径。 */
  rootId: string;
  /** 接口名——重命名前是 `UNTITLED_ENDPOINT_NAME`，之后就地更新。 */
  name: string;
  /**
   * 一行摘要，用法同 `ResourceNode.description`。
   *
   * 它是节点字段而不是 detail 字段，所以走 `addNode` 自己的 `description`
   * 写入，**不**走被合并的 `detail`——但它之所以存在而不是干脆没有，是因为
   * 工作区的描述编辑器对草稿和对已存 endpoint 一样启用，而一个默默丢弃输入
   * 的编辑器比一个根本不存在的编辑器更糟。
   */
  description: string;
  method: HttpMethod;
  /** 不含任何 baseUrl 的 URL 路径，含义同 `EndpointDetail.url`。 */
  url: string;
  auth: AuthConfig;
  environmentId: string | null;
  baseUrlDefinitionId: string | null;
  baseUrlOverride: string | null;
  tags: string[];
  docs: string;
  createdAt: number;
}

/**
 * draft endpoint 的初始名字。
 *
 * 从 `ui-new-item.ts` 再导出而不是重新声明：两个值相同的常量会让对话框路径
 * 和草稿路径悄无声息地分叉。子工厂从这里 import——它们不得互相 import。
 */
export { UNTITLED_ENDPOINT_NAME } from './slices/ui-new-item';

/** 设置标签内部的分节。驱动左侧栏。 */
export type SettingsCategoryId = 'env' | 'connection' | 'appearance' | 'request';

/**
 * （唯一一个）设置标签所用的哨兵 id。真实实体用自己的 id，但设置是单例面板，
 * 用固定 id 才能让开关与激活逻辑和实体标签完全一致。
 */
export const SETTINGS_TAB_ID = '__settings__';

/**
 * 右栏工作区里一个已打开的标签。
 *
 * `id` 复用底层实体的 id（project / module / collection / endpoint），查找因此
 * 是 O(1)，用户删除实体时 `closeTab` 也能干净地把对应标签摘掉。
 */
export interface OpenTab {
  id: string;
  kind: TabKind;
}

/**
 * 某个项目自己的标签条，用户在别处时暂存起来。
 *
 * `openTabs` 是单个扁平数组，因为标签条一次只渲染一条列表——但那条列表是按项目
 * 分的。属于项目 A 的 endpoint 标签在打开 B 时毫无意义，而 `pruneStaleTabs`
 * 最终会把它当成"不在树里了"退役。所以切项目把出去的那份暂存到这里、
 * 把进来的那份取回来。
 *
 * **只保留标签表**，这正是重点。未保存标签持有的一切——`draftEndpoints[draftId]`、
 * `endpointDrafts[id]`、它在 `dirtyTabIds` 里的条目——都按标签 id 作键，
 * 而切换刻意不碰它们。因此"带着三个脏标签和一个命名好的草稿离开项目 A，
 * 回来发现原样不变"不需要为其中任何一份数据做第二份拷贝：这条记录只说明
 * **哪些**标签要显示，而它们一直挂在原键下的那些映射里内容还在。
 */
export interface ProjectTabSet {
  tabs: OpenTab[];
  /** 其中哪个在前面；用户最后把它关掉时为 `null`。 */
  activeTabId: string | null;
}

/**
 * 每个 endpoint 的未保存编辑。存在 `endpointDrafts`（而不是 HttpTester 的局部
 * state）里，切走标签再切回来才留得住用户正在输入的内容。每个字段各自可选——
 * 只有用户真正编辑过的字段才存在，缺失的字段含义是"与已存的
 * `EndpointDetail` 相同"。
 *
 * 所有请求配置编辑器（params / headers / body / auth / docs / tags）也都写进
 * 这里，所以整个标签遵循同一条规则：**编辑存在草稿里，Save 才落盘，在此之前
 * 标签一直是脏的。**
 *
 * 刻意**不**包含 `baseUrlDefinitionId` / `baseUrlOverride`：它们是连接配置，
 * 点中选择器那一行的瞬间就提交了——始终是那种"没有 Save 按钮"的行为。
 */
export interface EndpointDraft {
  url?: string;
  method?: HttpMethod;
  params?: KeyValue[];
  headers?: KeyValue[];
  body?: RequestBody;
  auth?: AuthConfig;
  docs?: string;
  tags?: string[];
}

/** 某个新建对话框的内存草稿。 */
export interface NewItemDraft {
  open: boolean;
  type: NewDraftKind | null;
  parentModuleId: string;
  parentCollectionId: string;
  name: string;
  description: string;
  color: string;
}

/** 已保存的示例——形状与 ApiResponse 相同，只是多了用于删除的稳定 `id`。 */
export type ApiResponseExample = import('@/types/domain').ApiResponse & { id: string };
