# Simple Post

一款面向 Windows 10/11 的现代化桌面 API 调试 / 管理工具，核心差异化：**自带 MCP Server**，让 AI（Claude Desktop、Cursor、Cline）能直接读写接口、生成文档、参与调试。

> 当前阶段：**v0.1.0 beta**（详见下方「路线图」）。三种分发形态均可运行：Tauri 桌面应用、`serve-web` 无头 REST + SPA、以及独立的 `simple-post-mcp` 二进制（不发布到 npm 包）。数据持久化在 SQLite。

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

1. 用 `--public-base-path /sp`（或 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`）启动 binary
2. 反代把 `/sp/*` **原样**转发到 `http://127.0.0.1:4173/sp/*`，**不要剥离前缀**——前缀由 binary 自己处理

关键点：**前缀只能由一处处理。** 早期版本让 nginx 剥离前缀，同时服务端 `nest`，于是在 `/sp` 下 SPA 资源全 404。这里必须由 binary 统一处理——`web_api` 会把整个 router（含 `/api` 与 `/mcp`）`nest` 在 `/sp` 下，并且按实际挂载前缀改写 SPA 外壳里的资源引用（见下方「部署到服务器」）。nginx 若再剥一次，两边就对不上了。

完整示例见：

> Vite 已配置 `host: '0.0.0.0'` + `allowedHosts: true`，局域网其他设备可直接访问本机 IP:5173。

## 已实现

### 数据层（SQLite + Zustand 状态缓存）
- 层级：Project → Module → Collection（**可递归嵌套，深度不设上限**）→ Endpoint，外加 Environment / Response
- 整棵树存在**一张 `nodes` 表**里，用 `type` 字段区分模块 / 集合 / 接口；层级由 `parent_id` 表达，所属项目由 `root_id` 反规范化在每一行上（省掉一次向上追溯）
- 接口的请求配置（method / url / auth / body / headers / params / tags）单独放 `endpoint_details` 侧表，按 node id 关联——这些字段对模块和集合没有意义
- 类型矩阵是**被强制的契约**（SQLite trigger + 应用层双防线）：模块只能在顶层，接口可以挂在集合下、也可以直接挂在模块下，集合可以挂在模块或另一个集合下
- 持久化：Zustand slice → `invokeCmd()` → Tauri IPC 或 `/api/*` → `simple-post-core` → SQLite（WAL 模式）
- 完整 CRUD：项目 / 模块 / 集合 / 接口的增删改查 + 复制接口 + 级联清理（级联由 schema 保证，客户端只提交要删的节点 id）
- 排序：模块 / 集合 / 接口的上下移
- 响应：每个 endpoint 最多 10 条历史（FIFO，**仅内存**，重启即失）+ 用户保存的示例
- 偏好持久化：主题（dark/light/system）+ 请求设置（超时 / 跟随重定向 / 响应上限）写 localStorage

### 顶栏
- 项目 / 环境切换（点击下拉）
- 全局搜索（带 `Ctrl+K` 提示）
- MCP Server 状态指示器（running 绿点 / 离线灰点）
- 设置入口

### 侧边树（可展开 + 过滤）
- 项目 → 模块 → 集合 → 接口，**集合层用递归组件渲染，嵌套多深都能展开**
- 每个集合 / 接口显示 method 彩色徽标
- 顶栏 `+` 真实弹新建弹窗
- 双击树项 inline 改名
- 右键菜单：新建子项 / 重命名 / 上下移 / 删除 / 复制接口
- 删除弹确认框

**暴露嵌套能力的入口**：集合行右键的「新建子集合」；拖拽到某个集合正中间时进入 `into` 落点（落点子树），拖到上下边缘时是 `above` / `below` 的同级重排。面包屑（`项目 › 模块 › 集合 › …`）会逐级显示实际深度。

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
- 服务（baseUrl）：**项目级共享一份服务清单**，每个模块在每个环境下各填各的地址；表头按「模块 × 服务」逐格展开，某一格为空就是该模块在该环境下没有这个服务的地址，不会继承别的模块
- 环境变量：`{{var}}` 实时编辑，随项目创建，可重命名 / 删除
- MCP Server 状态：running 指示 + daemon 实时上报的已注册工具列表（数量动态显示，取决于当前 capability 策略下的可见工具集）
- 主题：dark / light / system 切换，立即生效并持久化
- 请求设置：超时 / 跟随重定向 / 最大响应体积
- 快捷键速查

### 主题
- dark / light / system 三选一
- 切换瞬时生效，刷新保留
- 全局 html class 切换，无白屏闪烁

### 首次启动
- **不注入任何演示数据**。首次启动只建表，项目树是空的，从欢迎页新建第一个项目开始
- `crates/simple-post-core/src/db/fixtures.json` 仅作为 Rust 测试夹具保留，
  编译进生产构建的路径不存在（`#[cfg(test)] pub(crate) mod test_support`）

## 路线图

v0.1.0 beta 已交付：Tauri 桌面应用 + `serve-web` 无头 Web + 独立 `simple-post-mcp` 二进制，共用一套 Rust 核心。下面是计划中的后续阶段，勾选状态以代码为准。

### v0.2

- [x] 鉴权扩展：Bearer / Basic / API Key（含 apikey 的 header/query 注入）
- [ ] 鉴权扩展：OAuth2 简化版
- [ ] 响应历史持久化到 SQLite（当前历史仅在内存中）
- [ ] 导入 Postman v2.1 / OpenAPI 3.0
- [ ] 导出 OpenAPI 3.0
- [x] MCP 写工具：`endpoint_create` / `endpoint_update` / `endpoint_search` / `endpoint_delete`
- [ ] MCP 进程可视化（设置页：PID、状态、tail 日志）

### v0.3

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

> App 启动时同时拉起一个 **stdio MCP Server** 子进程（Claude Desktop / Cursor / Cline 通过 stdio 通信），并通过 `serve-web` 子命令的 `/mcp` 端点暴露 HTTP MCP（外部 AI 客户端、远程调试器）。Server 默认不暴露网络，鉴权可选。**AI 写入默认关闭**：能力策略 `read_only` 是默认值，需要在「AI 写入权限」面板调档才能写入；`Capability` 为全序 `Read < Create < Update < Delete < Send`，`Send`（出站 HTTP）只有 `full_access` 覆盖。项目级 `permissions` 表是第二道闸，两者取严。完整机制见 [`AGENTS.md`](AGENTS.md) 的 *MCP permissions — two gates* 一节。

实际注册 **44 个** MCP 工具（精确值由 `every_registered_tool_has_a_capability` 测试守护）。每个工具在 `REQUIRED_CAPABILITY`（`crates/simple-post-mcp/src/mcp/mod.rs`）中声明一个 `Capability`，总序为 `Read < Create < Update < Delete < Send`；用户的全局策略 `CapabilityPolicy` 决定可见集：

| 策略 | 覆盖的 Capability | 含义 |
|---|---|---|
| `read_only`（默认） | `Read` | 只读观察，不发网络请求、不改状态 |
| `safe_write` | `Read` `Create` `Update` | 允许增删改，但**不含** `Send` |
| `full_access` | 全部（含 `Delete` `Send`） | 允许出站 HTTP 调用与破坏性操作 |

- **Read**：`project_list` / `project_get` / `module_list` / `collection_list` / `endpoint_list` / `endpoint_get` / `endpoint_search` / `base_url_def_list` / `env_list` / `env_get_active` / `api_resolve_*` / `api_validate_endpoint` / `api_parse_response` / `api_diff_responses` / `api_extract_jsonpath`
- **Create**：`project_create` / `module_create` / `collection_create` / `endpoint_create` / `endpoint_duplicate` / `env_create` / `base_url_def_create`
- **Update**：`project_update` / `module_update` / `module_move` / `collection_update` / `collection_move` / `endpoint_update` / `endpoint_move` / `env_update` / `env_set_var` / `env_set_baseurl` / `env_set_active` / `base_url_def_update` / `base_url_def_set_default`
- **Delete**：`project_delete` / `module_delete` / `collection_delete` / `endpoint_delete` / `env_delete` / `base_url_def_delete`
- **Send**（仅 `full_access`）：`api_run_request` / `api_call_endpoint`

> 全局策略之外还有一道**按项目**的权限闸（`simple_post_core::permissions`，`Read < ReadCall < Full`，数据库未显式设置时默认 `ReadCall`，未知工具按 `Full`）。两道闸是「与」关系，取更严格者。

完整列表 + 入参 / 出参 + 权限矩阵见 [`crates/simple-post-mcp/src/mcp/mod.rs`](crates/simple-post-mcp/src/mcp/mod.rs)。同名 HTTP REST 接口的权威来源是 `public_route_specs()`（`crates/simple-post-server/src/web_api/spec.rs`）与 `build_router` 的 `.route()` 注册（`crates/simple-post-server/src/web_api/mod.rs`）；`node scripts/dump-openapi.mjs` 可从该表生成一份机器可读的 OpenAPI 文档。

## 部署到服务器（serve-web shape）

`simple-post serve-web` 起一个无头 REST + SPA 服务，可直接跑在任何能跑 Rust
二进制的机器上。构建产物：

```bash
npm install && npm run build
# 然后运行 cargo build --release，产物为 target/release/simple-post
```

也可以只构建**不含 Tauri** 的 headless 二进制 —— 同样的 `serve-web` / `mcp`
子命令，但不链接 GUI 运行时，因此在 Linux 上不需要 GTK/WebKit，容器镜像用的就是它：

```bash
cargo build --release -p simple-post-server
./target/release/simple-post-server serve-web
```

桌面版 `simple-post` 的 `serve-web` 仍然可用（内部转调同一个实现），`npm run dev:split`
不受影响；两者共用同一份 router 代码，不存在两份行为。

启动与自检：

```bash
./simple-post serve-web
curl http://localhost:4173/api/healthz
# {"status":"ok","version":"…","shape":"server"}
```

接 MCP 客户端：Claude Desktop / Cursor / Cline 的 `mcpServers` 里配
`url=http://host:4173/mcp` + `Authorization: Bearer <密码>`。

安全提示：

- `SIMPLE_POST_WEB_PASSWORD` **必须**设，否则 `/api/*` 和 `/mcp` 全开放（`auth_middleware` 在 password 为空时是 no-op）。
- 健康检查路径 `/api/healthz`、`/mcp/healthz`、`/healthz` 始终免认证，方便探针。静态资源 `/assets/*` 与 SPA 外壳同样免认证——浏览器无法给 `<script type="module">` 挂 `Authorization` 头，若对它们鉴权，带密码的部署会直接白屏。
- **出站 URL 策略**：`serve-web` 会代替浏览器发请求，所以有两条出口，权限不同。
  - `POST /api/nodes/:id/send`（**UI 的 Send 按钮走这条**）：只接受 endpoint id + 相对路径，base URL、路径、`{{var}}`、鉴权全部由服务端按数据库里的配置拼装。**浏览器无法指定目标地址**，因此这条路由没有 SSRF 面；传入绝对 URL 会被 400 拒绝。
  - `POST /api/http/send`：**原始 URL 逃生舱**，调用方可以指定任意地址。仅 `http` / `https` 允许（`file:` / `gopher:` 等一律拒绝）。默认**不**拦截私网地址——桌面端用户常常就是要打 `localhost:3000`，默认拦会直接砸掉产品的主要用途。
  - 公网 / 共享部署请设 `SIMPLE_POST_BLOCK_PRIVATE_NET=1`，额外拒绝回环、私网、链路本地（含云厂商 metadata `169.254.169.254`）与 CGNAT 网段。
- 反代部署（nginx / Caddy）时设 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`，`build_router` 会把整个 router（含 `/api/*` 与 `/mcp`）nest 在 `/sp` 下；nginx 侧**原样转发、不要剥离前缀**。服务端会按该前缀改写 SPA 外壳的资源引用，并向渲染器注入 `window.__SIMPLE_POST_BASE__`，前端据此拼 `/sp/api/...`——不需要 `<base href>`，也不需要 nginx rewrite。
- 健康检查同理：设了前缀后 `/healthz` 也移到 `/sp/healthz`。容器镜像里的 healthcheck 脚本会读同一个环境变量去探前缀路径，所以两种模式都是 healthy。
- 数据持久化：把数据目录挂到持久卷即可。默认位置是 `SIMPLE_POST_DATA_DIR` 或 `%APPDATA%/simple-post`。SQLite 在 WAL 模式下也能安全落盘。

## 决策记录

- **技术栈**：Tauri 2 + React 18 + TypeScript（前端） + Cargo workspace（后端：`simple-post` Tauri shell / `simple-post-core` 共享层 / `simple-post-mcp` 独立 MCP 服务）
- **存储**：SQLite（`rusqlite` + `r2d2`，整个 workspace 通过 `simple-post-core::db` 共享连接池；WAL + `foreign_keys=ON` + `busy_timeout=3000`）
- **MCP 模式**：stdio 子进程桥（本地 AI 客户端）+ HTTP `POST /mcp`（`serve-web` 子命令，支持远程 / Bearer 鉴权 / 反代 base path）
- **UI 风格**：暗色为主，shadcn 变量层，可切亮色
- **数据位置**：三种分发形态（Tauri 桌面 / `serve-web` 无头 Web / 独立 MCP 二进制）共用一套 Rust 核心，`SIMPLE_POST_DATA_DIR`（或 `%APPDATA%/simple-post`）是唯一数据位置。

## 容器部署（Docker）

镜像里只有 `simple-post-server`（**Tauri-free** 的 serve-web 二进制）+ SPA 静态产物 + 一个数据目录。不装 GTK / WebKit / X11，也不装 OpenSSL：SQLite 走 `rusqlite` 的 `bundled` 静态编译，TLS 走 rustls 的内置根证书集。这是桌面版在 Linux 上装不装得上 GUI 工具链的区别，也是镜像只有几十 MB 的原因。

```bash
docker build -t simple-post:local .
docker compose up -d          # → http://localhost:4173
```

`docker-compose.yml` 不写 `build:`（`docker stack deploy` 会忽略）也不写顶层 `version:`（会被直接拒绝），同一份文件形状两种编排器都吃。镜像内路径固定：SPA 在 `/app/spa`，数据在 `/data`。

| 变量 | 默认 | 说明 |
|---|---|---|
| `SP_WEB_PASSWORD` | 空 | 映射到 `SIMPLE_POST_WEB_PASSWORD`：`/api/*` + `/mcp` 的 Bearer 口令，**留空即完全不鉴权** |
| `SP_WEB_BIND` | `4173` | 宿主机映射端口（容器内固定 4173） |
| `SP_RUST_LOG` | `info` | 映射到 `RUST_LOG`：tracing 日志过滤 |
| `SIMPLE_POST_PUBLIC_BASE_PATH` | 空 | 反代挂在 `https://host/sp/` 时用，**必须以 `/` 开头**；不要写成空值 |
| `SIMPLE_POST_BLOCK_PRIVATE_NET` | `0` | 置 `1` 后 `/api/http/send` 拒绝回环 / 私网 / 链路本地（含云 metadata） |

`SP_*` 是 compose 的插值变量名（写在 `.env`，见仓库里的 `.env.example`），由 yml 转成 binary 直接认识的 `SIMPLE_POST_*`；后两项是 binary 自己读的环境变量，可以直接 `docker run -e` 传。

> ⚠️ **口令不是可选的保险，是唯一的门**。容器监听 `0.0.0.0`，`SIMPLE_POST_WEB_PASSWORD` 留空时 `/api/*` 与 `/mcp` 对整个网络裸奔——其中 `POST /api/http/send` 是任意 URL 出口。镜像自带的 HEALTHCHECK 打的是 `/healthz`，该路径永远免认证，所以探针可安全使用；但它保护不了 `/api/*`。

Swarm：

```bash
# 1) 指定一个节点持有数据：打标签 + 在那个节点上建卷
docker node update --label-add simple_post_data=true <node-id>
docker volume create --driver local simple-post-data   # stack deploy 不会自动建命名卷

# 2) 用带 registry 前缀的 tag 构建并推送（多节点靠它拉取）
docker build -t <registry>/simple-post:0.1.1 .
docker push   <registry>/simple-post:0.1.1

# 3) 部署时告诉 stack 用哪个镜像
SP_IMAGE=<registry>/simple-post:0.1.1 docker stack deploy -c docker-swarm.yml simple-post
docker stack services simple-post                      # 看 rollout 状态
```

`docker-swarm.yml` 里有两条**不要动**的约束，它们挡的是两种不同的失败：

- `replicas: 1` —— 数据落在一个 WAL 模式的 SQLite 文件里，WAL 依赖同机共享内存，多个写进程会互相 `SQLITE_BUSY`，`mcp.port` 也只能有一个属主。要高可用得先换掉存储，不是加副本。
- `placement` —— 卷是 `local` 驱动，只存在于一个节点。**`replicas: 1` 挡不住这个问题**：节点故障或更新后的重调度会把唯一的任务放到别的节点，Docker 在那儿建一个空卷，服务照样 healthy 起来，但你的项目全没了，日志里也不会有异常。把 `placement` 注释掉部署会直接失败（"no suitable node"），这正是想要的失败方式。

另外 `image` 写成 `${SP_IMAGE:-simple-post:local}`：Swarm 是按 spec 里写的名字在**每个节点**上拉镜像，裸的本地 tag 只存在于一台机器上，所以多节点必须给 `SP_IMAGE` 指向你真正 push 上去的那个 tag。
