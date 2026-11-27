# simple-post-core

Simple Post 的共享底层核心库，提供跨 crate 复用的领域模型、SQLite 存储、请求脚本沙箱与 HTTP 执行能力。

## 负责什么、不负责什么

### 负责什么
- **领域模型（`domain`）**：定义项目、环境、节点、端点、认证配置、OAuth2 结构、动态变量等核心数据类型。
- **数据库持久化与迁移（`db`）**：管理 SQLite 连接池（`DbPool`）、表结构版本迁移（`schema.rs`）、CRUD 增删改查以及事务辅助函数（`with_tx`）。
- **请求脚本沙箱（`script`）**：基于 QuickJS 的前后置脚本运行时（`send_with_scripts`），有执行时长与内存上限；脚本输出不许进 stdout（MCP 的 JSON-RPC 帧只走 stdout），release 下 panic 即中止进程。
- **HTTP 执行与变量插值（`http_executor`、`body`、`multipart`、`path_params`、`template`、`tls`、`url`、`oauth2`）**：执行出站 HTTP 请求、按不同模式装配请求体（含 `multipart/form-data` 装配与按路径流式读盘）、解析路径参数与 `{{name}}` 模板变量、处理 OAuth2 令牌生命周期与出站 TLS 设置。
- **权限策略与单接口规范写入（`permissions`、`spec::upsert`）**：管理项目级权限枚举、全局能力策略（`CapabilityPolicy`）以及单接口规范的原子解析与事务性落库。
- **端口文件契约（`port_file`）**：定义 `mcp.port` 文件的存储路径与数据结构规范（多实例端口竞争、探针探测与生命周期接管由 `simple-post-server` 负责，见 [crates/simple-post-server/README.md](../simple-post-server/README.md)）。

### 不负责什么
- 不提供任何网络监听服务（无 HTTP/REST 或 MCP 守护进程监听，由 `simple-post-server` 与 `simple-post-mcp` 负责）。
- 不负责子进程管理或 MCP 守护进程监管（由 `simple-post-server::daemon_supervisor` 负责）。
- 不解析外部格式或处理数据导入导出（Postman 与 OpenAPI 转换由 `simple-post-interchange` 负责）。
- 不依赖 Tauri，不包含任何桌面 GUI 相关的逻辑或资产。

## 目录与关键类型

```
crates/simple-post-core/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # 导出 body / db / domain / dynamic / error /
    │                          # http_executor / multipart / oauth2 / path_params /
    │                          # permissions / script / spec / template / tls / url
    ├── body.rs                # 请求体装配规则：各请求体形态的解析、插值与序列化
    ├── db/                    # SQLite 存储与 CRUD 模块
    │   ├── mod.rs             # 模块入口：init / ping / with_conn / with_tx /
    │   │                      # DbPool / migrate
    │   ├── pool.rs            # 连接池管理与事务辅助函数
    │   ├── schema.rs          # 数据库迁移：字段补齐与废弃列清理
    │   ├── schema.sql         # 建表 SQL 语句（无初始化数据）
    │   ├── fixtures.json      # 测试用例固件数据（#[cfg(test)]）
    │   ├── test_helpers.rs    # 测试辅助：共享连接池与临时目录构建
    │   ├── test_support.rs    # 测试辅助：fixtures.json 读取器
    │   ├── dynamic_variable_cache.rs
    │   ├── dynamic_variables.rs # dynamic_variables/ 的门面
    │   ├── dynamic_variables/ # 项目级动态变量定义表
    │   │   ├── read.rs        #   list / list_by_project / get /
    │   │   │                  #   get_by_name_in_project / get_by_name_across /
    │   │   │                  #   project_id_of / source_project_id_of
    │   │   ├── write.rs       #   create / update / delete
    │   │   ├── validation.rs  #   数据源绑定与变量名格式校验
    │   │   └── mapping.rs     #   数据库行到领域对象转换
    │   ├── endpoint_details.rs
    │   ├── environments.rs
    │   ├── node_scripts.rs    # 集合级前置脚本与断言测试脚本
    │   ├── nodes.rs           # nodes/ 的门面
    │   ├── nodes/             # 资源节点整树存储
    │   │   ├── read.rs        #   get / list_all / list_by_root /
    │   │   │                  #   list_by_root_and_type / list_children /
    │   │   │                  #   module_id_of / root_id_of / search /
    │   │   │                  #   search_paths / subtree_ids / type_of
    │   │   ├── write.rs       #   create / update / delete / duplicate /
    │   │   │                  #   reparent
    │   │   ├── tree.rs        #   move_many, move_to_root
    │   │   └── placement.rs   #   父子节点关系与层级约束契约
    │   ├── oauth2_tokens.rs
    │   ├── permissions.rs
    │   ├── preferences.rs
    │   ├── projects.rs
    │   └── response_history.rs
    ├── domain.rs              # domain/ 的门面：Project / Permission / Endpoint / Environment 等
    ├── domain/                # 领域核心类型，一文件对应一领域关注点
    │   ├── auth.rs            #   AuthConfig / AuthType / RequestBody / KeyValue
    │   ├── dynamic.rs         #   动态变量协议类型
    │   ├── http.rs            #   HttpMethod
    │   ├── oauth2.rs          #   OAuth2 协议类型
    │   ├── project.rs         #   Project / Environment
    │   ├── resource.rs        #   ResourceNode / EndpointDetail
    │   ├── response.rs        #   ApiResponse / StoredResponse
    │   ├── script.rs          #   请求脚本相关类型
    │   ├── settings.rs        #   Permission / RequestSettings / TlsSettings
    │   └── util.rs            #   uid / now_ms 及基础工具函数
    ├── dynamic/               # 动态变量求值引擎，按关注点拆分（merge /
    │                          # serialize / names / cache / extract / source /
    │                          # flight / fetcher / resolve / transform）
    ├── error.rs               # 统一错误枚举 AppError
    ├── http_executor.rs       # 基于 reqwest 的 HTTP 发送执行器（供桌面端与无头端调用）
    ├── multipart.rs           # multipart/form-data 装配与按路径流式读盘
    ├── oauth2.rs              # oauth2/ 的门面
    ├── oauth2/                # OAuth2 令牌生命周期流转
    │   ├── api.rs             #   双传输通道共享的统一入口
    │   ├── config.rs          #   OAuth2 配置中的 {{var}} 变量替换
    │   ├── form.rs            #   x-www-form-urlencoded 报文解码
    │   ├── http_client.rs     #   oauth2 crate 的出站网络适配器
    │   ├── inject.rs          #   将获取的令牌注入出站请求
    │   ├── parse.rs           #   令牌端点响应解析
    │   ├── store.rs           #   oauth2_tokens 数据库缓存
    │   ├── token_endpoint.rs  #   令牌端点单次往返请求
    │   └── validate.rs        #   各认证流程必填字段校验
    ├── path_params.rs         # 路径参数识别与替换
    ├── script/                # 请求脚本 JS 沙箱
    │   ├── mod.rs             # 门面与公共接口
    │   ├── prelude.rs         # 定义宿主 API 的内置 JS 引导脚本
    │   ├── engine.rs          # QuickJS 运行时管理、资源限制与中断处理器
    │   ├── chain.rs           # 脚本执行链装配与环境写回合并
    │   ├── run.rs             # 单阶段脚本执行（pre-request 或 test）
    │   ├── send.rs            # 完整请求执行序列（唯一实现，保证桌面端与 MCP 端完全对齐）
    │   ├── transform.rs       # 动态变量转换脚本求值
    │   ├── limits.rs          # 超时时间、内存上限与调用栈深度硬性阈值
    │   └── types.rs           # 沙箱内部类型定义
    ├── spec/                  # 单接口规范（Specification）解析与写入
    │   ├── mod.rs             # 寻址原语与门面
    │   ├── input.rs           # 共享输入参数强制类型转换
    │   └── upsert/            # 事务性原子写入
    │       ├── mod.rs         #   门面
    │       ├── types.rs       #   规范协议类型
    │       ├── resolve.rs     #   模块、集合与环境寻址解析
    │       └── write.rs       #   端点写入与完整事务控制
    ├── template.rs            # {{name}} 模板变量替换原语
    ├── tls.rs                 # HTTPS 出站 TLS 安全配置
    ├── url.rs                 # 百分比编码实现（逐字节对齐渲染进程 encodeURIComponent）
    └── permissions.rs         # 项目级 Permission 枚举与全局能力策略 CapabilityPolicy（read_only/safe_write/full_access）
```

### 请求脚本沙箱（`script/`）
`script/` 负责执行用户编写的 JavaScript 代码。该模块置于本层而非服务端或 MCP crate，是因为桌面端（`http_request` / `POST /api/http/send`）与 MCP 工具（`api_call_endpoint`）均统一调用 `script::send_with_scripts`，确保请求生命周期中脚本执行顺序与环境合并时序绝不分叉。

沙箱具备以下三个关键约束：
1. **宿主接口仅限内置 JS Prelude**：引擎未注册任何向外部暴露的 Rust 回调，脚本无法访问本地文件系统、网络或启动进程。
2. **硬性资源限制**：通过 QuickJS 中断处理器将超时转为不可捕获的运行时异常；内存上限依赖默认内存分配器（`rquickjs` 的 `rust-alloc` 与 `allocator` 特性保持关闭）。
3. **静默执行且禁止 Panic**：由于 MCP 守护进程的标准输出专属承载 JSON-RPC 帧，沙箱严禁向 stdout 打印任何内容（由 `stdout_test.rs` 保证）；release 模式下 panic 直接中止。

### 依赖关系与下游消费
本 crate 处于依赖树最底层，打破了上层服务之间的循环依赖：
- `crates/simple-post-server`：通过 `pub use simple_post_core::{db, domain, error, http_executor, oauth2, permissions, script, spec}` 重新导出核心模块，维持历史调用路径。
- `src-tauri`：通过 `pub use simple_post_core::{…}` 转发相同模块，保持桌面命令路径不变。
- `crates/simple-post-mcp`：直接以 `simple_post_core::db::*` 等路径引用各底层模块。

## 怎么跑

本 crate 为纯 Rust 库，不生成独立二进制产物。

运行单元测试：
```bash
cargo test -p simple-post-core
```
测试覆盖内存 SQLite 实例、测试固件校验、沙箱 stdout 禁止断言及请求体跨模式对齐测试。