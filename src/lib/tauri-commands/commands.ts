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
  environmentsSetVarDisabled: 'environments_set_var_disabled',
  environmentsRenameVar: 'environments_rename_var',
  environmentsSetModuleBaseurl: 'environments_set_module_baseurl',
  environmentsSetActive: 'environments_set_active',

  // ----- baseUrl definitions -----
  baseUrlDefsList: 'base_url_defs_list',
  baseUrlDefsCreate: 'base_url_defs_create',
  baseUrlDefsUpdate: 'base_url_defs_update',
  baseUrlDefsDelete: 'base_url_defs_delete',
  baseUrlDefsSetDefault: 'base_url_defs_set_default',

  // ----- interchange (import / export) -----
  // 两条边各两步：**先预览后落定**。预览只读，它回答「这份文件里有哪些条目、有哪些
  // 已经在项目里了、有哪些构造会丢」；用户勾完才提交。导出侧对称：先看候选清单，
  // 再产文档。中间态**不**存在服务端——落库那一步重新解析同一份原文，故不需要
  // 临时表、TTL 与清理路径，而解析是纯函数，两次结果必然一致。
  interchangeImportPreview: 'interchange_import_preview',
  interchangeImportCommit: 'interchange_import_commit',
  interchangeExportPreview: 'interchange_export_preview',
  interchangeExportDocument: 'interchange_export_document',

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
  /** daemon 最近若干行日志。有界的（最近 500 行），故永远不长。 */
  mcpDaemonLogTail: 'mcp_daemon_log_tail',
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
