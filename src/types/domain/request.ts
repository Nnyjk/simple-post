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

/**
 * 请求体的七种形态。
 *
 * 与 Rust 的 `simple_post_core::domain::BodyMode` 一一对应（serde 用小写变体名）。
 * 每个变体在 `RequestBody.content` 里装的东西不一样，故换模式就是换内容形状：
 *
 * | 模式 | `content` 装的是什么 | 发出去的东西 |
 * |---|---|---|
 * | `none` | 空 | 没有 body |
 * | `json` / `xml` / `raw` | 原文 | 文本 |
 * | `form` | 行数组的 JSON 文本 | urlencoded 文本 |
 * | `multipart` | 分片数组的 JSON 文本 | 字节 |
 * | `binary` | 文件描述的 JSON 文本 | 字节（文件本身） |
 */
export type BodyMode = 'none' | 'json' | 'form' | 'multipart' | 'xml' | 'raw' | 'binary';

/**
 * `form` 模式里的一行。
 *
 * 与 Rust 的 `simple_post_core::domain::FormField` 一一对应。`enabled` 缺省为真：
 * 一份没写 `enabled` 的行就是一行要发的表单字段。
 */
export interface FormField {
  key: string;
  value: string;
  enabled: boolean;
  /** 说明。界面上那一列写着什么，不上线。 */
  description?: string;
}

/**
 * `multipart` 模式下的一个分片。分片数组存在 `RequestBody.content` 里
 * （序列化后的 JSON）；另开一个字段会改动 `body_json` 的形状。
 */
export interface MultipartPart {
  name: string;
  /** 文本分片的内容。 */
  value: string;
  /** 这一分片是否参与发送。缺省为真。 */
  enabled: boolean;
  /** 有值即文件分片：`Content-Disposition` 带上 `filename=`，并默认 `application/octet-stream`。 */
  filename?: string;
  /** 分片自己的 Content-Type；文件分片缺省为 `application/octet-stream`。 */
  contentType?: string;
  /**
   * 文件分片的字节（base64）。有它时 `value` 不参与发包。
   *
   * 分片数组存在 JSON 文本里，而文件是字节，故字节只能以 base64 的形态进去。
   */
  base64?: string;
  /** 说明。界面上那一列写着什么，不上线。 */
  description?: string;
}

/**
 * `binary` 模式选中的那个文件。
 *
 * 与 Rust 的 `simple_post_core::domain::BodyFile` 一一对应。文件**不在库里存路径**——
 * 路径由运行请求的那台机器解析，桌面端与服务端容器不是一个文件系统。
 */
export interface BodyFile {
  /** 文件名。界面上显示它；它不上线（请求体只有字节与 Content-Type）。 */
  name: string;
  /** 文件有多少字节。 */
  size: number;
  /** 文件自己的 MIME 类型。缺省 `application/octet-stream`。 */
  contentType: string;
  /** 文件的字节（base64）。 */
  base64: string;
}

export interface RequestBody {
  mode: BodyMode;
  /**
   * 这一次发出的载荷。json / xml / raw 模式是原始文本；form 模式是 `FormField[]` 的
   * JSON 文本；multipart 模式是 `MultipartPart[]` 的 JSON 文本；binary 模式是
   * `BodyFile` 的 JSON 文本。
   */
  content: string;
  /**
   * 文档里那份参数定义（JSON Schema）。只用于文档与 OpenAPI 导出，**从不上线**——
   * 它与 `content` 形状接近，写接口定义时别把它当载荷发出去。
   */
  schema?: string;
}

/**
 * HTTPS 出站的一组开关与证书位置，对应 Rust 的
 * `simple_post_core::domain::TlsSettings`（字段名 camelCase）。
 *
 * **证书只存文件路径，不存内容。** 私钥一旦进库，`GET /api/preferences` 会把整行原样
 * 返回（Web 形态下那是给任何持有 bearer 口令的客户端看的），而 SQLite 文件本身也是
 * 一份用户没预期的备份。路径则随时可以被删掉或换掉，这是可接受的。
 *
 * 路径由**运行请求的那台机器**解析：桌面端是本机路径，serve-web / Docker 端是服务端
 * 容器里的路径，故 Web 形态下运维需要把证书挂进容器才能用。
 */
export interface TlsSettings {
  /**
   * 跳过服务端证书校验，主机名一并跳过。
   *
   * 关闭时服务端证书的过期、自签名、签发者一律不查，故这条请求会被任何一个中间人
   * 截获与改写。默认 `false`——只有用户明确打开才生效。
   */
  skipVerify: boolean;
  /** 自定义 CA 证书的 PEM 文件路径。`null` = 用内置的公共根证书集。 */
  caCertificatePath: string | null;
  /** 客户端证书的 PEM 文件路径（mTLS）。`null` = 不挂客户端证书。 */
  clientCertificatePath: string | null;
  /** 客户端私钥的 PEM 文件路径。`null` = 从 `clientCertificatePath` 指向的文件里一起读。 */
  clientKeyPath: string | null;
}

export type AuthType = 'none' | 'bearer' | 'basic' | 'apikey' | 'oauth2';

export interface AuthConfig {
  /**
   * 命名 `kind` 而非 `type`（Rust 关键字）：serde `rename` 绕过去会让它与
   * `ResourceNode.kind` 的 Rust 对应字段对不上。
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
 * OAuth2 流程。线上 snake_case，与 RFC 6749 的 `grant_type` 同名，
 * 用户与 AI 客户端都不必再做一次映射。
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
 * 每个字段都在 Rust 侧带 `#[serde(default)]`，所以这里也全部必填——缺字段推迟到
 * 取 token 时由后端报错，那时报错比 serde 的字段列表有用。
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
