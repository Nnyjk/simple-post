/**
 * 渲染层调用的每个 Tauri 命令名的唯一来源。`src/` 里每个 `invoke(...)` 都必须
 * 走 `COMMANDS` 表，绝不用裸字符串 —— 这样两侧（Rust 侧
 * `src-tauri/src/lib.rs` 的 `invoke_handler!` 注册，或渲染层）改名会被编译器
 * 或一次全局替换抓到，而不是运行时静默失效。
 *
 * 命名约定：
 *   - 字符串值必须等于 `src-tauri/src/commands/` 里的函数名（`http_request`
 *     例外，它是 `src-tauri/src/lib.rs` 里的 `#[tauri::command]`）。
 *   - 对象上的键为 camelCase 以便 TS 侧使用：`mcp_status` ⇒ `mcpStatus`。
 *   - 每个分区内按字母序排列，扫一眼表就能发现缺失的命令。
 */

export const COMMANDS = {
  // ----- projects -----
  projectsList: 'projects_list',
  projectsGet: 'projects_get',
  projectsCreate: 'projects_create',
  projectsUpdate: 'projects_update',
  projectsDelete: 'projects_delete',

  // ----- resource tree -----
  // 节点是三种类型之一，由 `type` 区分；树形靠 `parentId`，归属靠 `rootId`。
  // 接口的请求配置另有一对命令：`method` / `url` 对模块无意义。
  nodesList: 'nodes_list',
  nodesGet: 'nodes_get',
  nodesCreate: 'nodes_create',
  nodesUpdate: 'nodes_update',
  nodesDelete: 'nodes_delete',
  nodesMove: 'nodes_move',
  nodesDuplicate: 'nodes_duplicate',
  endpointDetailsList: 'endpoint_details_list',
  endpointDetailGet: 'endpoint_detail_get',
  endpointDetailUpdate: 'endpoint_detail_update',

  // ----- environments -----
  environmentsList: 'environments_list',
  environmentsCreate: 'environments_create',
  environmentsUpdate: 'environments_update',
  environmentsDelete: 'environments_delete',
  environmentsSetVar: 'environments_set_var',
  environmentsRenameVar: 'environments_rename_var',
  environmentsSetModuleBaseurl: 'environments_set_module_baseurl',
  environmentsSetActive: 'environments_set_active',

  // ----- baseUrl definitions -----
  baseUrlDefsList: 'base_url_defs_list',
  baseUrlDefsCreate: 'base_url_defs_create',
  baseUrlDefsUpdate: 'base_url_defs_update',
  baseUrlDefsDelete: 'base_url_defs_delete',
  baseUrlDefsSetDefault: 'base_url_defs_set_default',

  // ----- request settings -----
  requestSettingsGet: 'request_settings_get',
  requestSettingsUpdate: 'request_settings_update',

  // ----- response history -----
  // 按接口持久化的发送历史，一个资源三个动词。每个动词都点名一个接口：
  // `list` 读用户刚打开的那个接口的行（由 `setActiveTab` 触发而非切项目时 ——
  // 按项目读会去取没人正在看的接口的行），`append` 存刚发出来的结果，
  // `clear` 是显式的"忘掉这个接口的历史"入口 —— 删接口本身是外键的职责，
  // 不是这一对的。
  responseHistoryAppend: 'response_history_append',
  responseHistoryClear: 'response_history_clear',
  responseHistoryList: 'response_history_list',

  // ----- agent permissions -----
  permissionsGet: 'permissions_get',
  permissionsSet: 'permissions_set',

  // ----- MCP daemon -----
  mcpStatus: 'mcp_status',
  /**
   * 幂等启动。已有 daemon 在跑时直接返回其 `{ok, pid, port}` 而不杀进程重启 ——
   * 与 restart 同一个信封，渲染层只需处理一种形状。
   */
  mcpDaemonStart: 'mcp_daemon_start',
  /** 手动运行时开关。返回 `{ok, pid: null, port: null}`。 */
  mcpDaemonStop: 'mcp_daemon_stop',
  /**
   * restart 必须终止端口记录指向的那个 daemon，而不只是 supervisor 恰好持有的
   * `Child` 句柄：supervisor 接管一个已在运行的 daemon 时没有句柄，只按句柄杀
   * 会悄悄留下旧进程，"重启"又收养了同一个进程。
   */
  mcpDaemonRestart: 'mcp_daemon_restart',
  /**
   * 解析 stdio bridge 二进制（`simple-post-mcp.exe` / `simple-post-mcp`）的
   * 磁盘绝对路径。ConnectionSection 用它把"代理二进制路径"占位符填成真值
   * 而不是字面提示。
   */
  mcpDaemonProxyPath: 'mcp_daemon_proxy_path',
  /** 持久化的 `mcp.auto_start` 偏好。默认 `false`；Tauri 与 `serve-web` 两种形态都认这一行。 */
  mcpAutoStartGet: 'mcp_auto_start_get',
  mcpAutoStartSet: 'mcp_auto_start_set',

  // ----- MCP capability policy -----
  // 过滤 AI 客户端 `tools/list` 的三级全局能力。默认 `read_only` —— 用户在设置里
  // 选档之前该键不存在，而键不存在即解析为只读档。
  mcpCapabilityGet: 'mcp_capability_get',
  mcpCapabilitySet: 'mcp_capability_set',
  mcpCapabilityListTools: 'mcp_capability_list_tools',

  // ----- Streamable HTTP listener config -----
  // MCP 端点的 host / port / path，加上远程访问开关与 Host/Origin 白名单。读返回
  // 已持久化的配置；写返回 `{ config, requiresRestart }` —— 监听器无法就地重绑，
  // 所以由调用方给出重启提示，而不是假装改动已生效。
  mcpHttpConfigGet: 'mcp_http_config_get',
  mcpHttpConfigSet: 'mcp_http_config_set',

  // ----- OAuth 2.0 -----
  // 取一份可注入请求的 token。判断与取 token 全在 Rust 的
  // `simple_post_core::oauth2`，MCP 的 `api_call_endpoint` 与 UI 走同一段逻辑。
  // `force` 供 UI 的「获取 Token」按钮绕过后端缓存；发请求时不传，走按需刷新。
  oauth2Token: 'oauth2_token',

  // ----- low-level HTTP executor (Tauri side) -----
  httpRequest: 'http_request',
} as const;

export type TauriCommand = (typeof COMMANDS)[keyof typeof COMMANDS];

/**
 * 带类型的 `invoke` 封装。优先用它而不是直接调 `invoke`，命令名才仍可溯源到上表，
 * 返回类型在调用点也显式。
 *
 * 用法：
 *   const project = await invokeCmd<Project>(COMMANDS.projectsGet, { id });
 *
 * 可中止的调用通过 `options.signal` 传 `AbortSignal`。Tauri 形态直接丢弃它
 * （Tauri 2 IPC 尚无原生 signal）—— JS 侧 promise 仍会在 abort 时 reject，
 * 迟到的 Rust 响应由调用方的 `inFlightRef` 丢弃。Web 形态把 signal 转给
 * `fetch`，浏览器才真的取消网络请求。
 *
 * 两种分发形态：
 *   - Tauri shell → 存在 `__TAURI_INTERNALS__`，走 `invoke`。
 *   - Web / Dev    → 落到 `webInvokeCmd`，打 axum `serve-web` 宿主机的 `/api/*`。
 *
 * 在模块加载期判定形态是刻意的：会话内切换形态需要重新加载（页面加载时运行时即固定）。
 */
import { invoke as tauriInvoke } from '@tauri-apps/api/core';

export const IS_TAURI_SHELL: boolean =
  typeof window !== 'undefined' &&
  Boolean(
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ ||
    (window as { __TAURI__?: unknown }).__TAURI__,
  );

/**
 * `invokeCmd` 的逐调用选项。目前只有中止信号；做成对象以便将来加标志
 * （超时、重试策略等）而不破坏调用点。
 */
export interface CallApiOptions {
  /** 中止在途的 IPC 调用。Tauri 形态：promise reject 但 Rust 继续跑；Web 形态：原生 fetch abort。 */
  signal?: AbortSignal;
}

export function invokeCmd<T>(
  cmd: TauriCommand,
  args?: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  if (IS_TAURI_SHELL) {
    // Tauri 2 的 `invoke` 不接受 AbortSignal —— 在这里丢掉，调用点就不必按形态分支。
    // 需要在 Tauri 下中止的调用方应自己包装 promise（或传 `options.signal` 并监听 `abort`）。
    return tauriInvoke<T>(cmd, args ?? {});
  }
  return webInvokeCmd<T>(cmd, args ?? {}, options);
}

// -------------------------------------------------------------------------
// Web 形态的 REST 桥。
//
// 路由表对应 `crates/simple-post-server/src/web_api/` 里的 axum 路由树
// （`.route()` 注册在 `mod.rs::build_router`）。两侧不共享源码 —— 靠
// `scripts/mcp-integration.test.mjs` 的集成测试与 PR 时的手工复核保持一致。
// 新增 invoke 命令必须在**两处**都补上对应的 REST 条目。
// -------------------------------------------------------------------------

interface RouteSpec {
  method: 'GET' | 'POST' | 'PATCH' | 'DELETE' | 'PUT';
  /** 路径模板；`{key}` 段由 `args` 替换。 */
  path: (args: Record<string, unknown>) => string;
  /**
   * 构造请求体。默认就是 `args` 本身。仅在路径参数必须从请求体里剔除时覆写
   * —— 例如更新时 `args` 是 `{ id, patch }` 而线上请求体应为 `{ patch }`。
   */
  body?: (args: Record<string, unknown>) => unknown;
}

/**
 * 由 record 生成稳定、确定的 `?key=value&…` 查询串。丢弃 `undefined` / `null` /
 * 空字符串的值，键排序，两半都过 `encodeURIComponent`。
 */
function buildQuery(params: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const key of Object.keys(params).sort()) {
    const raw = params[key];
    if (raw === undefined || raw === null || raw === '') continue;
    parts.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(raw))}`);
  }
  return parts.length === 0 ? '' : `?${parts.join('&')}`;
}

/**
 * 有浏览器侧对应物的命令的 REST 映射。这里没有条目的命令在 Web 形态下会抛错
 * —— 刻意如此：显式报"未实现"比静默 404 少得多困惑。
 *
 * 约定：
 *   - GET    /api/{resource}            列表，可带查询过滤（如 `?projectId=p1`）。
 *   - GET    /api/{resource}/{id}       单条读取。
 *   - POST   /api/{resource}            创建；父 id 走请求体，使渲染层调用点
 *                                       在两种形态下完全一致。
 *   - PATCH  /api/{resource}/{id}       部分更新；请求体是 `{ patch: … }`。
 *   - DELETE /api/{resource}/{id}       删除。
 *   - POST   /api/{resource}/move       排序（放不进名词式 CRUD 的动作动词）。
 *   - POST   /api/{resource}/{id}/…     子资源动作（duplicate、activate 等）。
 */
// ----- bearer-token cache -----
//
// `serve-web` 可能带 `--password=<token>` / `SIMPLE_POST_WEB_PASSWORD` 启动。
// 渲染层必须给它发出的每个 `/api/*` 与 `/mcp` 请求附上
// `Authorization: Bearer <token>`。bearer 存在 `localStorage` 以跨刷新存活，
// SPA 引导则在首屏从 URL query string 里捡起：
//
//   https://host/sp/?token=<x>     ← 反向代理的鉴权交接
//   https://host/                   ← 之后的访问，读 localStorage
//
// 安全取舍：`localStorage` 对同源 XSS 可读。这是 Postman / Insomnia / dbx 这类
// 独立客户端（无按用户会话）同样接受的暴露面。
export const WEB_BEARER_STORAGE_KEY = 'simple-post.web.bearer';

export function writeBearerToStorage(token: string): void {
  try {
    window.localStorage.setItem(WEB_BEARER_STORAGE_KEY, token);
  } catch {
    /* 隐私模式 / 配额 —— 退回仅 URL 鉴权 */
  }
}

export function clearBearerFromStorage(): void {
  try {
    window.localStorage.removeItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    /* 忽略 */
  }
}

/** 从 localStorage 读 bearer。在 Node 侧测试（无 `window`）或用户尚未登录时返回 `null`。 */
export function readBearerFromStorage(): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(WEB_BEARER_STORAGE_KEY);
  } catch {
    return null;
  }
}

/**
 * 一次性引导：URL 带 `?token=<x>` 时持久化它（覆盖旧值 —— 用户刚轮换了凭据）
 * 并抹掉 query，免得 bearer 落进浏览器历史 / Referer 头。返回该 token（或 null）。
 */
export function ingestBearerFromUrl(search: string = window.location.search): string | null {
  if (typeof window === 'undefined') return null;
  const params = new URLSearchParams(search);
  const candidate = params.get('token');
  if (!candidate) return null;
  writeBearerToStorage(candidate);
  // 抹掉 query，免得 bearer 泄进历史 / referrer。
  const clean = window.location.pathname + window.location.hash;
  window.history.replaceState(null, '', clean);
  return candidate;
}

const WEB_ROUTES: Partial<Record<TauriCommand, RouteSpec>> = {
  // ----- projects -----
  [COMMANDS.projectsList]: { method: 'GET', path: () => '/api/projects' },
  [COMMANDS.projectsGet]: {
    method: 'GET',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.projectsCreate]: { method: 'POST', path: () => '/api/projects' },
  [COMMANDS.projectsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.projectsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.id))}`,
  },

  // ----- resource tree -----
  // 一次列表调用覆盖全树。`rootId` 是 SPA 总会有的过滤条件；`type` 与 `q` 可选，
  // 与 Tauri 命令打到同一个端点。
  [COMMANDS.nodesList]: {
    method: 'GET',
    path: (a) => `/api/nodes${buildQuery({ rootId: a.rootId, type: a.type, q: a.query })}`,
  },
  [COMMANDS.nodesGet]: {
    method: 'GET',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.nodesCreate]: {
    method: 'POST',
    path: () => '/api/nodes',
    // `NodeCreateBody` 把节点摊平，所以请求体就是节点自身字段 —— 不是
    // `{ node: {…} }` 的包装。Tauri 命令同样吃扁平 args，故这里只需补上服务端
    // 自己盖章的部分：id 与 detail 行的键。`sortOrder` 在线上必填且无默认，
    // 缺省必须变成 0 而不是 `undefined`。
    //
    // 用 `kind` 而非 `type`，因为那是两侧的领域字段名；它还必须与 Tauri 命令的
    // `kind` 参数一致，否则桌面模式会要一个 handler 从不读取的键。
    body: (a) => ({
      id: a.id,
      rootId: a.rootId,
      parentId: a.parentId ?? null,
      kind: a.kind,
      name: a.name,
      description: a.description,
      sortOrder: a.sortOrder ?? 0,
      ...(a.detail ? { detail: { ...(a.detail as object), nodeId: a.id } } : {}),
    }),
  },
  [COMMANDS.nodesUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.nodesDelete]: {
    method: 'DELETE',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.nodesMove]: {
    method: 'POST',
    path: () => '/api/nodes/move',
    // Tauri 与 Web 使用同一个扁平参数对象；只投影 REST 所需的字段。
    // `parentId: null` 有意义 —— "项目顶层"，仅模块合法 —— 所以透传而非丢弃。
    body: (a) => ({ nodeIds: a.nodeIds, parentId: a.parentId ?? null, index: a.index }),
  },
  [COMMANDS.nodesDuplicate]: {
    method: 'POST',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.id))}/duplicate`,
    // 路径已消费 `id`，故显式投影请求体，不让默认的 `body = args` 把 `{ id }` 泄进线上。
    // `newId` 是副本落库所用的 id —— SPA 乐观地铸好它，需要服务端照用；`null` 是
    // handler 已理解的"服务端自选"兜底。
    body: (a) => ({ newId: a.newId ?? null }),
  },
  [COMMANDS.endpointDetailsList]: { method: 'GET', path: () => '/api/endpoint-details' },
  [COMMANDS.endpointDetailGet]: {
    method: 'GET',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/endpoint-detail`,
  },
  [COMMANDS.endpointDetailUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/endpoint-detail`,
    body: (a) => a.patch,
  },

  // ----- environments -----
  [COMMANDS.environmentsList]: {
    method: 'GET',
    path: (a) => `/api/environments${buildQuery({ projectId: a.projectId })}`,
  },
  [COMMANDS.environmentsCreate]: {
    method: 'POST',
    path: () => '/api/environments',
  },
  [COMMANDS.environmentsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.environmentsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.environmentsSetVar]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars`,
    body: (a) => ({ key: a.key, value: a.value }),
  },
  [COMMANDS.environmentsRenameVar]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars/rename`,
    body: (a) => ({ fromKey: a.fromKey, toKey: a.toKey }),
  },
  [COMMANDS.environmentsSetModuleBaseurl]: {
    method: 'POST',
    // 取值层是 (环境 × 模块 × 服务)，故单元格的两半都走请求体。字面量路径段让它
    // 与已下线的 `/baseurls` 路由区分开 —— 那条路由没了，因为裸 `defId` 只能写
    // 扁平的、不分模块的取值层。
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/module-baseurls`,
    body: (a) => ({ defId: a.defId, moduleId: a.moduleId, url: a.url }),
  },
  [COMMANDS.environmentsSetActive]: {
    method: 'POST',
    // 使用字面量 active 路径，避免与参数路由发生匹配歧义。
    path: () => '/api/environments/active',
    body: (a) => ({ envId: a.envId }),
  },

  // ----- baseUrl definitions -----
  [COMMANDS.baseUrlDefsList]: {
    method: 'GET',
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls`,
  },
  [COMMANDS.baseUrlDefsCreate]: {
    method: 'POST',
    // 服务挂在**项目**上，所以作用域是项目且住在路径里；`/api/baseurl-defs` 是下面
    // PATCH/DELETE 以裸 id 寻址的集合。
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls`,
    // 显式投影：`name` 就是全部契约，默认的 `args` 回退会把 `projectId` 以及调用方
    // 持有的 `opts` 一起泄进请求体。`opts.id` 要转发，因为渲染层自己铸 id
    // （它在行落库前就用它索引 ★ 与环境单元格）；留空则由服务端铸。
    body: (a) => {
      const opts = a.opts as { description?: unknown; id?: unknown } | undefined;
      const id = typeof opts?.id === 'string' ? opts.id : '';
      return {
        name: a.name,
        ...(opts ? { description: opts.description } : {}),
        ...(id ? { id } : {}),
      };
    },
  },
  [COMMANDS.baseUrlDefsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/baseurl-defs/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.baseUrlDefsDelete]: {
    method: 'DELETE',
    path: (a) => `/api/baseurl-defs/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.baseUrlDefsSetDefault]: {
    method: 'POST',
    // ★ 是项目的属性，所以作用域是项目而非模块。
    path: (a) => `/api/projects/${encodeURIComponent(String(a.projectId))}/baseurls/default`,
    body: (a) => ({ defId: a.defId }),
  },

  // ----- request settings -----
  // Web 形态提供专用的 `/api/request-settings`（PUT 为全量替换语义）。
  // Tauri 命令吃的是 `patch`，所以调用点必须自己合并成全量。
  [COMMANDS.requestSettingsGet]: {
    method: 'GET',
    path: () => '/api/request-settings',
  },
  [COMMANDS.requestSettingsUpdate]: {
    method: 'PUT',
    path: () => '/api/request-settings',
    body: (a) => (a as { patch?: unknown }).patch ?? a,
  },

  // ----- response history -----
  // 按单个接口作用域，且 id 住在路径而非查询串 —— 它没有任何可选性：标签条
  // 只要它马上要画的那个接口的数据，而什么都没存的接口回答 `[]` 而非报错。
  [COMMANDS.responseHistoryList]: {
    method: 'GET',
    path: (a) => `/api/response-history/${encodeURIComponent(String(a.endpointId))}`,
  },
  [COMMANDS.responseHistoryAppend]: {
    method: 'POST',
    path: () => '/api/response-history',
    // 显式投影而非默认的 `body = args`：下面两半就是全部线上契约，调用方多带的
    // 键不能跟着上车。
    body: (a) => ({ endpointId: a.endpointId, response: a.response }),
  },
  [COMMANDS.responseHistoryClear]: {
    method: 'DELETE',
    path: (a) => `/api/response-history/${encodeURIComponent(String(a.endpointId))}`,
  },

  // ----- agent permissions -----
  [COMMANDS.permissionsGet]: {
    method: 'GET',
    path: (a) => `/api/permissions/${encodeURIComponent(String(a.projectId))}`,
  },
  [COMMANDS.permissionsSet]: {
    method: 'PUT',
    path: (a) => `/api/permissions/${encodeURIComponent(String(a.projectId))}`,
    body: (a) => ({ permission: a.permission }),
  },

  // ----- OAuth 2.0 -----
  // 与 Tauri 命令同一个 `OAuth2TokenRequest`（`{ auth, manual?, force? }`），
  // 故请求体就是 args 本身，两种形态无需翻译。
  [COMMANDS.oauth2Token]: { method: 'POST', path: () => '/api/oauth2/token' },

  // ----- low-level HTTP executor -----
  // 两侧 args 形状相同（camelCase）；Rust 的 `HttpSendBody` 直接从 `args` 反序列化，
  // 这里无需翻译。
  [COMMANDS.httpRequest]: { method: 'POST', path: () => '/api/http/send' },

  // ----- MCP daemon -----
  [COMMANDS.mcpStatus]: { method: 'GET', path: () => '/api/mcp/status' },
  // 三个生命周期动词共用一个 `{ ok, pid, port }` 信封，渲染层的判别联合处理
  // 因此只在一处。`start` 在服务端是幂等的 —— 返回既有 daemon 而非重开一个。
  [COMMANDS.mcpDaemonStart]: {
    method: 'POST',
    path: () => '/api/mcp/start',
  },
  [COMMANDS.mcpDaemonStop]: {
    method: 'POST',
    path: () => '/api/mcp/stop',
  },
  [COMMANDS.mcpDaemonRestart]: {
    method: 'POST',
    path: () => '/api/mcp/restart',
  },
  [COMMANDS.mcpDaemonProxyPath]: {
    method: 'GET',
    path: () => '/api/mcp/proxy-path',
  },
  // 同上面 `request_settings_*`：走 preferences。
  [COMMANDS.mcpAutoStartGet]: {
    method: 'GET',
    path: () => '/api/preferences',
  },
  [COMMANDS.mcpAutoStartSet]: {
    method: 'PUT',
    path: () => '/api/preferences',
    body: (a) => ({ key: 'mcp.auto_start', value: a.value }),
  },
  // ----- MCP capability policy -----
  // 专用端点 —— 直接返回策略（不像 `mcpAutoStart*` 要从整张 preferences map 里挖）。
  [COMMANDS.mcpCapabilityGet]: {
    method: 'GET',
    path: () => '/api/mcp/capability',
  },
  [COMMANDS.mcpCapabilitySet]: {
    method: 'PUT',
    path: () => '/api/mcp/capability',
    body: (a) => ({ policy: a.policy }),
  },
  [COMMANDS.mcpCapabilityListTools]: {
    method: 'GET',
    path: () => '/api/mcp/capability/tools',
  },

  // ----- Streamable HTTP listener config -----
  // 两个动词共用一条路径。设置面板在 `serve-web` 形态下同样可达，所以这一对
  // 必须存在于此处，否则该分区在浏览器模式下静默无效（双传输不变量）。
  [COMMANDS.mcpHttpConfigGet]: {
    method: 'GET',
    path: () => '/api/mcp/http-config',
  },
  [COMMANDS.mcpHttpConfigSet]: {
    method: 'PUT',
    path: () => '/api/mcp/http-config',
    body: (a) => ({ config: a.config }),
  },
};

async function webInvokeCmd<T>(
  cmd: TauriCommand,
  args: Record<string, unknown>,
  options?: CallApiOptions,
): Promise<T> {
  const spec = WEB_ROUTES[cmd];
  if (!spec) {
    throw new Error(
      `Command "${cmd}" is not exposed in the web shape. Add a REST entry in src/lib/tauri-commands.ts + src-tauri/src/web_api/ to enable it.`,
    );
  }
  // `mcpAutoStartGet` 保持 GET 后挑一行的形状，因为该端点仍返回整张 preferences
  // map 而渲染层只要 `mcp.auto_start` 那一行。`requestSettingsGet` 现在有专用端点
  // （`/api/request-settings`），无需特殊处理。
  if (cmd === COMMANDS.mcpAutoStartGet) {
    const map = await webFetch<Record<string, unknown>>('GET', spec.path(args), null, options);
    return (map['mcp.auto_start'] === true) as unknown as T;
  }

  const body = spec.body ? spec.body(args) : args;
  return webFetch<T>(spec.method, spec.path(args), body, options);
}

/**
 * 本部署所挂载的反向代理前缀，或 `''`。
 *
 * `serve-web` 把整个路由 `nest` 在 `--public-base-path`
 * （`SIMPLE_POST_PUBLIC_BASE_PATH`）下，并在模块入口运行前把该值注入 SPA shell
 * 的 `window.__SIMPLE_POST_BASE__`。必须在**运行时**注入的原因：容器里前缀是环境
 * 变量，一个镜像要同时服务 `https://host/` 与 `https://host/sp/`。
 *
 * `import.meta.env.BASE_URL` 不是替代品 —— 它是 `vite build --base` 在构建期烤
 * 进去的常量，而镜像从不传这个参数。读它会在前缀下报告 `''` 并把每个请求静默发到
 * 裸根路径，而嵌套路由从不提供裸根。
 *
 * 与 `crates/simple-post-server/src/web_api/static_files.rs` 里的 `BASE_GLOBAL`
 * 保持一致。根部署不注入任何东西，故 `''` 这个默认值正是保住根绝对行为的东西。
 */
export function readApiBase(): string {
  // 与 `readBearerFromStorage` 同样的守卫：这些模块级 helper 在单元测试里跑在
  // Node 下，那里没有 `window`。不守卫会让每个切片测试都变成 ReferenceError。
  if (typeof window === 'undefined') return '';
  const injected = (window as unknown as Record<string, unknown>).__SIMPLE_POST_BASE__;
  return typeof injected === 'string' ? injected.replace(/\/+$/, '') : '';
}

async function webFetch<T>(
  method: RouteSpec['method'],
  path: string,
  body: unknown,
  options?: CallApiOptions,
): Promise<T> {
  const headers: Record<string, string> = { 'content-type': 'application/json' };
  // 服务端以 `--password=…` / `SIMPLE_POST_WEB_PASSWORD` 启动时，渲染层的 `webFetch`
  // 必须给每次 API 调用附上 `Authorization: Bearer <token>`。bearer 缓存在
  // `localStorage`（见 `WEB_BEARER_STORAGE_KEY` 上方的说明）；新页面加载时 SPA shell
  // 先从 URL 里找 `?token=<x>` 并在任何 fetch 之前写入 localStorage。我们接受这个
  // XSS 暴露面 —— 与 Postman / Insomnia / dbx 同样的姿态，它们也都从 localStorage
  // 或扩展存储里读 token。
  const bearer = readBearerFromStorage();
  if (bearer) {
    headers['authorization'] = `Bearer ${bearer}`;
  }
  const init: RequestInit = {
    method,
    headers,
  };
  if (body !== null && method !== 'GET' && method !== 'DELETE') {
    init.body = JSON.stringify(body);
  }
  if (options?.signal) {
    init.signal = options.signal;
  }
  const res = await fetch(readApiBase() + path, init);
  if (!res.ok) {
    // 两个分支都是先赋值后使用 —— 声明时不带初始值，好让 `no-useless-assignment` 满意。
    let detail: string;
    try {
      const j = (await res.json()) as { code?: string; message?: string };
      detail = ` [${j.code ?? '?'}] ${j.message ?? ''}`;
    } catch {
      detail = ` ${await res.text().catch(() => '')}`;
    }
    throw new Error(`HTTP ${res.status} ${method} ${path}${detail}`);
  }
  // 204 No Content（DELETE、PUT-pref）→ 返回 undefined 并 cast 成 T。
  if (res.status === 204) return undefined as unknown as T;
  return (await res.json()) as T;
}
