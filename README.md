# Simple Post

一款面向 Windows 10/11 的现代化桌面 API 调试 / 管理工具，核心差异化：**自带 MCP Server**，让 AI（Claude Desktop、Cursor、Cline）能直接读写接口、生成文档、参与调试。

> 当前阶段：**UI 先行已完成**（2026-09-11）。脚手架 + Tailwind/shadcn 基线 + 完整数据层 + 四大主面板 + 所有 P0/P1 交互全部就位。下一步：进入真实业务层（v0.1 MVP），详见下方「路线图」。

## 跑起来

```bash
npm install
npm run dev        # 启动 Vite dev server → http://127.0.0.1:5173（LAN 可访问）
npm run build      # 产物到 dist/
npm run build      # 类型检查 + 打包（已有 tsc 内嵌）
npm run test       # 单元测试（http-build + http，纯函数 + node:http 集成）
npm run test:mcp   # MCP 端到端（需要编译后的 simple-post binary）
npm run test:web:auth    # serve-web bearer auth 流程集成测试（需要 binary）
npm run test:web:basepath# serve-web reverse-proxy base path 集成测试
npm run perf:bench      # serve-web 性能基准（p50/p95/p99，可选 --assert 卡 200ms）
```

### 反向代理（nginx / Caddy）

如果要把 `simple-post serve-web` 部署在 `https://host/sp/`，需要：

1. 用 `--public-base-path /sp`（或 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`）启动 binary —— commit 5
2. 反代转发 `/sp/*` → `http://127.0.0.1:4173/`，并把 `/sp` 前缀剥离

完整示例见：

- [`docs/reverse-proxy-nginx.conf`](docs/reverse-proxy-nginx.conf) — nginx 1.24+，含健康探针 + `/sp` → `/sp/` 重定向 + WS 升级
- [`docs/reverse-proxy-caddyfile`](docs/reverse-proxy-caddyfile) — Caddy 2.7+，含 ACME 自动 TLS + `uri_strip_prefix` 模式

> Vite 已配置 `host: '0.0.0.0'` + `allowedHosts: true`，局域网其他设备可直接访问本机 IP:5173。

## 已实现

### 数据层（Zustand + 内存 mock）
- 4 级层级：Project → Module → Collection → Endpoint，外加 Environment / Response
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
- MCP Server 状态：running 指示 + 已注册 4 个工具列表
- 主题：dark / light / system 切换，立即生效并持久化
- 请求设置：超时 / 跟随重定向 / 最大响应体积
- 快捷键速查

### 主题
- dark / light / system 三选一
- 切换瞬时生效，刷新保留
- 全局 html class 切换，无白屏闪烁

### Mock 数据
- 3 项目 / 7 模块 / 8 集合 / 12 接口 / 3 环境
- 覆盖 SMS 登录、密码登录、用户资料、订单 CRUD、状态流转、收银台、抬头、开票、设备影子读/写

## 路线图

UI 先行阶段的所有打磨项（P0/P1/P2）已落地。下面是接 v0.1 起的四阶段路线，叠加 MCP 工具集（核心差异化）的渐进暴露。

### v0.1 MVP（约 4 周）— 已在 commit 7 (b99151b) 全部交付

- [x] **Tauri 壳**：`src-tauri/` 初始化、Cargo.toml、tauri.conf.json、图标
- [x] **Rust 主进程**：SQLite（`rusqlite` + `r2d2`）、HTTP（`reqwest`）、MCP 子进程桥
- [x] **真实持久化**：Zustand store → Tauri command → `simple-post-core`（SQLite via `rusqlite`）
- [x] **真实请求**：渲染端 → Tauri command → `reqwest` → 返回结构化响应
- [x] **MCP Server**：拆到独立 `crates/simple-post-mcp/` workspace member，`daemon_http.rs`（axum HTTP daemon）+ `stdio_loop.rs`（stdio bridge）+ `mcp/mod.rs`（JSON-RPC dispatcher）— 暴露 44+ 个 MCP 工具给 AI 客户端（Claude Desktop / Cursor / Cline）。同一 crate 也以独立 `simple-post-mcp` 二进制 + `@simple-post/mcp-server` npm 包分发（commit 4）。
- [x] **打包**：NSIS 安装包 + 便携 zip

### v0.2（+2 周）

- 鉴权扩展：Bearer / Basic / API Key / OAuth2 简化版
- 响应历史 & 收藏（持久化到 SQLite）
- 导入 Postman v2.1 / OpenAPI 3.0
- 导出 OpenAPI 3.0
- MCP 工具扩展：`api_create` / `api_update` / `api_search` / `api_delete`
- MCP 进程可视化（设置页：PID、状态、tail 日志）

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

实际暴露的工具集（44 个）按 capability tier 分组（`REQUIRED_CAPABILITY` 见 `crates/simple-post-mcp/src/mcp/mod.rs`）：

- **read_only（默认）**：`project_list` / `project_get` / `module_*` / `collection_*` / `endpoint_get` / `endpoint_search` / `env_list` / `env_get` 等
- **safe_write**：`project_*_create` / `project_*_update` / `module_*` / `collection_*` / `endpoint_create` / `endpoint_update` / `env_create` / `env_update` 等
- **full_access**：`endpoint_delete` / `env_delete` / `http_send` 等会改变外部状态的操作

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

- `SIMPLE_POST_WEB_PASSWORD` **必须**设，否则 `/api/*` 和 `/mcp` 全开放（commit 5 的 `auth_middleware` 在 password 为空时是 no-op）。
- 健康检查路径 `/api/healthz`、`/mcp/healthz`、`/healthz` 始终免认证，方便容器探针。
- 反代部署（nginx / Caddy）时设 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`，commit 5 的 `build_router` 会把整个 router nest 在 `/sp` 下；nginx 侧记得把 `/sp/assets/*` 转到 `/assets/*`（SPA shell 的 `<base href>` 仍是 `/`，**不**重写）。
- 数据持久化：把 `/data` 挂到 named volume。SQLite 在 WAL 模式下也能安全落盘，但 `docker stop` 走 `tini` 转发 SIGTERM 才会触发优雅 checkpoint。

详见：

- `Dockerfile.web` — Alpine 多阶段 musl 构建
- `docker/healthcheck.sh` — 容器内健康检查
- `docker-compose.yml` — 参考部署（强制要求 `SP_WEB_PASSWORD`）
- `.github/workflows/docker-build.yml` — buildx 多架构 + GHCR 发布

## 协作方法（团队模式硬约束）

- **多 worker 并行只交付代码，禁止自行浏览器 / dev server 验证**——所有端到端验证、截图、流程跑通由**主会话统一做一次**。
- 详见 user memory 中的"多 worker 并行：只交付代码，不做端到端验证"条目。

## 决策记录

- **技术栈**：Tauri 2 + React 18 + TypeScript（前端） + Cargo workspace（后端：`simple-post` Tauri shell / `simple-post-core` 共享层 / `simple-post-mcp` 独立 MCP 服务）
- **存储**：SQLite（`rusqlite` + `r2d2`，整个 workspace 通过 `simple-post-core::db` 共享连接池；WAL + `foreign_keys=ON` + `busy_timeout=3000`）
- **MCP 模式**：stdio 子进程桥（本地 AI 客户端）+ HTTP `POST /mcp`（`serve-web` 子命令，支持远程 / Bearer 鉴权 / 反代 base path）
- **UI 风格**：暗色为主，shadcn 变量层，可切亮色
- **第一阶段**：UI 先行 ✓（已完成 2026-09-11）
- **当前阶段**：v0.1 MVP 已交付（commit 7，2026-09-21），后续规划见 commit 5+ 的 distribution 决策 + `docs/distribution-modes-2026-09-18.md`