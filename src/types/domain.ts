// 领域类型，与 simple_post_core::domain 一一对应

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS' | 'TRACE';

export const HTTP_METHODS: HttpMethod[] = [
  'GET',
  'POST',
  'PUT',
  'PATCH',
  'DELETE',
  'HEAD',
  'OPTIONS',
  'TRACE',
];

export interface KeyValue {
  id: string;
  key: string;
  value: string;
  enabled: boolean;
  description?: string;
}

export type BodyMode = 'none' | 'json' | 'form' | 'multipart' | 'raw';

/**
 * `multipart` 模式下的一个分片。
 *
 * 分片数组存在 `RequestBody.content` 里（序列化后的 JSON），而不是另开一个字段——
 * 那会改动 `body_json` 的形状，而这里复用既有的 `content` 就够了。
 */
export interface MultipartPart {
  name: string;
  value: string;
  /** 有值即文件分片：`Content-Disposition` 带上 `filename=`，并默认 `application/octet-stream`。 */
  filename?: string;
  /** 分片自己的 Content-Type；文件分片缺省为 `application/octet-stream`。 */
  contentType?: string;
}

export interface RequestBody {
  mode: BodyMode;
  // json / form / raw 模式下的原始内容；multipart 模式下是一个 MultipartPart[] 的 JSON 文本
  content: string;
  // json 模式可选：序列化后的 JSON Schema
  schema?: string;
}

export type AuthType = 'none' | 'bearer' | 'basic' | 'apikey' | 'oauth2';

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
  /** `kind === 'oauth2'` 时才有；缺省即「配到一半」，取 token 时才报错。 */
  oauth2?: OAuth2Auth;
}

/**
 * OAuth2 流程。线上 snake_case，与 RFC 6749 的 `grant_type` 同名，用户与 AI
 * 客户端都不必再做一次映射。
 */
export type OAuth2GrantType = 'authorization_code' | 'client_credentials' | 'password' | 'implicit';

/** 客户端凭据怎么送到 token 端点：`Basic` 头（多数 IdP）或 form body。 */
export type OAuth2ClientAuth = 'basic' | 'body';

/** access token 挂哪；与 `apikey.inLocation` 同构。 */
export type OAuth2TokenLocation = 'header' | 'query';

/**
 * `OAuth2Auth` 的 TS 镜像，字段名与 `crates/simple-post-core/src/domain.rs`
 * 的同名结构体逐字对应。
 *
 * 每个字段都在 Rust 侧带 `#[serde(default)]`，所以这里也全部必填——
 * 缺字段推迟到取 token 时由后端报错，那时报错比 serde 的字段列表有用。
 */
export interface OAuth2Auth {
  grantType: OAuth2GrantType;
  /** token 端点；`implicit` 不用。 */
  tokenUrl: string;
  clientId: string;
  clientSecret: string;
  clientAuth: OAuth2ClientAuth;
  /** 空格分隔；空串即「用服务端默认值」。 */
  scope: string;
  /** 登录地址，只存着供用户复制——本应用不开回环端口，不主动打开浏览器。 */
  authorizationUrl: string;
  /** `authorization_code` 用，须与 IdP 侧登记的一致。 */
  redirectUri: string;
  /** PKCE verifier；本应用不自动生成，须与 IdP 侧的 challenge 对应。 */
  codeVerifier: string;
  /**
   * 用户从浏览器粘回来的原文：`authorization_code` 下是回调 URL 或其中的 code，
   * `implicit` 下是整条回调 URL 或裸 token（解析都在 Rust 侧 `parse_pasted`）。
   */
  authCode: string;
  username: string;
  password: string;
  /** 追加进 token 请求的表单字段（Azure 的 `audience`、Okta 的 `organization`…）。 */
  extraParams: KeyValue[];
  tokenLocation: OAuth2TokenLocation;
  /** query 档的参数名；空白时后端回落 `access_token`。 */
  tokenParam: string;
}

/**
 * 取 token 的返回形状，镜像 Rust 的 `OAuth2TokenDto`。
 *
 * 刻意不含 `refresh_token`：它在后端续期，浏览器形态下没必要持有。
 */
export interface OAuth2TokenResult {
  accessToken: string;
  tokenType: string;
  /** Unix 秒；`null` = 不知道何时过期。 */
  expiresAt: number | null;
  scope: string | null;
  /** 本次是否真发了网络请求；`false` = 命中后端缓存。 */
  refreshed: boolean;
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
  /**
   * URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。
   *
   * 与 `params` 分开存，因为两者进的地方不同：`params` 一律进查询串，
   * 路径参数要被替换进路径本身。混在一份列表里，「这个参数进哪儿」就取决于
   * URL 文本，改一个占位符的名字会静默改变一个参数的归属。
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
   * {@link NodeScripts} 里。
   */
  preRequestScript?: string | null;
  /**
   * 响应回来之后跑的那段脚本。`null`（或缺省）= 没有脚本。
   */
  testScript?: string | null;
}

/**
 * 一个节点上的两段脚本。collection 的脚本住这里；端点的脚本在
 * {@link EndpointDetail} 上（与 body / headers 同级）。
 */
export interface NodeScripts {
  nodeId: string;
  /** 请求发出之前跑的那段。`null`（或缺省）= 没有脚本。 */
  preRequestScript?: string | null;
  /** 响应回来之后跑的那段。`null`（或缺省）= 没有脚本。 */
  testScript?: string | null;
}

/** 脚本在哪个阶段跑。 */
export type ScriptPhase = 'pre' | 'test';

/** 脚本失败的原因分类。 */
export type ScriptErrorKind = 'compile' | 'runtime' | 'timeout' | 'memory' | 'stack';

/** 一条脚本日志。`level` 是 log / info / warn / error。 */
export interface ScriptLog {
  level: string;
  message: string;
}

/** 一条命名断言的结果。 */
export interface AssertionResult {
  name: string;
  passed: boolean;
  /** 失败时的消息；通过时为 null。 */
  error?: string | null;
}

/** 一个脚本的失败。`source` 是出问题的那段脚本（collection 名 / 端点名）。 */
export interface ScriptError {
  kind: ScriptErrorKind;
  message: string;
  source: string;
  phase: ScriptPhase;
}

/** 一条环境变量写入。`value` 为 null 表示删除该变量。 */
export interface EnvWrite {
  key: string;
  value: string | null;
}

/**
 * 一次发送里脚本的全部产出。
 *
 * 脚本失败**不影响请求的成败**——错误在 `errors` 里单独呈现，请求的成败照旧。
 */
export interface ScriptOutcome {
  logs: ScriptLog[];
  assertions: AssertionResult[];
  errors: ScriptError[];
  /** 脚本写下的环境变量（已由宿主落库的那些）。 */
  envWrites: EnvWrite[];
}

/**
 * 脚本看得到的环境快照。
 *
 * `id` 决定 `pm.environment.set` 往哪写；`null` 表示没有环境，那种写入会被丢弃。
 */
export interface EnvSnapshot {
  id: string | null;
  variables: Record<string, string>;
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

/**
 * 值的序列化形态。与 Rust 的 `domain::ValueMode` 一一对应。
 *
 * `auto` 与 `text` 的输出完全一致：对象与数组给裸 JSON 片段，标量给纯文本。
 * `text` 是用户的声明（值会被放进请求体的字符串位置，转义由用户承担），不是另一种形态。
 */
export type ValueMode = 'auto' | 'json' | 'text';

/**
 * 一条动态变量的定义：变量名 → 数据源 → 抽取规则 → 缓存时长。
 *
 * 数据源二选一：`sourceEndpointId` 引用树里的一个端点，`sourceInlineJson` 是一份内联请求。
 * 对应 `crates/simple-post-core/src/domain/dynamic.rs` 的 `DynamicVariable`。
 */
export interface DynamicVariable {
  id: string;
  projectId: string;
  name: string;
  description?: string | null;
  /** 数据源端点（`nodes.id`）。端点被删后这一项为 null，定义本身仍在。 */
  sourceEndpointId?: string | null;
  /** 数据源端点的名字。界面直接显示它，不必自己再查一次树。 */
  sourceEndpointName?: string | null;
  /** 内联请求（method / url / headers / body / auth 的 JSON 文本）。 */
  sourceInlineJson?: string | null;
  /** JSONPath 表达式。`$` 表示整段响应。 */
  extractPath: string;
  valueMode: ValueMode;
  /** 缓存有效期（秒）。 */
  ttlSeconds: number;
  /** 取数失败时是否允许用已过期的缓存值兜底。 */
  staleFallback: boolean;
  /** 是否参与解析。停用的定义不参与替换。 */
  enabled: boolean;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
}

/** 当前值是从哪来的。 */
export type DynamicValueSource = 'auto' | 'override' | 'stale' | 'missing';

/** 取数或解析失败的原因。 */
export type DynamicErrorKind =
  | 'network'
  | 'timeout'
  | 'status'
  | 'not_json'
  | 'no_match'
  | 'source_missing'
  | 'source_error'
  | 'too_large';

/** 一条动态变量的错误。`name` 是出错的变量名。 */
export interface DynamicError {
  name: string;
  kind: DynamicErrorKind;
  message: string;
}

/** 一条动态变量的当前状态。 */
export interface DynamicValueStatus {
  name: string;
  /** 当前生效的值（已按 `valueMode` 序列化）。 */
  value?: string | null;
  source: DynamicValueSource;
  fetchedAt?: number | null;
  expiresAt?: number | null;
  isOverride: boolean;
  error?: DynamicError | null;
}

/** 一条定义 + 它的当前值与缓存状态。 */
export interface DynamicVariableView extends DynamicVariable {
  value?: string | null;
  source: DynamicValueSource;
  fetchedAt?: number | null;
  expiresAt?: number | null;
  isOverride: boolean;
  error?: DynamicError | null;
}

/** 一次解析的结果：值、状态与错误。 */
export interface DynamicResolution {
  /** 变量名 → 生效的值。缺席的名字按缺失处理。 */
  values: Record<string, string>;
  statuses: DynamicValueStatus[];
  errors: DynamicError[];
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
  /**
   * 被停用的变量名。**照样存、照样显示，但不参与替换。**
   *
   * 与 `variables` 平行，而不是把值改成 `{value, enabled}`：后者会改掉
   * `environments.variables_json` 列的既有格式，旧行读不回来，就成了要数据迁移的地带。
   * 平行集合是纯增量：旧行读回来是空集合，语义等价于「全部启用」。
   */
  disabledVariables: string[];
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
