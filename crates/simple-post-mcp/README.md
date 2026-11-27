# simple-post-mcp

Simple Post 的 MCP（Model Context Protocol）服务模块与独立 CLI 二进制，为外部 AI 客户端提供接口管理、变量解析与 HTTP 请求调用等 48 个工具。

## 负责什么、不负责什么

### 负责什么
- **MCP 协议实现与工具派发（`mcp`）**：定义 JSON-RPC 通信协议、工具描述符（`tool_table()`）以及 6 大类共 48 个 MCP 工具的派发（`dispatch`）。
- **三级能力安全门禁（`capability`、`permissions`）**：实现 `read_only` / `safe_write` / `full_access` 三级能力校验。后端根据用户设置确定上限，CLI 的 `--capability` 参数只收紧不放宽。
- **变量插值与端点解析（`variable_fill`）**：解析端点覆盖参数、合并环境变量与基础地址（`resolve_endpoint`）、处理认证信息并拼接查询参数。
- **响应解析与比对（`response_parse`）**：解析 HTTP 响应载荷、执行 JSONPath 提取及响应差异比对。
- **传输适配与独立运行（`main.rs`、`stdio_loop.rs`、`daemon_http.rs`）**：支持标准输入输出流桥接（stdio 模式，与守护进程通信）或独立 HTTP 监听模式（`--transport=http`）。

### 不负责什么
- 不负责后台守护进程的启动、保活、健康探测或重启（由 `simple-post-server::daemon_supervisor` 负责；守护进程端口发现与指纹协商机制详见 [crates/simple-post-server/README.md](../simple-post-server/README.md)）。
- 不直接管理底层 SQLite 数据库连接或执行表结构迁移（由 `simple-post-core::db` 负责）。
- 不提供 Web 管理界面或 SPA 静态资源托管（由 `simple-post-server::web_api` 负责）。

## 目录与关键类型

```
crates/simple-post-mcp/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # 重新导出 `pub mod mcp` 与独立运行模块
    ├── main.rs                # clap 命令行入口：stdio 循环或 --transport=http
    ├── error.rs               # ClientError 错误类型与 http_status_to_rpc_code 映射（HTTP 401 → -32001）
    ├── http_client.rs         # 封装支持可选 Bearer 认证的 reqwest::Client
    ├── stdio_loop.rs          # 行式 JSON-RPC ↔ HTTP 桥接循环
    ├── capability.rs          # --capability 本地工具过滤校验
    ├── daemon_http.rs         # 守护进程 axum HTTP 监听器
    ├── dynamic/               # 动态变量共享操作
    │   ├── mod.rs             #   模块入口
    │   ├── ops.rs             #   三方消费端共用的动态变量操作
    │   ├── preview.rs         #   临时表达式求值预览
    │   └── source.rs          #   SourceFetcher 接口实现
    ├── spec/                  # OpenAPI 规范导入工具
    │   ├── mod.rs
    │   └── ops.rs
    ├── variable_fill.rs       # variable_fill/ 变量插值规则的门面
    ├── variable_fill/         # 变量插值规则，按关注点拆分
    │   ├── auth.rs            #   bearer / basic / apikey 字段插值（oauth2 字段由 core 处理）
    │   ├── base_url.rs        #   模块基础 URL 解析与路径拼接
    │   ├── request.rs         #   请求头排序与查询参数拼接
    │   ├── resolve.rs         #   端点最终解析入口 resolve_endpoint
    │   ├── template.rs        #   {{name}} 变量替换（转发至 core）
    │   └── types.rs           #   EndpointOverrides 与 ResolvedEndpoint 类型定义
    ├── response_parse.rs      # 响应体解析、响应差异比对与 JSONPath 提取
    └── mcp/                   # JSON-RPC 请求分发器与具体工具实现
        ├── mod.rs             # 门面：AppState、dispatch、tool_descriptors、REQUIRED_CAPABILITY 表与 tool_fn! 宏
        ├── protocol.rs        #   JSON-RPC 协议类型定义
        ├── registry.rs        #   tool_table() 与 REQUIRED_CAPABILITY 静态注册表
        ├── dispatch.rs        #   JSON-RPC 方法分发与全局能力门禁
        ├── permissions.rs     #   能力层级权限判断
        ├── args.rs            #   JSON-RPC 入参解析辅助
        ├── state.rs           #   AppState 定义与 serve_http 入口
        ├── http_ops/          # HTTP 发送与变量解析工具
        │   ├── mod.rs         #   模块入口
        │   ├── args.rs        #   工具入参类型定义
        │   ├── batch.rs       #   api_run_collection 工具实现
        │   ├── call_endpoint.rs # api_call_endpoint 工具实现
        │   ├── run_request.rs #   api_run_request 工具实现
        │   ├── dynamic.rs     #   动态变量提取
        │   ├── envelope.rs    #   响应包装结构 Envelope
        │   ├── input.rs       #   入参扁平化与校验
        │   ├── project.rs     #   临时请求所属项目补齐
        │   ├── send.rs        #   请求发送路径与脚本执行环境快照
        │   └── table.rs       #   分发映射表与三种工具 schema
        ├── project_ops/       # 项目、集合、模块、端点与规范工具
        │   ├── endpoint.rs    #   endpoint/ 门面
        │   ├── endpoint/      #   端点相关工具实现
        │   │   ├── args.rs    #     入参类型定义
        │   │   ├── history.rs #     endpoint_history：端点历史调用记录
        │   │   ├── input.rs   #     入参类型转换
        │   │   ├── missing.rs #     必填字段缺失判断规则
        │   │   ├── patch.rs   #     端点局部更新数据转换
        │   │   ├── read.rs    #     端点读取工具
        │   │   ├── registry.rs #    端点 JSON Schema 定义
        │   │   ├── resource.rs #    节点与端点详情复合读取
        │   │   └── write.rs   #     端点写入工具
        │   ├── collection.rs  #   集合管理工具（增删改查、移动）
        │   ├── module.rs      #   模块管理工具（增删改查、移动）
        │   ├── project.rs     #   项目管理工具（增删改查）
        │   └── spec.rs        #   spec_upsert 接口规范写入工具
        ├── env_ops.rs         # 环境增删改查工具
        ├── dynamic_ops.rs     # 动态变量管理工具
        ├── response_parse.rs  # JSONPath 响应解析与差异比对工具
        ├── resources.rs       # MCP Resource 跨工具接入指引
        ├── key_value.rs       # 键值对共享解析辅助
        └── variable_fill.rs   # 变量插值工具
```

### 工具分类与能力档位
`tool_table()` 注册了 6 个大类共 48 个 MCP 工具：
- `env_ops`（7 个）：`env_list`、`env_get_default`、`env_create`、`env_update`、`env_delete`、`env_set_var`、`env_set_baseurl`
- `project_ops`（25 个）：
  - project（5 个）：`project_list`、`project_get`、`project_create`、`project_update`、`project_delete`
  - module（5 个）：`module_list`、`module_create`、`module_update`、`module_delete`、`module_move`
  - collection（5 个）：`collection_list`、`collection_create`、`collection_update`、`collection_delete`、`collection_move`
  - endpoint（9 个）：`endpoint_list`、`endpoint_get`、`endpoint_search`、`endpoint_history`、`endpoint_create`、`endpoint_update`、`endpoint_delete`、`endpoint_duplicate`、`endpoint_move`
  - spec（1 个）：`spec_upsert`
- `http_ops`（3 个）：`api_run_request`、`api_call_endpoint`、`api_run_collection`
- `variable_fill`（3 个）：`api_resolve_template`、`api_resolve_endpoint`、`api_validate_endpoint`
- `response_parse`（3 个）：`api_parse_response`、`api_diff_responses`、`api_extract_jsonpath`
- `dynamic_ops`（7 个）：`dynamic_var_list`、`dynamic_var_get`、`dynamic_var_create`、`dynamic_var_update`、`dynamic_var_delete`、`dynamic_var_resolve`、`dynamic_var_set_override`

`REQUIRED_CAPABILITY` 逐工具声明它需要的能力 `Capability`（`Read` / `Create` / `Update` / `Delete` / `Send`）；未登记的工具按未分类处理，直接拒绝。用户在设置里选的档位 `read_only` / `safe_write` / `full_access`（存在 `preferences.mcp.policy`）决定可见的工具集合，`Send` 只有 `full_access` 才覆盖。本二进制的 `--capability` 只在本地再收窄一次，改不了后端策略。

### 命令行参数
```
simple-post-mcp
  --url=<SIMPLE_POST_WEB_URL>          (可选；默认通过 mcp.port 发现守护进程端口)
  --password=<SIMPLE_POST_WEB_PASSWORD>   (可选，作为 Authorization: Bearer 发送)
  --transport=stdio|http               (默认: stdio)
  --port=<number>                       (仅用于 --transport=http，默认 4333)
  --capability=read_only|safe_write|full_access   (默认由后端策略决定)
  --log-level=info|debug
```
其中 `--url`、`--password` 和 `--capability` 分别支持环境变量 `SIMPLE_POST_WEB_URL`、`SIMPLE_POST_WEB_PASSWORD` 和 `SIMPLE_POST_MCP_CAPABILITY` 覆盖，命令行显式参数优先于环境变量。`--transport`、`--port` 与 `--log-level` 仅支持命令行传入。

## 怎么跑

### 安装独立二进制
在仓库根目录下执行：
```bash
cargo install --path crates/simple-post-mcp
```

### stdio 模式（AI 客户端启动外部进程）
AI 客户端（如 Claude Desktop、Cursor）通过 stdio 管道接入。未传入 `--url` 时，进程自动读取 `mcp.port` 连接本地守护进程（端口发现与监管机制见 [crates/simple-post-server/README.md](../simple-post-server/README.md)）。

### HTTP 模式（独立服务）
```bash
cargo run -p simple-post-mcp -- --transport=http --port=4333
```

### 客户端免安装二进制的接入方式
若客户端机器无法运行 Rust 二进制，可在服务端启动 `simple-post serve-web`，客户端直接配置 HTTP 端点：
```json
{
  "mcpServers": {
    "simple-post": {
      "type": "http",
      "url": "http://<host>:<mcp 端口>/mcp"
    }
  }
}
```

这个端口来自数据目录里 `mcp.port` 记录的 `port`（绑定地址、挂载路径与白名单也在同一份 `mcp.http_config` 里），由部署侧发布出来；主端口 `4173` 只服务 REST API 与 SPA。

### 历史代理二进制迁移
原位于 `src-tauri/src/bin/mcp_proxy.rs` 的 `simple-post-mcp-proxy` 已废弃移除，客户端配置迁移方式如下：
```diff
- {
-   "command": "simple-post-mcp-proxy"
- }
+ {
+   "command": "simple-post-mcp"
+ }
```

### 运行单元测试
```bash
cargo test -p simple-post-mcp
```