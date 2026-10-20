# Simple Post

一款面向 Windows 10/11 的桌面 API 调试 / 管理工具（Postman 类），核心差异化是**自带 MCP Server**——Claude Desktop、Cursor、Cline 等外部 AI 客户端可以直接列出、检索、调用、改写这里的接口定义。

> 当前阶段：**v0.1.2**（版本号以根 `package.json` 与 `Cargo.toml` 为准）。数据持久化在本地 SQLite。

---

## 项目说明

### 定位

本地优先的 API 客户端：接口树、请求构造、响应查看都在一个桌面应用里完成，不依赖任何 SaaS 后端。同时它是一个 MCP server，让 AI 客户端在**同一份数据**上工作，而不是让 AI 去猜接口长什么样。

### 三种分发形态

同一套 Rust 核心，三种入口：

| 形态 | 命令 | 说明 |
|---|---|---|
| 桌面应用 | `simple-post`（默认） | Tauri GUI，`tauri:build` 打 NSIS / dmg / deb |
| 无头 Web | `simple-post serve-web` | REST + SPA，默认 4173 端口，可挂反向代理、可进容器 |
| MCP 桥 | `simple-post mcp-proxy` | stdio ↔ daemon 桥，供 Claude Desktop / Cursor / Cline 挂载 |

其中 `serve-web` 还有一条**不含 Tauri** 的构建路径：`cargo build --release -p simple-post-server` 产出 `simple-post-server`，具备同样的 `serve-web` / `mcp` 子命令但不链接 GUI 运行时。容器镜像用的就是它——这是镜像只有几十 MB、且不需要 GTK / WebKit 的原因（`src-tauri/Cargo.toml` 的 `gui` feature 默认关闭）。

### 架构一句话

前端（React 18 + Vite 5 + Zustand 5）通过 Tauri IPC **或** `fetch('/api/…')` 两条等价通道调用同一套 Rust 命令；Rust 侧是一个 Cargo workspace，SQLite（`rusqlite`，WAL）作为唯一存储。

| 部件 | 路径 | 职责 |
|---|---|---|
| Tauri 外壳 + CLI | `src-tauri/` | 桌面 GUI、`#[tauri::command]` 表面、clap 子命令分发 |
| 服务宿主 | `crates/simple-post-server/` | `serve-web` 的 REST/SPA、daemon 监管、端口注册、Bearer 鉴权、wire DTO |
| 核心库 | `crates/simple-post-core/` | 领域模型、SQLite（`db/`）、HTTP 执行器、权限、MCP 指标、错误类型。无 Tauri / axum 依赖 |
| MCP 服务 | `crates/simple-post-mcp/` | JSON-RPC 2.0 派发（44 个工具）、axum HTTP daemon、stdio 桥 |
| 前端 | `src/` | React + Vite + Zustand + Tailwind + CodeMirror 6 |

依赖方向是单向的：`simple-post (Tauri) → simple-post-server → simple-post-core`。

### 跑起来

```bash
npm install
npm run dev            # Vite dev server → http://127.0.0.1:5173（已绑定 0.0.0.0，局域网可访问）
npm run dev:split      # Vite + cargo run -- serve-web(4173)，前后端联调用这个
npm run build          # tsc -b && vite build → dist/
npm run check          # 一键本地门禁：Prettier → ESLint → tsc → 单测 → cargo fmt → clippy
```

Rust 侧：

```bash
cargo build --release                      # 桌面 + 全部子命令
cargo build --release -p simple-post-server # 仅无头 server（容器用）
cargo test --workspace                     # 全部 Rust 测试
```

> `npm run check` 与 CI 跑的是同一套检查。推 PR 前本地跑一次即可。

### 部署路径

#### 无头 Web

```bash
./target/release/simple-post-server serve-web
curl http://localhost:4173/api/healthz
# {"status":"ok","version":"…","shape":"server"}
```

MCP 客户端接到 `http://host:4173/mcp`，带 `Authorization: Bearer <密码>`。

数据落在 `SIMPLE_POST_DATA_DIR`（未设时为 `%APPDATA%/simple-post`），把它挂到持久卷即可。SQLite 跑在 WAL 模式下，能安全落盘。

#### 反向代理

要让服务挂在 `https://host/sp/` 下，用 `--public-base-path /sp`（或 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp`）启动，然后**原样转发，不要剥离前缀**——前缀由服务端自己处理。

nginx：

```nginx
location /sp/ {
    # 结尾不带 URI 部分 = 原样转发；写成 /sp/ 或用 rewrite strip 都会把前缀剥掉
    proxy_pass http://127.0.0.1:4173;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

Caddy：

```
sp.example.com {
    reverse_proxy 127.0.0.1:4173   # 保留原始路径；不要用 handle_path（它会剥掉前缀）
}
```

**前缀只能由一处处理。** 服务端会把整个 router（`/api/*`、`/mcp` 与三个健康探针）一起 `nest` 到 `/sp` 下，并按实际前缀改写 SPA 外壳里的资源引用、注入 `window.__SIMPLE_POST_BASE__`。nginx 若再剥一次，两边就对不上了。设了前缀后健康检查也移到 `/sp/healthz`，探针要跟着改。

#### Docker

```bash
docker build -t simple-post:local .
docker compose up -d          # → http://localhost:4173
```

镜像里只有 `simple-post-server`（无 Tauri）+ SPA 静态产物 + 一个数据目录。不装 GTK / WebKit / X11，也不装 OpenSSL：SQLite 走 `rusqlite` 的 `bundled` 静态编译，TLS 走 rustls 内置根证书集。镜像内 SPA 在 `/app/spa`，数据在 `/data`。

| 变量 | 默认 | 说明 |
|---|---|---|
| `SP_WEB_PASSWORD` | 空 | 映射到 `SIMPLE_POST_WEB_PASSWORD`：`/api/*` + `/mcp` 的 Bearer 口令，**留空即完全不鉴权** |
| `SP_WEB_BIND` | `4173` | 宿主机映射端口（容器内固定 4173） |
| `SP_RUST_LOG` | `info` | 映射到 `RUST_LOG` |
| `SIMPLE_POST_PUBLIC_BASE_PATH` | 空 | 反代前缀，**必须以 `/` 开头** |
| `SIMPLE_POST_BLOCK_PRIVATE_NET` | `0` | 置 `1` 后 `/api/http/send` 拒绝回环 / 私网 / 链路本地（含云 metadata） |

`SP_*` 是 compose 的插值变量名（写在 `.env`，见 `.env.example`），由 yml 转成 binary 认识的 `SIMPLE_POST_*`。

Swarm 部署时有两个**不要动**的约束：

- `replicas: 1` —— 数据落在一个 WAL 模式的 SQLite 文件里，WAL 依赖同机共享内存，多写进程会互相 `SQLITE_BUSY`，`mcp.port` 也只能有一个属主。
- `placement` —— 命名卷是 `local` 驱动，只存在于一个节点。`replicas: 1` **挡不住**这个问题：更新后重调度会把任务放到别的节点，Docker 在那儿建一个空卷，服务照样 healthy 起来，但数据全没了。把 `placement` 注释掉部署会直接失败（"no suitable node"），这正是想要的失败方式。

镜像 tag 写 `${SP_IMAGE:-simple-post:local}`，因为 Swarm 按 spec 里写的名字在**每个节点**上拉镜像，多节点必须用 `SP_IMAGE` 指向你真正 `docker push` 上去的 tag。

### 安全约束

这几条是部署时最容易踩的，改配置前先读一遍。

- **`SIMPLE_POST_WEB_PASSWORD` 必须设。** 为空时 `auth_middleware` 是 no-op，`/api/*` 和 `/mcp` 对整个网络裸奔。容器监听 `0.0.0.0`，这不是"可以不配的保险"，是唯一的门。
- 健康检查 `/api/healthz`、`/mcp/healthz`、`/healthz` 始终免认证（方便探针）；`/assets/*` 与 SPA 外壳同样免认证——浏览器无法给 `<script type="module">` 挂 `Authorization` 头，对它们鉴权会让带密码的部署直接白屏。
- **两条出站通道，权限不同**：
  - `POST /api/nodes/:id/send`（UI 的 Send 按钮走这条）只接受 endpoint id + 相对路径，base URL、路径、`{{var}}`、鉴权全部由服务端按数据库配置拼装。**浏览器无法指定目标地址**，传入绝对 URL 会被 400 拒绝，因此没有 SSRF 面。
  - `POST /api/http/send` 是**原始 URL 逃生舱**，调用方可以指定任意地址（仅允许 `http` / `https`）。默认**不**拦截私网地址——桌面端用户常常就是要打 `localhost:3000`，默认拦会砸掉产品的主要用途。公网 / 共享部署请设 `SIMPLE_POST_BLOCK_PRIVATE_NET=1`，额外拒绝回环、私网、链路本地（含云厂商 metadata `169.254.169.254`）与 CGNAT 网段。

---

## 项目亮点

### 自带 MCP Server，AI 直接读写接口

44 个 MCP 工具（由 `crates/simple-post-mcp/src/mcp/mod.rs` 的 `every_registered_tool_has_a_capability` 测试守护数量与权限行的对应关系）。AI 客户端能列项目、搜接口、查环境变量、发起调用、创建和改写接口定义——不是只读，是真的能改。

两种接法：stdio 子进程桥（`mcp-proxy`，本地客户端用）或 HTTP `POST /mcp`（`serve-web` 暴露，远程 / 远程调试用）。

> 端口不要写死。`simple-post mcp` 默认 `--port=0`，OS 分配的端口每次重启都变，写死的 `http://127.0.0.1:<port>/mcp` 会在下次重启时静默失效。**没有活动部件的接法是：stdio 桥 → 固定 4173 端口的 `serve-web`**，客户端配置里只有 `command` + `args`，没有端口可漂移。

### 一套 Rust 核心，三种形态

桌面应用、无头 Web、容器镜像跑的是同一份实现。桌面版的 `serve-web` 依然可用（内部转调同一个 router），不存在"两份行为"。新增后端命令时前端不需要区分自己在哪个形态里——这也是 dual-transport 设计的前提。

### 递归嵌套的集合树

Project → Module → Collection（**可无限递归嵌套**）→ Endpoint。集合下面还能建集合，面包屑逐级显示真实深度。落库时模块、集合、接口在同一张 `nodes` 表里用 `type` 区分，父子关系由 `parent_id` 表达。

层级的合法性是**被强制的契约**而不是约定：模块只能在顶层，接口可以挂在集合下、也可以直接挂在模块下（单个接口不需要先建一个一次性集合），集合可以挂在模块或另一个集合下。SQLite trigger 与应用层检查双重把关，拖拽落点也遵守同一矩阵。

### 服务地址按「模块 × 环境」逐格隔离

服务清单是**项目级**的（所以两个模块不可能有同名服务，每个模块都看得到全部服务），但每个服务的**地址**是逐格独立的：`环境 × 模块 × 服务` 三个维度各自一格。

某模块没填这一格，就是它在这个环境下没有这个服务的地址——**不继承任何东西**，既不继承项目默认，也不继承别的模块。`environments.module_base_urls_json` 是 `service_id → module_id → url` 的嵌套结构，这正是为了消灭"看起来生效、实际串到别的模块去了"这类静默错误。

### 两道权限闸，都 fail-closed

MCP 写操作的授权要同时过两道闸，任何一道缺失都是**拒绝**而不是放行：

- **全局能力闸**：用户选的策略（`read_only` / `safe_write` / `full_access`）决定可见工具集。`Capability` 是全序 `Read < Create < Update < Delete < Send`，其中 **`Send`（出站 HTTP）只有 `full_access` 覆盖**——"允许写"不等于"允许随便发请求"。
- **项目权限闸**：按项目在 `permissions` 表里授权，默认 `ReadCall`。未分类的工具按最严格处理。

两道闸的关系是"与"，取更严格者。默认策略是 `read_only`，AI 写入默认关闭，需要用户在设置里显式调档。

---

## 已实现

每条都能指到具体代码。

**数据与资源树**

- Project → Module → Collection（可递归，深度不限）→ Endpoint，外加 Environment / Response（`crates/simple-post-core/src/domain.rs`）
- 整棵树存在一张 `nodes` 表，用 `type` 区分；所属项目以 `root_id` 落在每一行上，一次索引读即可回答"这是哪个项目的"（`crates/simple-post-core/src/db/schema.sql`、`db/nodes.rs`）
- 接口的请求配置（method / url / auth / body / headers / params / tags）单独放 `endpoint_details` 侧表——这些字段对模块和集合没有意义（`db/endpoint_details.rs`）
- 层级合法性由 SQLite trigger + 应用层检查双重强制（`ResourceNodeType::may_under`、`db::nodes::move_many`）
- 完整 CRUD + 复制接口 + 级联清理（级联由 schema 外键保证，客户端只提交要删的 id）+ 模块 / 集合 / 接口上下移排序（`db/nodes.rs`）
- **响应历史持久化到 SQLite**（`db/response_history.rs`）：每个接口最多 10 条，超出由 `append` 裁掉最旧的；响应体有大小上限，被截断时记 `body_truncated`，查看器据此明说而不是把半个文档当完整的显示

**桌面与界面**

- 项目 / 环境切换、全局搜索（`Ctrl+K`）、MCP Server 状态指示器、设置入口（`src/features/top-bar/`）
- 侧边树：递归渲染集合层、method 彩色徽标、双击改名、右键菜单（新建 / 重命名 / 上下移 / 删除 / 复制）、删除确认（`src/features/project-tree/`）
- 拖拽三种落点：集合正中间 = 移进该子树，上 / 下边缘 = 同级前后插入（`src/lib/tree-index.ts`）
- 请求测试器：method / URL（`{{var}}` 高亮 + 变量选择浮层）/ Params / Headers / Body / Auth / Docs / Tests，CodeMirror 6 JSON 编辑器带实时校验，Markdown 分屏预览，Auth 支持 None / Bearer / Basic / API Key，Tag chips（`src/features/http-tester/`）
- 复制为 cURL、保存为接口示例（`src/lib/http-build.ts`）
- 响应区：状态码 / 耗时 / 大小、Pretty / Raw / Headers 三视图、复制与下载、历史折叠条可点击重放（`src/features/response-viewer/`）
- `Ctrl+K` 命令面板：跨项目 / 模块 / 集合 / 接口模糊搜索 + 内置命令（切换环境、切换主题、打开设置、新建项目、复制 URL）
- 主题 dark / light / system 三选一，`<html>` 上切 class，刷新保留且无白屏闪烁（`src/styles/globals.css`）

**请求脚本（沙箱隔离）**

- 端点与集合都能配**前置脚本**（pre-request）与**测试脚本**（test）：端点的住在 `endpoint_details` 行里，集合的住在 `node_scripts` 表（`db/endpoint_details.rs`、`db/node_scripts.rs`）
- 前置脚本在变量替换之后、请求发出之前跑，可改写 URL / headers / body；**发出去的就是改写后的那个请求**，请求日志里看到的也是它
- 测试脚本在响应回来之后跑，可断言并写回环境变量（`pm.environment.set`），下一个请求立刻能读到
- 脚本顺序对齐 Postman 官方文档：集合 → 嵌套集合 → 接口，前置与测试同序
- 脚本在**嵌入式 QuickJS 沙箱**里跑（`simple-post-core/src/script/`）：宿主能力只有一段固定 JS 前导给出的那几样（`pm.request` / `pm.environment` / `pm.response` / `pm.test` / `pm.expect` / `console.log`），**够不到文件、网络与进程**
- 硬限额：每阶段 1000 ms 墙钟（超时真的打断死循环，不等它跑完）、32 MB 内存、256 KB 栈；日志与断言条数有上限
- 脚本失败**不影响这次请求的成败**：失败脚本的改动被丢弃、变量写入不落库，失败信息在响应面板的「脚本」页里单独呈现
- 整条执行序列（装配链 → 前置脚本 → 发出 → 测试脚本 → 变量回写）只在 `core::script::send_with_scripts` 一处实现，桌面端与 MCP 端共用，故两侧不可能分叉


**设置**

- 服务（baseUrl）：项目级共享清单，每个模块每个环境各填各的地址，表头按「模块 × 服务」逐格展开，空格即无地址、不继承
- 环境变量 `{{var}}` 实时编辑，随项目创建，可重命名 / 删除
- MCP Server 状态：运行指示 + daemon 上报的已注册工具列表（数量随当前策略变化）+ daemon 日志 tail（最近 500 行，可折叠）
- 主题、请求设置（超时 / 跟随重定向 / 最大响应体积）、快捷键速查（`src/features/settings/`）

**关于数据**

- 首次启动**不注入任何演示数据**，只建表，项目树是空的。`crates/simple-post-core/src/db/fixtures.json` 仅是 Rust 测试夹具，只能通过 `#[cfg(test)] pub(crate) mod test_support` 触达，生产构建里不存在这个路径。
- 偏好（主题 / 请求设置）持久化在 localStorage，业务数据在 SQLite。

---

## 路线图

v0.1.x 已交付：Tauri 桌面应用 + `serve-web` 无头 Web + 独立 MCP 二进制，共用一套 Rust 核心。勾选状态以对应 issue 的状态为准。

### v0.2

已全部交付。

- [x] 鉴权扩展：Bearer / Basic / API Key（含 apikey 的 header / query 注入）
- [x] 鉴权扩展：OAuth2 简化版（#2）
- [x] 响应历史持久化到 SQLite（#1）
- [x] MCP 进程可视化补齐：daemon 日志 tail（PID / 状态已实现）（#6）
- [x] 导入 Postman v2.1（#3）
- [x] 导入 OpenAPI 3.0 文档（#4）
- [x] 导出 OpenAPI 3.0（#5）
- [x] MCP 写工具：`endpoint_create` / `endpoint_update` / `endpoint_search` / `endpoint_delete`
- [x] 仓库治理：说明内容 / CI 提速与镜像发布 / 协作规范 / 注释中文化（#23 #24 #25 #26）

### v0.3

- [ ] AI 文档生成（端点 → Markdown）（#9）
- [ ] AI 测试用例生成（schema → 测试）（#10）
- [x] 请求脚本（pre-request / test 脚本，沙箱隔离）（#7）
- [ ] 集合批量运行 + 简易报告（#8）
- [ ] 团队协作（Git 同步 JSON 工作区，可选）（#11）

### v1.0

- [ ] WebSocket / SSE / GraphQL 协议支持（#12）
- [ ] Mock Server（从集合生成 mock 服务 + 故障注入）（#13）
- [ ] 性能压测（简易，与 `npm run perf:bench` 的开发期基准区分）（#14）
- [ ] 插件系统：第三方 MCP 工具注册（须保住双闸 fail-closed）（#15）

---

## 决策记录

- **技术栈**：Tauri 2 + React 18 + TypeScript（前端） + Cargo workspace（后端）。四个 crate：
  - `simple-post`（`src-tauri/`）—— Tauri 外壳与 CLI
  - `simple-post-server`（`crates/simple-post-server/`）—— `serve-web` 宿主，无 Tauri 依赖，容器镜像构建的就是它
  - `simple-post-core`（`crates/simple-post-core/`）—— 领域模型与 SQLite，无头
  - `simple-post-mcp`（`crates/simple-post-mcp/`）—— MCP 工具与 daemon

  依赖单向：`simple-post → simple-post-server → simple-post-core`。`src-tauri` 无条件链接 `tauri`，会把 GTK / WebKit 拖进 Linux 构建——服务半个进程拆出去正是为了这个。
- **存储**：SQLite（`rusqlite` + `r2d2`），整个 workspace 通过 `simple-post-core::db` 共享连接池。WAL + `foreign_keys=ON` + `busy_timeout=3000`
- **MCP 模式**：stdio 子进程桥（本地客户端）+ HTTP `POST /mcp`（`serve-web` 暴露，支持 Bearer 鉴权与反代 base path）
- **UI 风格**：语义 token 层，双主题作用域（`globals.css` 的 `:root` 与 `.dark`），Tailwind 变量消费；dark / light / system 三态，切换作用于 `<html>` 的 class
- **数据位置**：三种分发形态共用一套 Rust 核心与同一个数据目录（`SIMPLE_POST_DATA_DIR`，未设时 `%APPDATA%/simple-post`）
- **请求脚本的沙箱**：嵌入式 QuickJS（`rquickjs`），不是独立受限进程、也不是纯 Rust 的 boa。
  - 只有它同时给出**真墙钟打断**（`set_interrupt_handler` → 抛不可捕获异常）、**内存上限**（`set_memory_limit`）与**栈上限**（`set_max_stack_size`）。issue 要求「超时要能中断执行而不是等它自己跑完」，boa 的循环计数兜得住 `while(true)` 却兜不住无循环的长计算，也没有内存上限
  - 不新增工具链要求：Windows 端已因 Tauri 需要 MSVC，Docker 构建阶段（`rust:bookworm`）已带 gcc，rquickjs 的预生成绑定覆盖 windows-msvc / linux-gnu / aarch64-linux-gnu
  - 宿主能力只有一段固定 JS 前导给出的那几样，故「脚本够不到文件 / 网络 / 进程」不靠逐个禁用，而靠根本没有那条路
  - 代价：workspace 的 MSRV 从 1.77.2 抬到 1.87（rquickjs 的要求；CI 用 `stable`，无 MSRV 门禁 job）

---

## 贡献与工具链

开发规范、架构约定、高风险点（schema 迁移、MCP 双闸、dual-transport 五处协同）见 [`AGENTS.md`](AGENTS.md)。协作流程见 [`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md)。

```bash
npm run check   # 提交前跑这一条即可覆盖全部本地门禁
```

MIT 许可。