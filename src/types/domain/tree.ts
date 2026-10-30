import type { AuthConfig, HttpMethod, KeyValue, RequestBody, TlsSettings } from './request';

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
   * 本 endpoint 的 baseUrl 覆盖，优先于模块那一格；用于不挂在任何环境里的一次性
   * 主机。三态：`null` = 不在自定义模式，`''` = 自定义模式但不加前缀。
   *
   * 覆盖生效时模块地址与代理都不参与，故这一项是这一整套地址解析的唯一出口。
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
  /**
   * 该端点自己的 HTTPS 出站设置。`null`（或缺省）= 继承全局默认
   * （`RequestSettings.tls`）。
   *
   * 端点给了设置就**整体**取代全局那份，不是逐字段合并，故这一个字段足以表达
   * 「这个接口用另一套证书」。
   */
  tls?: TlsSettings | null;
}

/** endpoint 节点 join 上它的 `EndpointDetail`——HTTP 测试器与响应查看器工作的形状，需要 method / url 连同节点身份。 */
export interface EndpointResource extends ResourceNode {
  detail: EndpointDetail;
}
