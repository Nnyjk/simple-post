// 领域类型，与 simple_post_core::domain 一一对应

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS';

export const HTTP_METHODS: HttpMethod[] = [
  'GET',
  'POST',
  'PUT',
  'PATCH',
  'DELETE',
  'HEAD',
  'OPTIONS',
];

export interface KeyValue {
  id: string;
  key: string;
  value: string;
  enabled: boolean;
  description?: string;
}

export type BodyMode = 'none' | 'json' | 'form' | 'raw';

export interface RequestBody {
  mode: BodyMode;
  // json / form / raw 模式下的原始内容
  content: string;
  // json 模式可选：序列化后的 JSON Schema
  schema?: string;
}

export type AuthType = 'none' | 'bearer' | 'basic' | 'apikey';

export interface AuthConfig {
  /**
   * 命名 `kind` 而非 `type`（Rust 关键字）；用 serde `rename` 绕过去会让
   * 它与 `ResourceNode.kind` 的 Rust 对应字段对不上。
   */
  kind: AuthType;
  bearer?: string;
  basic?: { username: string; password: string };
  /**
   * `in` 是 TS 保留字（`obj.in` 解析不过），位置直接拼写出来；
   * 改成尾下划线也不行——Rust 字段 camelCase 回来仍是 `in`。
   */
  apikey?: { key: string; value: string; inLocation: 'header' | 'query' };
}

/**
 * 资源树的三种行：module 在项目下顶层，collection 可在 module 或
 * collection 下无限嵌套，endpoint 是叶子，挂 collection 下——
 * 不需要专门建一层时也可直接挂 module 下。
 *
 * 命名 `ResourceNodeType` 而非 `NodeType`：tsconfig 含 `"DOM"`，
 * `Node` 是全局接口，会在每个碰树的文件里盖掉领域含义。
 */
export type ResourceNodeType = 'module' | 'collection' | 'endpoint';

/**
 * 资源树的一行：只含结构与身份。
 *
 * 是 `simple_post_core::domain::ResourceNode` 的 TS 镜像。
 * `Module` / `Collection` / `Endpoint` 三个接口、三个表合并为这一个
 * 可辨识联合，endpoint 独有字段挪到 {@link EndpointDetail}。
 */
export interface ResourceNode {
  id: string;
  /**
   * 归属项目，冗余写在每个节点上：一次索引读即可回答"属于哪个项目"，
   * 既是 store 的项目过滤条件，也是后端按项目鉴权的 key。
   */
  rootId: string;
  /**
   * 直接父节点；module 的父级是项目，故为 `null`。collection 的父级是
   * module（顶层）或 collection（嵌套），endpoint 的是 collection 或
   * module；其他组合 `db::nodes::move_many` 会拒绝。
   */
  parentId: string | null;
  /** 命名 `kind` 因为 `type` 是 Rust 关键字；不用 serde `rename` 绕，理由同 {@link AuthConfig.kind}。 */
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
 * 从 {@link ResourceNode} 拆出：method / url / tags 对 module 无意义，
 * 混在一起会让每个 module 行多出永久为空的列。树只渲染
 * name / kind / sortOrder，所以读树不碰这张表。
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
   * URL 取 `env.moduleBaseUrls[defId][moduleId]`，按 endpoint 自己的模块查。
   */
  baseUrlDefinitionId?: string | null;
  /**
   * 本 endpoint 的 baseUrl 覆盖，优先于模块的服务解析；用于不挂在任何
   * 环境里的一次性主机。三态：`null` = 不在自定义模式。
   */
  baseUrlOverride?: string | null;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs?: string;
  /** Markdown 笔记。与 `docs` 分开存放：整页文档保持不动，描述就地填写。 */
  notes?: string;
  tags: string[];
}

/** endpoint 节点 join 上它的 {@link EndpointDetail}——HTTP 测试器与响应查看器工作的形状，需要 method / url 连同节点身份。 */
export interface EndpointResource extends ResourceNode {
  detail: EndpointDetail;
}

/**
 * 项目级的一个具名服务（"鉴权服务"、"数据服务"）。
 *
 * 列表归项目，所有模块都看得见全部服务；按模块各存一份会让两个用同一主机的
 * 模块重复维护。留在模块上的是**URL**，见 {@link Environment.moduleBaseUrls}：
 * 有哪些服务是项目问题，而某环境下"数据服务"指向哪里是
 * (环境 × 模块 × 服务) 问题。这个不对称就是整个模型。
 *
 * 对应 `crates/simple-post-core/src/domain.rs` 的 `BaseUrlDefinition`。
 */
export interface BaseUrlDefinition {
  id: string;
  name: string;
  description?: string;
}

export interface Project {
  id: string;
  name: string;
  description?: string;
  /** @deprecated 改用 `baseUrlDefinitions` + `Environment.moduleBaseUrls`；仅为兼容保留，新代码不要引用。 */
  baseUrl?: string;
  /**
   * 本项目对外暴露的具名 baseUrl 槽位。**一份列表，全模块共享**——
   * 设置页表格是这些服务与本项目模块的笛卡尔积，HttpTester 选择器也列出
   * 全部。新建项目此处为空（v2 契约 §9 起 `create_with_defaults` 不再预置）。
   */
  baseUrlDefinitions: BaseUrlDefinition[];
  /**
   * 项目级默认服务（★）——endpoint 未指定服务时按它解析，即
   * `baseUrlDefinitionId === null`。`null` 是有意状态：有服务但用户没打星，
   * 未指定服务的 endpoint 就没有 baseUrl。打星是**项目级**的，没有模块级默认。
   */
  defaultBaseUrlDefinitionId: string | null;
  color: string;
  createdAt: number;
  updatedAt: number;
}

export interface Environment {
  id: string;
  projectId: string;
  name: string;
  /**
   * @deprecated 扁平值层，不再读写。
   *
   * 它是 (环境 × 模块 × 服务) 那一层之前的形状：键只有 `defId`，看不见模块，
   * 表达不了「同一服务在两个模块指向不同主机」。唯一值层是
   * {@link Environment.moduleBaseUrls}。
   *
   * 仍留在 schema 与线格式里：删列要重建表 = bump `SCHEMA_VERSION`
   * = `ensure_current_schema` 直接删库。迁移把历史值搬进 `moduleBaseUrls`，
   * 留在 `baseUrls` 的那份是死数据。
   */
  baseUrls: Record<string, string>;
  /**
   * 唯一的 URL 层：`serviceId → moduleId → URL`。
   *
   * 每个格子都是独立值，彼此**不继承也不回退**——缺省或空即"该组合没有值"，
   * 不会借用项目默认服务或别的模块的格子；两个模块能对同一服务名配不同主机
   * 就靠这一点。`resolveEndpointBaseUrl` 读 `moduleBaseUrls[def.id][moduleId]`。
   */
  moduleBaseUrls: Record<string, Record<string, string>>;
  /** 自由模板变量（如 `token`、`apiKey`）。baseUrl 值在 `moduleBaseUrls`，不在这里。 */
  variables: Record<string, string>;
  isActive: boolean;
}

export interface ApiResponse {
  status: number;
  statusText: string;
  durationMs: number;
  sizeBytes: number;
  headers: Record<string, string>;
  body: string;
  // JSON 响应解析结果，供美化视图用
  bodyJson?: unknown;
  contentType: string;
  error?: string;
  // ----- 请求上下文（可选，由发送方填）-----
  // HttpTester.handleSend 填，供响应历史行显示这次响应出自哪个请求
  method?: HttpMethod;
  url?: string;
}

/**
 * endpoint 发送历史的一行落库记录——`crates/simple-post-core/src/domain.rs`
 * 里 `ResponseHistoryEntry` 的渲染器镜像。
 *
 * 套一层而不是加宽 `ApiResponse`：后者是当次响应，直接交给查看器；
 * `id` / `endpointId` / `createdAt` / `bodyTruncated` 是**这行记录的**身份与出处。
 *
 * `bodyTruncated` 尤其只能放在这里：服务端写库时会裁剪超长 body 并连
 * `bodyJson` 一起丢，所以读回来的行未必等于调用方发的那次响应。
 */
export interface ResponseHistoryEntry {
  /** 行 id，服务端生成，每条唯一，当 React key 用。 */
  id: string;
  /** 来源 endpoint 的 `nodes.id`。 */
  endpointId: string;
  /** 实际落库的内容，可能是被裁剪后的形态。 */
  response: ApiResponse;
  /** 服务端裁剪了 `response.body`，`response.bodyJson` 一并丢弃。 */
  bodyTruncated: boolean;
  /** Unix 毫秒，由数据库打戳而非渲染器。 */
  createdAt: number;
}

export interface McpStatus {
  running: boolean;
  pid?: number;
  uptime?: number;
  requestsHandled: number;
  lastError?: string;
}

/**
 * Streamable-HTTP MCP 监听器的持久化配置，对应 Rust 的 `McpHttpConfig`
 * （`#[serde(rename_all = "camelCase")]`）。两处含义超出类型本身：
 *
 * - `port: 0` = 交给 OS 分配。不固化成具体数字：每次重启都变的端口不能
 *   写进 AI 客户端的配置文件（见 AGENTS.md「MCP client must not hardcode
 * a daemon port」）。
 * - `allowRemote` 是 `host` 之外的第二道闸：绑了非回环地址而不开它，
 *   监听就暴露到局域网，下面的白名单字段只在开启时才有意义。
 */
export interface McpHttpConfig {
  /** 监听地址；未开 `allowRemote` 时仅回环。 */
  host: string;
  /** `0` = 由 OS 分配临时端口。 */
  port: number;
  /** MCP 端点挂载路径，默认 `/mcp`。 */
  path: string;
  /** 是否对外提供服务，默认 `false`。 */
  allowRemote: boolean;
  /** 接受的 `Host` 头；`['*']` 关闭该检查。 */
  allowedHosts: string[];
  /** 接受的 `Origin` 头；`['*']` 关闭该检查。 */
  allowedOrigins: string[];
}
