/**
 * 跨多个切片模块共用的类型与常量声明。
 *
 * 单独成文件是因为好几个切片必须对同一形状达成一致——例如 UiSlice 的 `openTabs`
 * 与 DirtyTabsSlice 的 `pendingDirtyClose` 都引用 `OpenTab`，各声明一份就会漂移。
 *
 * 本文件只含类型加几个小的运行时常量。切片工厂只 import，从不调用它；运行时
 * 状态留在切片模块自己那里。
 */
import type { AuthConfig, HttpMethod, KeyValue, RequestBody, TlsSettings } from '@/types/domain';

/**
 * 视觉主题预设。驱动 document 根节点上的 CSS 变量。
 *
 * 定义在 `src/lib/theme-boot/index.ts` —— 那里同时被 `index.html` 的首帧脚本与 store 用，
 * 两边对「什么算合法主题」必须给出同一个答案。类型在这里再导出一次，别处照旧 import。
 */
export type { Theme } from '@/lib/theme-boot';

/**
 * HTTPS 出站设置。定义在 `@/types/domain`（那里是 `simple_post_core::domain` 的镜像），
 * 这里再导出一次，好让只关心 store 形状的调用方不必跨目录 import。
 */
export type { TlsSettings } from '@/types/domain';

/**
 * HTTP 执行器的可调项。
 *
 * 存在后端 `preferences` 表的 `request_settings` 行里（不是 localStorage）：MCP 侧
 * 发请求读的是同一行，故界面上改的超时对 AI 客户端同样生效。
 */
export interface RequestSettings {
  timeoutMs: number;
  followRedirects: boolean;
  maxResponseSizeKb: number;
  /** HTTPS 出站的**全局默认**。端点可以整体取代它，见 `EndpointDetail.tls`。 */
  tls: TlsSettings;
}

/**
 * 仍然会弹出**新建对话框**的种类。
 *
 * endpoint 被刻意排除：新建它不需要问任何问题（你点的节点就是父节点），所以它
 * 直接写进前端草稿并落到重命名，见 `slices/ui-new-item.ts::createEndpointAt`。
 *
 * `ProjectTree` 为自己的 hook 绑定镜像了同一形状，两者有意保持一致，树菜单项才能
 * 按 store 的动作签名来定类型。
 */
export type NewDraftKind = 'project' | 'module' | 'collection';

/**
 * 右栏工作区能显示的标签种类。
 *
 * `endpoint-draft` 是唯一 id **不是** `nodes` 行的种类：它是渲染侧铸造的
 * `DraftEndpoint` id，只有用户按 Save 时才落库。它是独立种类而不是「少一行的
 * endpoint」，因为每一处「这个实体还在吗？」的清扫（`pruneStaleTabs`、级联删除、
 * 脏关闭队列）都得能区分「已删除」与「还没写入」，混同两者会让新建的草稿标签在
 * 树一刷新的瞬间就被关掉。
 */
export type TabKind =
  | 'project'
  | 'module'
  | 'collection'
  | 'endpoint'
  | 'endpoint-draft'
  | 'settings'
  | 'dynamic-variables';

/**
 * 只存在于内存的东西所用的 id 前缀。真实节点 id 是裸 `uid()`，所以 `draft:`
 * 不可能与它重名——草稿标签、草稿 endpoint 与真实 endpoint 因此能共用同一个
 * `openTabs` 数组，而不必靠「猜」来判断持有的是哪一种。
 */
export const DRAFT_ID_PREFIX = 'draft:';

/**
 * 动态变量标签的 id 前缀。
 *
 * 动态变量挂在**项目**里，故这个标签是**项目内**的标签页：每个项目有自己的一条，
 * 切项目时它跟着那条项目的标签条一起暂存与取回。标签 id 里带上项目 id，
 * 于是「这条标签属于哪个项目」不用另找一张表去问。
 */
export const DYNAMIC_TAB_ID_PREFIX = '__dynamic_variables__:';

/**
 * 某个项目的动态变量标签 id。
 *
 * 前缀保证它与真实节点 id（裸 `uid()`）不会重名——标签 id 直接当 `openTabs` 的键用。
 */
export function dynamicTabId(projectId: string): string {
  return `${DYNAMIC_TAB_ID_PREFIX}${projectId}`;
}

/**
 * 从动态变量标签 id 里取回项目 id。id 不是这个形状时返回空串。
 */
export function dynamicTabProjectId(tabId: string): string {
  return tabId.startsWith(DYNAMIC_TAB_ID_PREFIX) ? tabId.slice(DYNAMIC_TAB_ID_PREFIX.length) : '';
}

/**
 * 只存在于渲染侧的 endpoint。
 *
 * 语义是**整份文档**，不同于 `EndpointDraft`（后者是对已存 `EndpointDetail` 的
 * 补丁）。草稿没有可供补丁依附的存储行，所以要写的每个字段都在这里，从
 * `blankDetail` 本该提供的默认值起步。
 *
 * params / headers / body **不是**这里的字段：它们由请求配置编辑器送来，而那些
 * 编辑器对草稿和已存 endpoint 都一样写进 `endpointDrafts`。`saveDraftEndpoint`
 * 合并两半，见 `slices/ui-draft-endpoints.ts`。
 */
export interface DraftEndpoint {
  /** 本地铸造的唯一 id，同时用作标签 id。总以 `DRAFT_ID_PREFIX` 开头。 */
  id: string;
  /** endpoint 将被写入的父节点（一个 module 或一个 collection）。 */
  parentId: string;
  /** 所属项目。UI 用它界定草稿范围。 */
  rootId: string;
  /** 接口名——重命名前是 `UNTITLED_ENDPOINT_NAME`，之后就地更新。 */
  name: string;
  /**
   * 一行摘要，用法同 `ResourceNode.description`。
   *
   * 它是节点字段而不是 detail 字段，所以走 `addNode` 自己的 `description` 写入，
   * **不**走被合并的 `detail`——它之所以存在而不是干脆没有，是因为工作区的描述
   * 编辑器对草稿和对已存 endpoint 一样启用，而一个默默丢弃输入的编辑器比一个
   * 根本不存在的编辑器更糟。
   */
  description: string;
  method: HttpMethod;
  /** 不含任何 baseUrl 的 URL 路径，含义同 `EndpointDetail.url`。 */
  url: string;
  auth: AuthConfig;
  environmentId: string | null;
  baseUrlOverride: string | null;
  tags: string[];
  docs: string;
  createdAt: number;
}

/**
 * draft endpoint 的初始名字。
 *
 * 从 `ui-new-item.ts` 再导出而不是重新声明：两个值相同的常量会让对话框路径和
 * 草稿路径悄无声息地分叉。子工厂从这里 import——它们不得互相 import。
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
 * 分的。属于项目 A 的 endpoint 标签在打开 B 时毫无意义，而 `pruneStaleTabs` 最终
 * 会把它当成「不在树里了」退役。所以切项目把出去的那份暂存到这里、把进来的那份
 * 取回来。
 *
 * **只保留标签表**，这正是重点。未保存标签持有的一切——`draftEndpoints[draftId]`、
 * `endpointDrafts[id]`、它在 `dirtyTabIds` 里的条目——都按标签 id 作键，而切换
 * 刻意不碰它们。因此「带着三个脏标签和一个命名好的草稿离开项目 A，回来发现原样
 * 不变」不需要为其中任何一份数据做第二份拷贝。
 */
export interface ProjectTabSet {
  tabs: OpenTab[];
  /** 其中哪个在前面；用户最后把它关掉时为 `null`。 */
  activeTabId: string | null;
}

/**
 * 每个 endpoint 的未保存编辑。存在 `endpointDrafts`（而不是 HttpTester 的局部
 * state）里，切走标签再切回来才留得住用户正在输入的内容。每个字段各自可选——
 * 只有用户真正编辑过的字段才存在，缺失的字段含义是「与已存的 `EndpointDetail`
 * 相同」。
 *
 * 所有请求配置编辑器（params / headers / body / auth / docs / tags）也都写进这里，
 * 所以整个标签遵循同一条规则：**编辑存在草稿里，Save 才落盘，在此之前标签一直是
 * 脏的。**
 *
 * 刻意**不**包含 `baseUrlOverride`：它是连接配置，点中选择器那一行的瞬间就提交
 * 了——始终是那种「没有 Save 按钮」的行为。
 */
export interface EndpointDraft {
  url?: string;
  method?: HttpMethod;
  params?: KeyValue[];
  /**
   * URL 路径里的占位符与它们各自的值。它们替换进路径本身，不进查询串。
   */
  pathParams?: KeyValue[];
  headers?: KeyValue[];
  body?: RequestBody;
  auth?: AuthConfig;
  docs?: string;
  tags?: string[];
  /** 请求发出之前跑的那段脚本。 */
  preRequestScript?: string;
  /** 响应回来之后跑的那段脚本。 */
  testScript?: string;
  /**
   * 该端点自己的 HTTPS 出站设置。`null` = 回到继承全局默认
   * （`RequestSettings.tls`）。
   *
   * 三态与其它字段不同：`undefined` = 用户没碰过这一项（保存时不动已存的那一行），
   * `null` = 明确回到继承全局（保存时写 `null`），一份设置 = 端点自带的那份。
   */
  tls?: TlsSettings | null;
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
