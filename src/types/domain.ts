// Shared domain types — will be used by both renderer and (later) Rust commands

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
  // json / form / raw content
  content: string;
  // for json mode, optional schema (stringified JSON Schema)
  schema?: string;
}

export type AuthType = 'none' | 'bearer' | 'basic' | 'apikey';

export interface AuthConfig {
  /**
   * Which scheme is in play. Named `kind` rather than `type` because
   * `type` is a keyword in Rust too, and a serde `rename` bridging
   * that gap used to make this field and `ResourceNode.kind` disagree
   * with their Rust counterparts. Same rule as `ResourceNode.kind`.
   */
  kind: AuthType;
  bearer?: string;
  basic?: { username: string; password: string };
  /**
   * `in` is a reserved word in TypeScript (`obj.in` will not parse),
   * so the location is spelled out. A trailing-underscore workaround
   * does not help: the Rust field would camelCase back to `in`.
   */
  apikey?: { key: string; value: string; inLocation: 'header' | 'query' };
}

/**
 * The three kinds of row the resource tree holds. A module is the
 * top-level branch under a project, a collection nests arbitrarily deep
 * under a module or another collection, and an endpoint is a leaf under
 * a collection — or directly under a module, when it needs no folder of
 * its own.
 *
 * Named `ResourceNodeType` rather than `NodeType` because `tsconfig`
 * includes `"DOM"`, where `Node` is a global interface that would
 * shadow the domain meaning in every file that touches the tree.
 */
export type ResourceNodeType = 'module' | 'collection' | 'endpoint';

/**
 * One row of the resource tree: structure and identity, nothing else.
 *
 * This is the TS mirror of `simple_post_core::domain::ResourceNode`.
 * `Module` / `Collection` / `Endpoint` — three separate interfaces over
 * three separate tables — collapsed into this one discriminated shape;
 * the endpoint-only fields moved to {@link EndpointDetail}.
 */
export interface ResourceNode {
  id: string;
  /**
   * Owning project, denormalised onto every node. A single indexed read
   * answers "which project does this belong to", which is both the store's
   * project filter and the backend's per-project permission gate.
   */
  rootId: string;
  /**
   * The direct parent node, or `null` for a module — whose parent is the
   * project rather than another node. A collection's parent is a module
   * (top level) or another collection (nested); an endpoint's is a
   * collection or a module. `db::nodes::move_many` refuses any other
   * combination.
   */
  parentId: string | null;
  /**
   * Named `kind` because `type` is a Rust keyword; see
   * {@link AuthConfig.kind} for why the wire name is not worked
   * around with a serde `rename`.
   */
  kind: ResourceNodeType;
  name: string;
  description?: string;
  /** Position among siblings. Scoped to the parent, not the project. */
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  /** Renderer-only expand state. Not persisted. */
  expanded?: boolean;
}

/**
 * An endpoint's request configuration, keyed by node id.
 *
 * Split from {@link ResourceNode} because `method` / `url` / `tags`
 * mean nothing for a module — folding them in would give every module
 * and collection row permanently empty columns. The tree only ever
 * renders name / type / sortOrder, so a tree read skips this entirely.
 */
export interface EndpointDetail {
  nodeId: string;
  method: HttpMethod;
  /**
   * The path part of the request URL, **without** any baseUrl. The full
   * URL is computed at send time by `resolveBaseUrl` (see lib/url).
   * Stored as a path so the same endpoint can be retargeted between
   * environments without rewriting the URL.
   */
  url: string;
  /**
   * Environment to inherit `baseUrls` and free-form `variables` from.
   * `null` (or absent) means "no env selected" — then the variable pool
   * is empty and no baseUrl resolves (a URL slot without a value is not
   * a value; see `resolveEndpointBaseUrl`).
   */
  environmentId?: string | null;
  /**
   * Which service this endpoint uses. The id names a row of **this
   * project's** `baseUrlDefinitions`; a service of another project is a
   * dangling reference and resolves to no baseUrl at all rather than to
   * another project's host.
   *
   * `null` (or absent) means "follow the project default service" —
   * `Project.defaultBaseUrlDefinitionId`. The actual URL is then
   * `env.moduleBaseUrls[defId][moduleId]`, looked up for the endpoint's
   * own module. Decoupling the env from the service is what lets one
   * endpoint keep its "鉴权服务 / 数据服务" semantics while the operator
   * flips between dev / staging / prod.
   */
  baseUrlDefinitionId?: string | null;
  /**
   * Per-endpoint baseUrl override — wins over the module's service
   * resolution. Use for one-off hosts (e.g. a staging mirror that
   * lives in no env). Three-state: `null` = not in custom mode.
   */
  baseUrlOverride?: string | null;
  params: KeyValue[];
  headers: KeyValue[];
  body: RequestBody;
  auth: AuthConfig;
  docs?: string;
  /**
   * Markdown notes for the endpoint. Kept separate from `docs` so the
   * user can keep the full-page docs untouched while filling in a quick
   * description inline.
   */
  notes?: string;
  tags: string[];
}

/**
 * An endpoint node joined with its {@link EndpointDetail} — the shape
 * the HTTP tester and the response viewer work in, where the method and
 * url are needed alongside the node's identity.
 */
export interface EndpointResource extends ResourceNode {
  detail: EndpointDetail;
}

/**
 * One named service ("鉴权服务", "数据服务") belonging to a **project**.
 *
 * The service *list* belongs to the project and every module sees all of
 * it: the user asked for exactly that ("服务应该绑定到项目，这样每一个
 * 模块都会全部包含这些服务"), because a per-module list makes two modules
 * that need the same host maintain it twice over.
 *
 * What stays per-module is the *URL*, in {@link Environment.moduleBaseUrls}:
 * which service exists is a project question, but where "数据服务" points
 * for a given environment and a given module is a
 * (环境 × 模块 × 服务) question. That asymmetry is the whole model.
 *
 * Mirrors `BaseUrlDefinition` in
 * `crates/simple-post-core/src/domain.rs`.
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
  /** @deprecated Use `baseUrlDefinitions` and `Environment.baseUrls[defId]` instead. The `baseUrl` field is kept for backward compatibility; new code should not reference it. */
  baseUrl?: string;
  /**
   * The named baseUrl slots this project exposes. **One list, shared by
   * every module** — the settings table is the cartesian product of these
   * services against the project's modules, and the HttpTester picker
   * lists all of them. Empty when the project hasn't declared any (a
   * freshly created project starts here; §9 of the v2 contract stopped
   * `create_with_defaults` from seeding one).
   */
  baseUrlDefinitions: BaseUrlDefinition[];
  /**
   * The project-wide default service (★) — what an endpoint resolves
   * through when it names no service itself, i.e.
   * `baseUrlDefinitionId === null`.
   *
   * `null` means "no default", which is a real, intentional state: the
   * project has services but the user has not starred one, so endpoints
   * that picked nothing have no baseUrl. Starring is per **project**;
   * there is no per-module default.
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
   * @deprecated 扁平值层，**不再读写**。
   *
   * 它是 (环境 × 模块 × 服务) 那一层出现之前的形状，键只有 `defId`，
   * 看不见模块，因此无法表达「同一个服务在两个模块指向不同主机」。
   * 唯一值层是 {@link Environment.moduleBaseUrls}。
   *
   * 字段仍留在 schema 与线格式里：删列要重建表 = bump `SCHEMA_VERSION`
   * = `ensure_current_schema` 直接删库。迁移会把历史值搬进
   * `moduleBaseUrls`，留在 `baseUrls` 里的那份是不再被读取的死数据。
   */
  baseUrls: Record<string, string>;
  /**
   * The one and only URL layer: `serviceId → moduleId → URL`.
   *
   * Every cell is an independent value. There is **no inheritance and no
   * fallback** between them — an absent or empty cell means "no value for
   * this combination", full stop, and deliberately does not borrow the
   * project default service's cell or any other module's. That is what
   * lets two modules use the same 服务 name against different hosts.
   *
   * `resolveEndpointBaseUrl` reads `moduleBaseUrls[def.id][moduleId]`,
   * and the settings table writes one cell per (module, service) pair.
   */
  moduleBaseUrls: Record<string, Record<string, string>>;
  /**
   * Free-form template variables (e.g. `token`, `apiKey`). baseUrl
   * values live in `moduleBaseUrls`, not here.
   */
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
  // for JSON content, parsed for pretty view
  bodyJson?: unknown;
  // for binary or other
  contentType: string;
  error?: string;
  // ----- request context (optional, set by the sender) -----
  // Populated by HttpTester.handleSend so response history rows can show
  // which request produced this response. Optional so existing producers
  // (real backend, mock senders from other tasks) stay source-compatible.
  method?: HttpMethod;
  url?: string;
}

export interface McpStatus {
  running: boolean;
  pid?: number;
  uptime?: number;
  requestsHandled: number;
  lastError?: string;
}

/**
 * Persisted configuration of the Streamable-HTTP MCP listener.
 *
 * Mirrors the Rust `McpHttpConfig` (`#[serde(rename_all = "camelCase")]`).
 * Two fields carry meaning beyond their type:
 *
 * - `port: 0` means "let the OS assign one". It is deliberately not
 *   persisted as a concrete number, because a daemon port that changes on
 *   every restart must not be written into an AI client's config file
 *   (see AGENTS.md — "an MCP client must not hardcode a daemon port").
 * - `allowRemote` is the second gate on top of `host`: binding a
 *   non-loopback address without it leaves the listener reachable from
 *   the LAN, so the whitelist fields below only matter while it is on.
 */
export interface McpHttpConfig {
  /** Listen address. Loopback-only unless `allowRemote` is also set. */
  host: string;
  /** `0` = OS-assigned ephemeral port. */
  port: number;
  /** Route the MCP endpoint is mounted at. Defaults to `/mcp`. */
  path: string;
  /** Opt-in for serving beyond this machine. Defaults to `false`. */
  allowRemote: boolean;
  /** Accepted `Host` header values. `['*']` disables the check. */
  allowedHosts: string[];
  /** Accepted `Origin` header values. `['*']` disables the check. */
  allowedOrigins: string[];
}
