import type { AuthConfig, HttpMethod, KeyValue, RequestBody } from './request';

/**
 * 资源树的三种行：module 在项目下顶层，collection 可在 module 或 collection 下
 * 无限嵌套，endpoint 是叶子，挂 collection 下——不需要专门建一层时也可直接挂
 * module 下。
 *
 * 命名 `ResourceNodeType` 而非 `NodeType`：tsconfig 含 `"DOM"`，
 * `Node` 是全局接口。
 */
export type ResourceNodeType = 'module' | 'collection' | 'endpoint';

/**
 * 资源树的一行：只含结构与身份，是 `simple_post_core::domain::ResourceNode`
 * 的 TS 镜像。三种行合并为这一个可辨识联合，endpoint 独有的字段在
 * `EndpointDetail` 上。
 */
export interface ResourceNode {
  id: string;
  /**
   * 归属项目，冗余写在每个节点上：一次索引读即可回答"属于哪个项目"，
   * 既是 store 的项目过滤条件，也是后端按项目鉴权的 key。
   */
  rootId: string;
  /**
   * 直接父节点；module 的父级是项目，故为 `null`。其他组合
   * `db::nodes::move_many` 会拒绝。
   */
  parentId: string | null;
  /** 命名 `kind` 因为 `type` 是 Rust 关键字；不用 serde `rename` 绕，理由同 `AuthConfig.kind`。 */
  kind: ResourceNodeType;
  name: string;
  description?: string;
  /** 同级中的序号，作用域是父节点而非项目。 */
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  /** 仅渲染器的展开状态，不落库。 */
  expanded?: boolean;
}

/**
 * endpoint 的请求配置，按 node id 单独一行。
 *
 * 从 `ResourceNode` 拆出：method / url / tags 对 module 无意义。树只渲染
 * name / kind / sortOrder，读树不碰这张表。
 */
export interface EndpointDetail {
  nodeId: string;
  method: HttpMethod;
  /**
   * 请求 URL 的 path 部分，**不含**任何 baseUrl；完整 URL 在发送时由
   * `resolveBaseUrl`（见 lib/url）算出。存 path 是为了让同一个 endpoint
   * 换环境时不必重写 URL。
   */
  url: string;
  /**
   * 继承 baseUrl 与自由变量 `variables` 的环境。`null`（或缺省）表示
   * 未选环境——此时变量池为空，也解析不出 baseUrl。
   */
  environmentId?: string | null;
  /**
   * 本 endpoint 用的服务，id 指向**本项目**的 `baseUrlDefinitions` 行；
   * 指向别的项目即为悬挂引用，不会退化成拿别的主机。
   *
   * `null`（或缺省）表示跟随项目默认服务 `Project.defaultBaseUrlDefinitionId`，
   * URL 取 `env.moduleBaseUrls[defId][moduleId]`。
   */
  baseUrlDefinitionId?: string | null;
  /**
   * 本 endpoint 的 baseUrl 覆盖，优先于模块的服务解析；用于不挂在任何
   * 环境里的一次性主机。三态：`null` = 不在自定义模式。
   */
  baseUrlOverride?: string | null;
  params: KeyValue[];
  /**
   * URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
   *
   * 与 `params` 分开存，因为两者进的地方不同：`params` 一律进查询串，
   * 路径参数要被替换进路径本身。混在一份列表里，改一个占位符的名字会静默
   * 改变一个参数的归属。
   */
  pathParams?: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs?: string;
  /** Markdown 笔记。与 `docs` 分开存放：整页文档保持不动，描述就地填写。 */
  notes?: string;
  tags: string[];
  /**
   * 请求发出之前跑的那段脚本。`null`（或缺省）= 没有脚本。
   *
   * 与 body / headers 同级：它们都是这个端点自己的请求配置。collection 的脚本在
   * `NodeScripts` 里。
   */
  preRequestScript?: string | null;
  /** 响应回来之后跑的那段脚本。`null`（或缺省）= 没有脚本。 */
  testScript?: string | null;
}

/** endpoint 节点 join 上它的 `EndpointDetail`——HTTP 测试器与响应查看器工作的形状，需要 method / url 连同节点身份。 */
export interface EndpointResource extends ResourceNode {
  detail: EndpointDetail;
}

/**
 * 项目级的一个具名服务（"鉴权服务"、"数据服务"）。
 *
 * 列表归项目，所有模块都看得见全部服务；按模块各存一份会让两个用同一主机的
 * 模块重复维护。留在模块上的是**URL**，见 `Environment.moduleBaseUrls`：
 * 有哪些服务是项目问题，而某环境下"数据服务"指向哪里是
 * (环境 × 模块 × 服务) 问题。
 */
export interface BaseUrlDefinition {
  id: string;
  name: string;
  description?: string;
}
