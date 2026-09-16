// Shared domain types — will be used by both renderer and (later) Rust commands

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD' | 'OPTIONS';

export const HTTP_METHODS: HttpMethod[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];

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
  type: AuthType;
  bearer?: string;
  basic?: { username: string; password: string };
  apikey?: { key: string; value: string; in_: 'header' | 'query' };
}

export interface Endpoint {
  id: string;
  collectionId: string;
  name: string;
  description?: string;
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
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
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

export interface Collection {
  id: string;
  /**
   * The **top-most** module this collection ultimately belongs to.
   * Denormalized onto every collection (root and nested) so we can filter
   * "all collections in module M" in a single pass without walking the
   * parent chain. Moved automatically when a collection (or any of its
   * ancestors) is dragged between modules — see `moveCollection` in the
   * store.
   */
  moduleId: string;
  /**
   * The direct parent collection, or `null` for a top-level collection
   * (one that lives directly under its `moduleId`). A non-null value
   * makes this collection a **nested** collection — the tree renders
   * it under its parent.
   *
   * Constraint: the parent must belong to the same `moduleId` (the
   * store enforces this on move; rendering assumes it).
   */
  parentCollectionId: string | null;
  name: string;
  description?: string;
  sortOrder: number;
  // expanded: derived in tree, not persisted
  expanded?: boolean;
}

export interface Module {
  id: string;
  projectId: string;
  name: string;
  description?: string;
  sortOrder: number;
  expanded?: boolean;
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
