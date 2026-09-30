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
 * a collection.
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
   * (top level) or another collection (nested); an endpoint's is always
   * a collection. `db::nodes::move_many` refuses any other combination.
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
   * `null` (or absent) means "no env selected" — pickers fall back to
   * the project's default baseUrl definition and the variable pool is
   * empty.
   */
  environmentId?: string | null;
  /**
   * Which project-level baseUrl definition this endpoint uses. `null`
   * (or absent) means "use the project's default definition" (see
   * `Project.defaultBaseUrlDefinitionId`). The actual URL is then
   * looked up from `env.baseUrls[defId]`. Decoupling the env from
   * the baseUrl means a single endpoint can keep its
   * "鉴权服务 / 数据服务" semantics while the operator flips between
   * dev / staging / prod envs.
   */
  baseUrlDefinitionId?: string | null;
  /**
   * Per-endpoint baseUrl override — wins over both the env's
   * `baseUrls[defId]` and the project default. Use for one-off hosts
   * (e.g. a staging mirror that lives in no env).
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
 * A named baseUrl slot owned by a project. The project declares the
 * *set of names* (e.g. "鉴权服务", "数据服务"); each env then provides
 * a concrete URL for each name. This is the "baseUrl 脱离变量单独
 * 配置" model — baseUrls are first-class project resources, not
 * just `{{var}}` placeholders in the env's free-form variables.
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
   * The named baseUrl slots this project exposes. Each env maps each
   * id to a concrete URL. Empty when the project hasn't declared any
   * (a freshly imported project is allowed to start here).
   */
  baseUrlDefinitions: BaseUrlDefinition[];
  /**
   * Which definition the picker should show when the user picks
   * "项目默认". Endpoints with `baseUrlDefinitionId === null` also
   * resolve through this id. `null` means "no default" — the picker
   * then has nothing to offer as default.
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
   * Per-environment URLs for each project-level baseUrl definition,
   * keyed by `BaseUrlDefinition.id`. The picker in HttpTester reads
   * `env.baseUrls[endpoint.baseUrlDefinitionId ?? project.default…]`
   * to compose the full URL.
   *
   * This replaces the older `Environment.baseUrl: string`. The store
   * migrates the old single-URL field into this map at boot (under
   * the project's default definition), so existing data carries
   * forward.
   */
  baseUrls: Record<string, string>;
  /**
   * Free-form template variables (e.g. `token`, `apiKey`). baseUrl
   * values live in `baseUrls`, not here.
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
