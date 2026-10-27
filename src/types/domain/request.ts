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
 * `multipart` 模式下的一个分片。分片数组存在 `RequestBody.content` 里
 * （序列化后的 JSON）；另开一个字段会改动 `body_json` 的形状。
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
  /** 这一次发出的载荷。json / form / raw 模式是原始文本，multipart 模式是 `MultipartPart[]` 的 JSON 文本。 */
  content: string;
  /**
   * 文档里那份参数定义（JSON Schema）。只用于文档与 OpenAPI 导出，**从不上线**——
   * 它与 `content` 形状接近，写接口定义时别把它当载荷发出去。
   */
  schema?: string;
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
