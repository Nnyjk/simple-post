/**
 * web 形态的 REST 路由表。
 *
 * 路由表对应 `crates/simple-post-server/src/web_api/` 里的 axum 路由树
 * （`.route()` 注册在 `mod.rs::build_router`）。两侧不共享源码 —— 靠
 * `scripts/mcp-integration.test.mjs` 的集成测试与 PR 时的手工复核保持一致。
 * 新增 invoke 命令必须在**两处**都补上对应的 REST 条目。
 */
import { COMMANDS, type TauriCommand } from './commands';
export interface RouteSpec {
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
export function buildQuery(params: Record<string, unknown>): string {
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
export const WEB_ROUTES: Partial<Record<TauriCommand, RouteSpec>> = {
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
  // 懒加载层级树：展开一个节点只取它的直接孩子；给项目 id 则取那个项目的模块。
  [COMMANDS.nodesChildren]: {
    method: 'GET',
    path: (a) =>
      `/api/nodes/children${buildQuery({ parentId: a.parentId, projectId: a.projectId })}`,
  },
  // 筛选：跨全部项目搜接口，带它所在的路径。
  [COMMANDS.nodesSearchPaths]: {
    method: 'GET',
    path: (a) => `/api/nodes/search${buildQuery({ q: a.query, limit: a.limit })}`,
  },
  [COMMANDS.nodesCreate]: {
    method: 'POST',
    path: () => '/api/nodes',
    // `NodeCreateBody` 把节点字段直接放在最外层，所以请求体就是节点自身字段 —— 不是
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
    // `newId` 是副本落库所用的 id —— SPA 乐观地铸好它，需要服务端照用；`null` 表示
    // 由服务端自己生成。
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
  [COMMANDS.nodeScriptsGet]: {
    method: 'GET',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/scripts`,
  },
  [COMMANDS.nodeScriptsUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/nodes/${encodeURIComponent(String(a.nodeId))}/scripts`,
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
  [COMMANDS.environmentsSetVarDisabled]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars/disabled`,
    body: (a) => ({ key: a.key, disabled: a.disabled }),
  },
  [COMMANDS.environmentsRenameVar]: {
    method: 'POST',
    path: (a) => `/api/environments/${encodeURIComponent(String(a.envId))}/vars/rename`,
    body: (a) => ({ fromKey: a.fromKey, toKey: a.toKey }),
  },
  [COMMANDS.environmentsSetModuleBaseurl]: {
    method: 'POST',
    // 取值层是 (环境 × 模块 × 服务)，故单元格的两半都走请求体。路径段用
    // `module-baseurls` 而不是 `baseurls`：裸 `defId` 只能写扁平的、不分模块的取值层。
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

  // ----- dynamic variables -----
  // 定义挂在项目里，故这组路由带一个可选的项目参数：不带就是跨项目那一路。
  // 列表只读缓存行，不取数；`/resolve` 才会真的发出站请求。
  [COMMANDS.dynamicVariablesList]: {
    method: 'GET',
    path: (a) => `/api/dynamic-variables${buildQuery({ projectId: a.projectId })}`,
  },
  [COMMANDS.dynamicVariablesCreate]: {
    method: 'POST',
    path: () => '/api/dynamic-variables',
    // 显式投影：下面这些键就是全部线上契约，调用方多带的键不能跟着上车。
    body: (a) => ({
      projectId: a.projectId ?? null,
      name: a.name,
      id: a.id ?? null,
      description: a.description ?? null,
      sourceEndpointId: a.sourceEndpointId ?? null,
      sourceInlineJson: a.sourceInlineJson ?? null,
      sourceEnvironmentId: a.sourceEnvironmentId ?? null,
      sourceBaseUrlDefinitionId: a.sourceBaseUrlDefinitionId ?? null,
      extractPath: a.extractPath ?? null,
      transformScript: a.transformScript ?? null,
      valueMode: a.valueMode ?? null,
      ttlSeconds: a.ttlSeconds ?? null,
      staleFallback: a.staleFallback ?? null,
      enabled: a.enabled ?? null,
      sortOrder: a.sortOrder ?? null,
    }),
  },
  [COMMANDS.dynamicVariablesUpdate]: {
    method: 'PATCH',
    path: (a) => `/api/dynamic-variables/${encodeURIComponent(String(a.id))}`,
    body: (a) => a.patch,
  },
  [COMMANDS.dynamicVariablesDelete]: {
    method: 'DELETE',
    path: (a) => `/api/dynamic-variables/${encodeURIComponent(String(a.id))}`,
  },
  [COMMANDS.dynamicVariablesResolve]: {
    method: 'POST',
    path: () => '/api/dynamic-variables/resolve',
    body: (a) => ({
      names: a.names,
      // `projectId` 不传就是跨项目取值。
      projectId: a.projectId || null,
      force: a.force ?? false,
    }),
  },
  [COMMANDS.dynamicVariablesSetOverride]: {
    method: 'POST',
    path: (a) => `/api/dynamic-variables/${encodeURIComponent(String(a.id))}/override`,
    body: (a) => ({
      value: a.value ?? null,
      expiresAt: a.expiresAt ?? null,
      forever: a.forever ?? false,
    }),
  },
  [COMMANDS.dynamicVariablesRefresh]: {
    method: 'POST',
    path: (a) => `/api/dynamic-variables/${encodeURIComponent(String(a.id))}/refresh`,
    body: () => ({}),
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
    // 显式投影而非默认的 `body = args`：下面三半就是全部线上契约，调用方多带的
    // 键不能跟着上车。`request` 是那次发送出去的请求，缺席时省略。
    body: (a) => ({
      endpointId: a.endpointId,
      response: a.response,
      ...(a.request ? { request: a.request } : {}),
    }),
  },
  [COMMANDS.responseHistoryClear]: {
    method: 'DELETE',
    path: (a) => `/api/response-history/${encodeURIComponent(String(a.endpointId))}`,
  },
  // 字面量路径：与上面那条 `{endpointId}` 路由在 radix tree 的同一层，故不能带参数段。
  [COMMANDS.responseHistorySetFavorite]: {
    method: 'PUT',
    path: () => '/api/response-history/favorite',
    body: (a) => ({ id: a.id, favorite: a.favorite }),
  },
  // 保留条数是全局设置，不属于任何接口，故路径里没有 id。
  [COMMANDS.responseHistoryKeepGet]: {
    method: 'GET',
    path: () => '/api/response-history/keep',
  },
  [COMMANDS.responseHistoryKeepSet]: {
    method: 'PUT',
    path: () => '/api/response-history/keep',
    body: (a) => ({ keep: a.keep }),
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

  // ----- interchange (import / export) -----
  // 请求体就是 args 本身：opts 的键名与 `InvokePreviewOpts` 等 Rust 结构体的
  // `camelCase` 字段一一对应，无需 `body` 覆写。四条全是 POST —— 请求体装得下
  // 整份文档（一份 3 MB 的 OpenAPI yaml），而它们里没有一个是「按 id 取一行」。
  [COMMANDS.interchangeImportPreview]: {
    method: 'POST',
    path: () => '/api/interchange/import/preview',
  },
  [COMMANDS.interchangeImportCommit]: {
    method: 'POST',
    path: () => '/api/interchange/import/commit',
  },
  [COMMANDS.interchangeExportPreview]: {
    method: 'POST',
    path: () => '/api/interchange/export/preview',
  },
  [COMMANDS.interchangeExportDocument]: {
    method: 'POST',
    path: () => '/api/interchange/export/document',
  },

  // ----- interface specs -----
  // 请求体就是那份规格本身，不带外层包装：它与 MCP 工具 `spec_upsert` 的参数
  // 逐字段相同，故一份规格可以从 MCP 客户端原样搬到 HTTP 调用方。
  //
  // 字段名两种拼写都收（`project_id` 与 `projectId`）：服务端落在同一个类型上。
  [COMMANDS.specUpsert]: {
    method: 'POST',
    path: () => '/api/spec/upsert',
    body: (a) => a.spec,
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
  [COMMANDS.mcpDaemonLogTail]: { method: 'GET', path: () => '/api/mcp/logs' },
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
