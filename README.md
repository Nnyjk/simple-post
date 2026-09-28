# Simple Post

一款面向 Windows 10/11 的现代化桌面 API 调试 / 管理工具，核心差异化：**自带 MCP Server**，让 AI（Claude Desktop、Cursor、Cline）能直接读写接口、生成文档、参与调试。

> 当前阶段：**v0.1 MVP 已交付**（详见下方「路线图」）。四种分发形态均可运行：Tauri 桌面应用、`serve-web` 无头 REST + SPA、Docker web 镜像、以及独立的 `simple-post-mcp` npm 包。数据层已从早期内存 mock 迁移为 SQLite 持久化。

## 跑起来

```bash
npm install
npm run dev            # 启动 Vite dev server → http://127.0.0.1:5173（LAN 可访问）
npm run build          # 类型检查 + 打包 → dist/
npm run check          # 一键质量门禁：Prettier + ESLint + tsc + 单测 + cargo fmt + clippy
npm run lint[:fix]     # 只跑 ESLint；npm run format 只跑 Prettier
npm run test           # 单元测试（自动发现 src/**/*.test.ts）
npm run test:mcp       # MCP 端到端（需要编译后的 simple-post binary）
npm run test:web:auth      # serve-web bearer auth 流程集成测试（需要 binary）
npm run test:web:basepath  # serve-web reverse-proxy base path 集成测试
npm run perf:bench      # serve-web 性能基准（p50/p95/p99，可选 --assert 卡 200ms）
```

> 推送到 `main` 或提 PR 时，`.github/workflows/ci.yml` 会自动跑同一套门禁（web：格式/lint/类型/单测/构建；Rust：fmt/clippy/测试）。

### 反向代理（nginx / Caddy）

如果要把 `simple-post serve-web` 部署在 `https://host/sp/`，需要：

1. 用 `--public-base-path /sp`（或 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`）启动 binary —— commit 5
2. 反代转发 `/sp/*` → `http://127.0.0.1:4173/`，并把 `/sp` 前缀剥离

完整示例见：

- [`docs/reverse-proxy-nginx.conf`](docs/reverse-proxy-nginx.conf) — nginx 1.24+，含健康探针 + `/sp` → `/sp/` 重定向 + WS 升级
- [`docs/reverse-proxy-caddyfile`](docs/reverse-proxy-caddyfile) — Caddy 2.7+，含 ACME 自动 TLS + `uri_strip_prefix` 模式

> Vite 已配置 `host: '0.0.0.0'` + `allowedHosts: true`，局域网其他设备可直接访问本机 IP:5173。

## 已实现

### 数据层（SQLite + Zustand 状态缓存）
- 4 级层级：Project → Module → Collection → Endpoint，外加 Environment / Response
- 持久化：Zustand slice → `invokeCmd()` → Tauri IPC 或 `/api/*` → `simple-post-core` → SQLite（WAL 模式）
- 完整 CRUD：项目 / 模块 / 集合 / 接口的增删改查 + 复制接口 + 级联清理
- 排序：模块 / 集合 / 接口的上下移
- 响应：每个 endpoint 最多 10 条历史（FIFO）+ 用户保存的示例
- 偏好持久化：主题（dark/light/system）+ 请求设置（超时 / 跟随重定向 / 响应上限）写 localStorage

### 顶栏
- 项目 / 环境切换（点击下拉）
- 全局搜索（带 `Ctrl+K` 提示）
- MCP Server 状态指示器（running 绿点 / 离线灰点）
- 设置入口

### 侧边树（可展开 + 过滤）
- 4 级：项目 → 模块 → 集合 → 接口
- 每个集合 / 接口显示 method 彩色徽标
- 顶栏 `+` 真实弹新建弹窗
- 双击树项 inline 改名
- 右键菜单：新建子项 / 重命名 / 上下移 / 删除 / 复制接口
- 删除弹确认框

### 请求测试器
- Method 选择 + URL 输入（变量高亮显示）+ Send + **Copy as cURL** + Save
- Tabs：Params / Headers / Body / Auth / Docs / Tests
  - CodeMirror 6 JSON 编辑器，实时语法校验
  - Markdown 文档编辑器，分屏预览
  - Auth 4 种：None / Bearer / Basic / API Key
  - Tag 输入：chips 样式、逗号/回车添加、Backspace 删
- URL 输入 `{{` 触发 env 变量选择浮层（方向键选 + Enter 插入）
- 全局快捷键：`Ctrl+Enter` 发送 / `Ctrl+S` 保存 / `Ctrl+K` 命令面板

### Ctrl+K 命令面板
- 模糊搜索跨项目 / 模块 / 集合 / 接口
- 内置命令：切换环境、切换主题、打开设置、新建项目、复制当前 URL
- 键盘可达：↑↓ 选择 / Enter 触发 / Esc 关闭

### 响应区
- 状态码 / 耗时 / 大小 + 复制 / 下载
- Pretty / Raw / Headers 三个视图
- "Save as Example" 按钮（保存到接口示例）
- 响应历史折叠条（最近 10 条，可点击重放）
- 空集合 / 未发送时显示友好引导

### 设置抽屉
- 环境变量：3 个环境 × 3 个变量，`{{var}}` 实时编辑
- MCP Server 状态：running 指示 + daemon 实时上报的已注册工具列表（数量动态显示，取决于当前 capability 策略下的可见工具集）
- 主题：dark / light / system 切换，立即生效并持久化
- 请求设置：超时 / 跟随重定向 / 最大响应体积
- 快捷键速查

### 主题
- dark / light / system 三选一
- 切换瞬时生效，刷新保留
- 全局 html class 切换，无白屏闪烁

### Mock 数据
- 3 项目 / 6 模块 / 8 集合 / 12 接口 / 3 环境
- 覆盖 SMS 登录、密码登录、用户资料、订单 CRUD、状态流转、收银台、抬头、开票、设备影子读/写

## 路线图

UI 先行阶段的所有打磨项（P0/P1/P2）已落地。v0.1 已交付；下面是后续阶段。**注意**：v0.2 最初列的部分条目在实现过程中已提前完成（下方已勾选），勾选状态以代码为准。

### v0.1 MVP — 已全部交付

- [x] **Tauri 壳**：`src-tauri/` 初始化、Cargo.toml、tauri.conf.json、图标
- [x] **Rust 主进程**：SQLite（`rusqlite` + `r2d2`）、HTTP（`reqwest`）、MCP 子进程桥
- [x] **真实持久化**：Zustand store → Tauri command → `simple-post-core`（SQLite via `rusqlite`）
- [x] **真实请求**：渲染端 → Tauri command → `reqwest` → 返回结构化响应
- [x] **MCP Server**：拆到独立 `crates/simple-post-mcp/` workspace member，`daemon_http.rs`（axum HTTP daemon）+ `stdio_loop.rs`（stdio bridge）+ `mcp/mod.rs`（JSON-RPC dispatcher）— 向 AI 客户端（Claude Desktop / Cursor / Cline）暴露 44 个 MCP 工具。同一 crate 也以独立 `simple-post-mcp` 二进制 + `@simple-post/mcp-server` npm 包分发。
- [x] **打包**：NSIS 安装包 + 便携 zip

### v0.2（+2 周）

- [x] 鉴权扩展：Bearer / Basic / API Key（`AuthType` 四变体已落地，含 apikey 的 header/query 注入）
- [ ] 鉴权扩展：OAuth2 简化版
- [ ] 响应历史 & 收藏（持久化到 SQLite）
- [ ] 导入 Postman v2.1 / OpenAPI 3.0
- [ ] 导出 OpenAPI 3.0
- [x] MCP 写工具：以 `endpoint_create` / `endpoint_update` / `endpoint_search` / `endpoint_delete` 交付（v0.2 原计划的 `api_*` 命名未采用，见 44 工具清单）
- [ ] MCP 进程可视化（设置页：PID、状态、tail 日志）

### v0.3（+2 周）

- AI 文档生成（端点 → Markdown 文档）
- AI 测试用例生成（schema → 测试）
- 请求脚本（pre-request / test 脚本，沙箱隔离）
- 集合批量运行 + 简易报告
- 团队协作（Git 同步 JSON 工作区，可选）

### v1.0

- WebSocket / SSE / GraphQL
- Mock Server
- 性能压测（简易）
- 插件系统（允许第三方 MCP 工具注册）

### MCP 工具清单（stdio + HTTP 模式，给外部 AI 客户端）

> App 启动时同时拉起一个 **stdio MCP Server** 子进程（Claude Desktop / Cursor / Cline 通过 stdio 通信），并通过 `serve-web` 子命令的 `/mcp` 端点暴露 HTTP MCP（外部 AI 客户端、远程调试器）。Server 默认不暴露网络，鉴权可选；详见 [`docs/mcp-permission-policy.md`](docs/mcp-permission-policy.md)。

实际注册 **44 个** MCP 工具（精确值由 `every_registered_tool_has_a_capability` 测试守护）。每个工具在 `REQUIRED_CAPABILITY`（`crates/simple-post-mcp/src/mcp/mod.rs`）中声明一个 `Capability`，总序为 `Read < Create < Update < Delete < Send`；用户的全局策略 `CapabilityPolicy` 决定可见集：

| 策略 | 覆盖的 Capability | 含义 |
|---|---|---|
| `read_only`（默认） | `Read` | 只读观察，不发网络请求、不改状态 |
| `safe_write` | `Read` `Create` `Update` | 允许增删改，但**不含** `Send` |
| `full_access` | 全部（含 `Delete` `Send`） | 允许出站 HTTP 调用与破坏性操作 |

- **Read**：`project_list` / `project_get` / `module_list` / `collection_list` / `endpoint_get` / `endpoint_search` / `base_url_def_list` / `env_list` / `env_get_active` / `api_resolve_*` / `api_parse_response` / `api_diff_responses` / `api_extract_jsonpath`
- **Create**：`project_create` / `module_create` / `collection_create` / `endpoint_create` / `endpoint_duplicate`
- **Update**：`project_update` / `module_update` / `module_move` / `collection_update` / `collection_move` / `endpoint_update` / `endpoint_move` / `env_update` / `env_set_var` / `env_set_baseurl` / `env_set_active` / `base_url_def_update` / `base_url_def_set_default`
- **Delete**：`project_delete` / `module_delete` / `collection_delete` / `endpoint_delete` / `env_delete` / `base_url_def_delete`
- **Send**（仅 `full_access`）：`api_run_request` / `api_call_endpoint`

> 全局策略之外还有一道**按项目**的权限闸（`simple_post_core::permissions`，`Read < ReadCall < Full`，默认 `Full`）。两道闸是「与」关系，取更严格者。

完整列表 + 入参 / 出参 + 权限矩阵见 [`crates/simple-post-mcp/src/mcp/mod.rs`](crates/simple-post-mcp/src/mcp/mod.rs)。同名 HTTP REST 接口见 [`docs/http-api-reference.md`](docs/http-api-reference.md)（`scripts/dump-openapi.mjs` 重新生成 `docs/openapi.yaml`）。

Resources（AI 侧"读取"通道，HTTP MCP 模式下暴露）：

- `simple-post://projects`
- `simple-post://projects/{id}`
- `simple-post://endpoints/{id}`
- `simple-post://environments/{projectId}`

## Docker（web shape 一键部署）

`simple-post` 的 `serve-web` shape 可以容器化部署到任何带 Docker 的 Linux 主机（amd64 / arm64）。

```bash
# 1. 准备构建产物（Dockerfile.web 把 dist/ 拷进镜像）
npm install && npm run build

# 2. 构建镜像（约 50MB 运行时镜像，alpine + musl）
docker build -f Dockerfile.web -t simple-post:web .

# 3. 启动容器
docker run -d --name simple-post \
  -p 4173:4173 \
  -e SIMPLE_POST_WEB_PASSWORD=changeme \
  -v simple-post-data:/data \
  simple-post:web

# 4. 验证
curl http://localhost:4173/api/healthz
# {"status":"ok","version":"…","shape":"server"}

# 5. 接 MCP 客户端：Claude Desktop / Cursor / Cline 的 mcpServers 里配
#    url=http://host:4173/mcp  + Authorization: Bearer changeme
```

或者用 compose 拉现成镜像：

```bash
cp .env.example .env && $EDITOR .env   # 至少填 SP_WEB_PASSWORD
docker compose up -d
docker compose ps                       # STATUS=healthy
```

镜像发布到 GHCR：推 `v*` tag 触发 `.github/workflows/docker-build.yml`，输出 `ghcr.io/nnyjk/simple-post:web-v<version>`（多架构 manifest）。

安全提示：

- `SIMPLE_POST_WEB_PASSWORD` **必须**设，否则 `/api/*` 和 `/mcp` 全开放（`auth_middleware` 在 password 为空时是 no-op）。
- 健康检查路径 `/api/healthz`、`/mcp/healthz`、`/healthz` 始终免认证，方便容器探针。
- 反代部署（nginx / Caddy）时设 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`，`build_router` 会把整个 router nest 在 `/sp` 下；nginx 侧记得把 `/sp/assets/*` 转到 `/assets/*`（SPA shell 的 `<base href>` 仍是 `/`，**不**重写）。
- 数据持久化：把 `/data` 挂到 named volume。SQLite 在 WAL 模式下也能安全落盘，但 `docker stop` 走 `tini` 转发 SIGTERM 才会触发优雅 checkpoint。

详见：

- `Dockerfile.web` — Alpine 多阶段 musl 构建
- `docker/healthcheck.sh` — 容器内健康检查
- `docker-compose.yml` — 参考部署（强制要求 `SP_WEB_PASSWORD`）
- `.github/workflows/docker-build.yml` — buildx 多架构 + GHCR 发布

## 决策记录

- **技术栈**：Tauri 2 + React 18 + TypeScript（前端） + Cargo workspace（后端：`simple-post` Tauri shell / `simple-post-core` 共享层 / `simple-post-mcp` 独立 MCP 服务）
- **存储**：SQLite（`rusqlite` + `r2d2`，整个 workspace 通过 `simple-post-core::db` 共享连接池；WAL + `foreign_keys=ON` + `busy_timeout=3000`）
- **MCP 模式**：stdio 子进程桥（本地 AI 客户端）+ HTTP `POST /mcp`（`serve-web` 子命令，支持远程 / Bearer 鉴权 / 反代 base path）
- **UI 风格**：暗色为主，shadcn 变量层，可切亮色
- **第一阶段**：UI 先行 ✓（已完成 2026-09-11）
- **当前阶段**：v0.1 MVP 已交付，分发形态决策见 `docs/distribution-modes-2026-09-18.md`