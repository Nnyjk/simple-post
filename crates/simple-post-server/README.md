# simple-post-server

Simple Post 的无头服务端宿主，负责提供 REST API、托管 SPA 静态资源，并管控 MCP 守护进程的完整生命周期。

## 负责什么、不负责什么

### 负责什么
- **REST API 与 SPA 静态资源托管（`web_api`）**：基于 axum 提供完整的业务 REST 接口与前端单页应用静态资产交付，为 `serve-web` 命令的主入口。
- **双传输通道编排层（`project_api`、`interchange_api` 等）**：为桌面端 Tauri 命令与无头 REST 接口提供统一的业务编排（项目增删、动态变量、环境配置、数据导入导出、日志等），外部传输壳仅做协议解析，业务逻辑绝不分叉。
- **MCP 守护进程监管（`daemon_supervisor`）**：跨平台启动子进程（Windows 静默启动 `CREATE_NO_WINDOW`）、存活探针探测（`/mcp/healthz`）、输出缓冲环与异常退出自动重启。
- **端口注册与多实例协商（`port_registry`）**：维护 `mcp.port` 文件，写入包含监听配置指纹（`fingerprint`）的记录，实现多实例互斥与无缝接管（详见下文）。
- **身份认证中间件（`auth`）**：处理 Bearer Token 鉴权并维护公开路径白名单。

### 不负责什么
- 不包含任何 Tauri 桌面窗口或 Linux GTK/WebKit GUI 渲染逻辑（默认不依赖 Tauri，`gui` feature 仅用于桌面模式转发崩溃事件）。
- 不实现 MCP JSON-RPC 协议与具体 48 个工具的内部逻辑（由 `simple-post-mcp` 负责）。
- 不直接维护 SQLite 表结构定义与底层数据库迁移（由 `simple-post-core::db` 负责）。

## 目录与关键类型

```
crates/simple-post-server/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                # 公共导出接口与跨 crate 共享转发
    ├── main.rs               # 无头服务端二进制：`serve-web` 命令入口
    ├── cli.rs                # 两个二进制共用的 CLI 片段（serve-web / mcp）
    ├── auth.rs               # Bearer Token 认证中间件与公开白名单路径
    ├── dto.rs                # 双传输通道共享的传输对象数据结构
    ├── http_config.rs        # MCP 监听器配置与其指纹计算
    ├── dynamic_api.rs        # 动态变量业务编排
    ├── environment_api.rs    # 环境增删改查业务编排
    ├── interchange_api.rs    # 数据导入导出业务编排
    ├── log_api.rs            # 日志导出业务编排
    ├── log_layer.rs          # 汇入日志环形缓冲区的 tracing layer
    ├── mcp_router.rs         # 挂载在 MCP 守护进程独立端口上的路由
    ├── project_api.rs        # 项目与资源树节点业务编排
    ├── spec_api.rs           # 接口规范解析与写入业务编排
    ├── update_api.rs         # 软件更新检查与应用业务编排
    ├── daemon_supervisor.rs  # MCP 守护进程监管层门面
    ├── daemon_supervisor/    # 守护进程监管实现，按关注点拆分
    │   ├── listen_config.rs  #   持久化监听配置到子进程启动参数的映射
    │   ├── log_buffer.rs     #   尾部日志有界环形缓冲区
    │   ├── probe.rs          #   healthz 探针与「重复启动为空操作」判定规则
    │   ├── reap.rs           #   子进程终止控制：PID 跟踪与信号通道
    │   ├── state.rs          #   单次启动生命周期记账
    │   ├── status.rs         #   运行状态枚举与状态快照投影
    │   ├── stderr.rs         #   守护进程标准错误输出转发
    │   └── watcher.rs        #   子进程退出监听器
    ├── port_registry.rs      # `mcp.port` 端口文件门面
    ├── port_registry/        # 端口注册中心实现（路径与记录数据结构位于 core）
    │   ├── claim.rs          #   多实例抢占式注册（create_new 与回退机制）
    │   ├── health.rs         #   `/mcp/healthz` 存活探针
    │   └── proc.rs           #   进程表存活探测与强制终止
    ├── web_api/              # REST 接口与静态文件服务层
    │   ├── dto.rs            #   Web 端专用传输数据结构
    │   ├── error.rs          #   AppError 到 HTTP 状态码的映射
    │   ├── router.rs         #   路由树装配与 WebState 共享状态
    │   ├── serve.rs          #   Web 服务启动入口
    │   ├── static_files.rs   #   前端 SPA 单页入口与带哈希静态资产交付
    │   ├── routes/           #   各资源路由处理器，一文件对应一资源
    │   └── spec/             #   OpenAPI 契约映射表
    ├── win_spawn.rs          # Windows 平台子进程 `CREATE_NO_WINDOW` 静默启动标志
    └── (the tests sit beside their source as `*_test.rs` / `mod_test.rs`)
```

### 双传输通道共用一套编排层
桌面端命令（`src-tauri/src/commands/`）与 REST 路由处理器（`web_api/routes/`）共同暴露的功能统一在此编排：`interchange_api`（导入导出）、`project_api`（项目与树节点增删改）、`spec_api`（单接口规范）、`dynamic_api`（动态变量）、`environment_api`（环境管理）、`log_api`（日志导出）以及 `update_api`（检查与应用更新）。两端的命令/路由处理器仅为最外层薄壳（Shell）。

薄壳只做线格式解析：REST 端认 `POST /api/projects` 的两种历史载荷并补齐缺失 id，Tauri 端把 `kind` 字符串解析成枚举。id 归谁铸、时间戳、`root_id` 推导与事务边界都在编排层。薄壳一旦越界写业务逻辑，另一条通道就静默少一半功能、编译却不报错；`project_api_shells_test.rs` 读四处薄壳的源码，要求它们都调编排层。

### MCP 守护进程生命周期与指纹重用机制（此处详述）
监管者通过 `port_registry` 将守护进程的运行信息写入 `mcp.port` 文件，并附带守护进程实际监听配置的哈希指纹（`fingerprint`）。指纹作为顶级键存放在 JSON 中，保持与历史读取端（stdio 代理、外部脚本）的兼容性。

监管者启动时的处理规则：
1. **进程复用（Adopt）**：若 `mcp.port` 中记录的 PID 进程存活、`/mcp/healthz` 响应正常，且记录中的配置指纹与当前监管者期望的配置完全一致，监管者直接接管该守护进程。保留其 PID 且不重启进程，确保正在执行的外部 MCP 会话不中断。
2. **强制重启（Restart）**：若配置指纹不一致，或文件中缺失指纹字段（旧版本写入），或健康探测失败，监管者判定为配置已漂移，终止旧进程并启动新守护进程，重新写入 `mcp.port`。

`serve-web` 与桌面主程序均遵循相同的指纹协商规则，避免互相无条件终止对方已拉起的守护进程。

### 下游消费者与单向依赖
```text
simple-post (Tauri)  ──▶  simple-post-server  ──▶  simple-post-core
                                  │
                                  └──▶  simple-post-interchange
                                  └──▶  simple-post-mcp
```
- `src-tauri`（Tauri 桌面外壳与桌面二进制下的 `serve-web` 子命令）：通过 `pub use simple_post_server::{…}` 转发服务端模块，使桌面端仍能按 `crate::daemon_supervisor::…` 和 `crate::web_api::…` 路径调用。
- 可选的 `gui` 特性（`features = ["gui"]`）默认关闭；开启时会重新接入三个接收 `AppHandle` 的守护进程方法（`ensure_started`、`start`、`restart`），以便将守护进程崩溃事件转发给桌面端 webview。默认构建完全不依赖 Tauri。
- 容器镜像构建：Docker 镜像直接编译 `simple-post-server` 二进制。该构建不依赖 Tauri，因而无需打包 X11、GTK 或 WebKit 库。

## 怎么跑

### 运行无头 Web 服务
```bash
cargo run -p simple-post-server -- serve-web --bind 127.0.0.1 --port 4173
```

### 运行 MCP 守护进程
```bash
cargo run -p simple-post-server -- mcp --port 4333
```

### 运行单元测试
```bash
cargo test -p simple-post-server
```
