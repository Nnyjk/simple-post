# Repository Guidelines

## Project Overview

`simple-post` 是 Windows 优先的桌面 API 调试工具（Postman 类），差异化在**内置 MCP server**：外部 AI 客户端经 48 个 MCP 工具列出、搜、发、改接口定义。技术栈 **Tauri 2 + React 19 + TypeScript** 压在 **Cargo workspace** 上（`rusqlite` 存 SQLite，`reqwest` 发请求，`axum` 起服务），一份 Rust 核心交付三种形态：Tauri 桌面应用、无头 `serve-web`（REST + SPA）与 `simple-post-mcp` 打桥。桥**只以编译产物分发**（不在 crates.io、没有 npm 包、release 只带桌面安装包），只能从源码建：`cargo install --path crates/simple-post-mcp`。

文档一半中文（`README.md`、三份 crate README），代码与标识符英文。`README.md` 是给人看的产品文档，本文件是 **agent 的运行契约**：只有「不知道就会写错」的事实才留在这里。

## 按需文档

本文件只放**跨改动**的硬约束。动到某个域之前先读对应文件，那里的原文更完整（逐字保留，不重复这里的摘要）：

| 域 | 文件 |
|---|---|
| 服务端 / Tauri 拆分 / 守护进程 / REST 路由与 OpenAPI 表 / 反代基础路径 | `docs/agent/server-architecture.md` |
| 交换格式与导入导出 | `docs/agent/interchange.md` |
| 渲染层：启动、设置面板、标签条、modal、Vite/GSAP/chunk、主题 token | `docs/agent/renderer.md` |
| 请求装配 / TLS / 响应历史 / 批量运行 | `docs/agent/request-assembly.md` |
| 动态变量（定义、取数、缓存、面板） | `docs/agent/dynamic-variables.md` |
| SSE / WebSocket / GraphQL 与 MCP 的协议边界 | `docs/agent/transports.md` |
| MCP 守护进程、工具契约、`tools/list` 预算 | `docs/agent/mcp-server.md` |
| CI、`npm run release`、updater、图标与安装路径 | `docs/agent/release.md` |
| 测试与质量门 | `docs/agent/testing.md` |
| 源码注释规则与读源码的回归哨兵 | `docs/agent/comments-and-traps.md` |
| 目录与关键文件清单 | `docs/agent/directories.md` |

## Architecture & Data Flow

六个运行时部件：

| Piece | Path | Role |
|---|---|---|
| Tauri shell + CLI | `src-tauri/` | 桌面 GUI 与 `#[tauri::command]` 面（`commands/`）；其余全部从 server crate 再导出，故 `crate::daemon_supervisor` / `crate::web_api` 仍在这里解析 |
| Server host | `crates/simple-post-server/` | `serve-web` 的 REST/SPA、守护进程监管、端口登记、bearer 鉴权、线上 DTO。**默认不含 Tauri** —— 容器镜像建的就是它 |
| Core library | `crates/simple-post-core/` | 域模型、SQLite（`db/`）、`http_executor`、`permissions`、`error`，以及 `dynamic` / `oauth2` / `path_params` / `script` / `spec` / `template` / `tls` / `url` 模块；无 Tauri、无 axum |
| Interchange | `crates/simple-post-interchange/` | 外来格式 ↔ `domain`。**`simple-post-mcp` 刻意不依赖它**，故 MCP 二进制不必背上 YAML 解析器 |
| MCP server | `crates/simple-post-mcp/` | JSON-RPC 2.0 dispatch（48 工具）、axum HTTP daemon、stdio 桥 |
| Frontend | `src/` | React 19 + Vite 8 + Zustand 5 + Tailwind 4 + CodeMirror 6 |

依赖方向只有一条：`simple-post (Tauri) → simple-post-server → simple-post-mcp → simple-post-core`（server 转发 mcp crate 的 spec / dynamic 操作，故 mcp 夹在中间）。`simple-post-server` 保持 Tauri-free 是 Dockerfile 运行阶段不装任何包的前提。

### 数据层

- **`SCHEMA_VERSION` 一涨就换库，没有迁移路径。** `db::schema::ensure_current_schema` 读 `PRAGMA user_version`：`found < SCHEMA_VERSION` 时**删掉** `store.db`（连 `-wal` / `-shm`）再由 `db::migrate` 重建；`found > SCHEMA_VERSION` 时一个字节都不删，返回 `AppError::Validation`。故 `db::migrate` 只服务增量那条路：`db/schema.rs` 的 `ADDED_COLUMNS` 里的列追加到既有文件、不涨版本号。**规则一：无法表达成「追加一个带默认值的可空列」的改动就是 bump，而 bump 会删库。规则二：存在但没有读者的表在打开时被 drop**（如 `request_settings` 那张**表**——单行、`CHECK id=1`、三列；旋钮住在 `preferences` 表的同名**行**里）——别把它抄回 `schema.sql`，那会产出一张在同一趟里被建又被删的表。`DROP` 一列要重建表，重建表就是 bump。
- **DB 访问。** 读 → `db::with_conn`；写 → `db::with_tx`（`BEGIN IMMEDIATE`，`SQLITE_BUSY` 重试 3 次 / 50 ms）。WAL + `foreign_keys=ON` + `busy_timeout=3000`。复杂字段（`KeyValue[]`、`RequestBody`、`AuthConfig`、`tags`）存 `*_json` 文本列。新实体的 SQL 放 `crates/simple-post-core/src/db/<entity>.rs`。`with_tx` 闭包拿不到第二个连接（`BEGIN IMMEDIATE` 会自死锁），故同事务的多步写要拆成接受 `&Connection` 的 `*_in_tx` 函数。
- **Partial updates 一律双 Option。** Patch 类型（`ResourceNodePatch`、`EndpointDetailPatch`、`ProjectPatch`、`EnvironmentPatch`…）用 `#[serde(default, deserialize_with = "double_option")] Option<Option<T>>`：键缺席 = 不动，JSON `null` = 清除，值 = 写入。`ResourceNodePatch::parent_id` 的 `Some(None)` 是把节点提到顶层。节点与端点详情的 patch 都带 `deny_unknown_fields`（字段名写错要报错，不能静默丢掉——写 `sort_order` 而线格式是 `sortOrder` 时，从前返回 200 且什么都没改），且**接受但不写入**资源自己的身份字段（`id` / `rootId` / `kind` / `createdAt` / `updatedAt`、`nodeId`），它们带 `skip_serializing`，不进 `is_empty()`。
- **Search 要转义 `%` 与 `_`。** `db::nodes::search` 与 `db::nodes::search_paths` 用 `like_pattern` 造模式并带 `ESCAPE '!'`；新的 `LIKE` 查询用同一个转义字符，否则搜 `50%` 会匹配每一行、`a_c` 会匹配 `abc`。
- **序列化。** 域类型全部 `#[serde(rename_all = "camelCase")]`，TS 对应物在 `src/types/domain.ts`；加字段两边一起改。
- **绝不用毫秒时钟造 id。** `format!("mod_{now}")` / `format!("col_{now}")` / `format!("proj_{now}")` / `now_ms().to_string()` 一类在同一毫秒内撞主键（批量创建撞 `nodes.id` / `environments.id`，第二条报约束错），批量创建第二条就失败。`domain::uid()` 是唯一的 id 来源，`ep_` / `mod_` / `col_` / `proj_` / `env_` 前缀叠在它上面。
- **`preferences.value` 存的是 JSON 编码后的值，不是裸字符串。** `db::preferences::set` 走 `serde_json::to_string`，读者走 `from_str`；写 `full_access` 而不是 `"full_access"` 会解析失败，而 `mcp_policy` 把不认的值当成 `ReadOnly` 且不报错。
- 错误形状见下：Rust 返回 `Result<T, AppError>`（`crates/simple-post-core/src/error.rs`），线上恒为 `{ code: <snake_case>, message }`；`code` 是契约，`message` 给人看。前端不为网络失败抛异常——`ApiResponse.status === 0` 表示传输层错误。

### 资源树

模块、集合与接口同住**一张 `nodes` 表**，靠 `type` 区分；层级是 `parent_id`，所属项目反范式化到每行的 `root_id`（store 的过滤条件，也是 MCP 权限门的键）。接口的请求配置是另一行 `endpoint_details`（按节点 id 关联）。逻辑形状：项目 → 模块 → 集合（可递归）→ 接口，接口也可直接挂在模块下。

父/子关系是**强制契约**：`ResourceNodeType::may_under` 规定模块只能在顶层、接口可挂集合或模块、集合可挂模块或另一集合；一条 SQLite trigger、一层应用校验（`db::nodes::move_many` 拒绝其他组合）与前端 `moveNodes` 守卫照同一张表判。这张表由 `fixtures/placement-parity.json` 钉住（Rust `may_under` / TS `src/lib/node-placement.ts`）。

### 双传输（加后端命令必须走完五步）

任何新后端命令需要**五处协同改动**，少一处就静默只有一侧能用：

1. `src-tauri/src/commands/<resource>.rs` 的处理函数（`commands/mod.rs` 用**glob** `pub use` 再导出——列清单会丢掉 `#[macro_export]` 的 `__cmd__<name>`，`generate_handler!` 报 E0433）
2. `src-tauri/src/lib.rs` 里注册
3. `src/lib/tauri-commands/commands.ts` 的 `COMMANDS` 常量
4. `crates/simple-post-server/src/web_api/` 的路由（`.route()` 注册在 `web_api/router.rs::build_router`，处理函数在 `web_api/routes/<resource>.rs`）
5. 浏览器可调时再加 `WEB_ROUTES` 条目（`src/lib/tauri-commands/web-routes.ts`）

两侧都要的 DTO 放 `crates/simple-post-server/src/dto.rs`，**不放** `commands/`（否则 server 又与 `tauri::State` 绑上）。`src-tauri/src/commands/dto.rs` 只是 `pub use` 的兼容面。

**Tauri 命令的参数就是 REST body，逐字段摊平。** `#[tauri::command]` 按参数名绑定（宏会先过 `to_lower_camel_case`），从不反序列化 payload；REST 侧把整个对象喂给 `Json<SomeOpts>`。故参数列表要把结构体字段一个个写出来——写成 `opts: ImportPreviewOpts` 能编译、Rust 测试全绿、web 形状完美，而每一次桌面调用都以 `invalid args … missing required key opts` 失败。回归网是 `src-tauri/src/commands/interchange_test.rs` 与 `crates/simple-post-server/src/web_api/routes/interchange_test.rs`。

**`http_request` 在 `src-tauri/src/lib.rs` 必须保持非 `pub`**，否则 `#[tauri::command]` 与 `generate_handler!` 之间的 `__cmd__http_request` 标识符冲突（E0255）。

`core::spec` / `interchange_api` / `project_api` / `spec_api` / `dynamic_api` 这类**编排**住在 server crate 或 core，两侧薄壳只做线格式转换——编排写在某一侧，另一侧就静默少一半。

### MCP：防线与契约

- **stdout 纪律。** `simple-post-mcp` 的 stdout 只跑 JSON-RPC 帧；日志与 panic 走 stderr。`simple-post-core::script` 也链接进这个 daemon，故它连 `println!` 都不许有（`stdout_test.rs` 扫源码）。
- **权限两道门，都 fail-closed。** 默认策略 `read_only`。Gate A（全局）：`mcp::tool_descriptors` 按 `REQUIRED_CAPABILITY` 过滤 `tools/list`，`mcp::dispatch` 在处理前再查一次（客户端直调 `tools/call` 绕不过去，只会拿到 `AppError::PermissionDenied`），无能力行的工具视为未分类并**拒绝**。`Capability` 顺序 `Read < Create < Update < Delete < Send`，`Send`（出站 HTTP）只在 `full_access` 可见。Gate B（逐项目）：`required_permission()` 走 `permissions` 表（`Read < ReadCall < Full`，默认 `ReadCall`），未知工具名按 `Full`。
- **Gate B 只会以两种方式静默失效，两种都发生过。** 一是**传错项目 id**：必须用 `db::nodes::root_id_of` / `db::environments::lookup_project_id` 取真实归属，传字面量 `"global"` 时 `agent_permissions.project_id` 是指向 `projects(id)` 的外键、而 id 是 `proj_<millis>` 铸的，查不到行就回落到 `Permission::default()`，于是每个写工具永久被拒而注册看起来完全正常（`project_create` / `project_list` 按构造没有归属项目，跳过 Gate B 是对的）。二是**丢掉结果**：`check_permission(..).await.ok()` 能编译并通过肉眼评审，要用 `?` 传播。回归网是 `crates/simple-post-mcp/src/mcp/mod_test.rs` 里的 `no_gate_is_passed_the_literal_global` 与 `no_permission_check_result_is_discarded`——它们是源码级扫描，改注释也可能让它们变红。
- **加一个 MCP 工具要改两处注册表。** `tool_table()` 里加了工具却没加 `REQUIRED_CAPABILITY` 行，它就完全绕开全局能力门；`every_registered_tool_has_a_capability` 与其反向 `every_capability_row_names_a_registered_tool` 是这个错的哨兵。
- **线上字段名是 camelCase，手写 JSON 是第三个出口。** `ToolDescriptor.input_schema` 与 `CallToolResult.is_error` 靠 `#[serde(rename_all = "camelCase")]` 输出成 `inputSchema` / `isError`，但 `daemon_http.rs::handle_tools_call` 的失败分支用 `json!({…})` 自己拼信封，绕开这两个结构体——只修结构体会留下那一处。工具失败是 `CallToolResult` 里的 `is_error: true`（不是 JSON-RPC 信封错误），LLM 客户端才看得见。`CallToolContent` 的 `rename_all = "snake_case"` 管的是 `type` 这个 tag，别混。

### 共享样本：一份规则，两处实现

请求装配、body 解析、base URL、OAuth2、动态变量、批量顺序、父子放置这些规则在 Rust 与 TS 各写一遍，**由 `fixtures/*.json` 钉住**。改动任何一条规则就改两处，否则样本会红；样本与两侧实现必须同一个提交落地（按语言拆 PR 会让中间那次提交一边读不动样本）。

| 样本 | 两侧实现 |
|---|---|
| `resolve-parity.json` | `resolve_endpoint` / `prepareRequest` |
| `multipart-parity.json` | `mcp::http_ops` / `src/lib/multipart.ts` |
| `body-parity.json` | `core::body` / `src/lib/body` |
| `dynamic-parity.json` | `core::dynamic` / `src/lib/dynamic-vars.ts` |
| `path-params-parity.json` | `core::path_params` / `src/lib/path-params.ts` |
| `base-url-parity.json` | `resolve_base_url` + `core::url::apply_rewrites` / `src/lib/url.ts` |
| `oauth2-parity.json` | `core::oauth2::inject` / `src/lib/oauth2.ts` |
| `oauth2-config-parity.json` | `core::oauth2::config` / `src/lib/http-build/auth.ts` |
| `collection-order-parity.json` | `db::nodes::ordered_subtree_endpoints` / `src/features/batch-run/model/collect.ts` |
| `placement-parity.json` | `ResourceNodeType::may_under` / `src/lib/node-placement.ts` |
| `graphql-parity.json` | `graphql_payload` / `src/lib/http-build/prepare.ts` |

两侧的读取器都要 `deny_unknown_fields` 并数一遍条目数——键名漂了会让整条测试静默空转。

三处只写一遍、别处不要重写：base URL 的**路径前缀改写**在 `core::url::apply_rewrites`（MCP 与导入导出都转发到它）；OAuth2 配置里那四个字段的 `{{var}}` 替换在 `core::oauth2::config::substitute_config`（两边把变量池交给 core）；错误形状 `{ code, message }`。

### 模块边界

超大文件是**同名模块目录之上的 facade**：文件只留模块表与 `pub use`，实现在 `<name>/<unit>.rs`（Rust 2018 会把 `foo.rs` 里的 `mod unit;` 解析到 `foo/unit.rs`，导入路径不变）。拆分时 facade 要**再导出**公开面而不是搬家；`#[path]` 测试挂载仍指向声明 `mod` 的那个文件，拆出来的测试文件要自己挂。TS 同形：`src/lib/http-build.ts` / `src/types/domain.ts` 是 facade，实现分在 `src/lib/http-build/*.ts` / `src/types/domain/*.ts` 并用 `export *` 再导出，导入路径（`@/lib/http-build`、`@/types/domain`）不动（同名文件优先于同名目录，`vite build` 照常解析）。

## Development Commands

```bash
npm run dev            # Vite dev server → http://127.0.0.1:5173
npm run dev:split      # Vite + `cargo run -- serve-web` (4173)，全后端联调用它
npm run build          # tsc -b && vite build → dist/
npm run typecheck      # tsc -b only
npm run check          # format:check → lint → typecheck → test → lint:openapi → fmt:check → clippy
npm run check:all      # check + test:rust（= cargo test --workspace），推之前跑这个
npm run lint:openapi   # 用 cargo example 生成一份导出文档（--out target/openapi.yaml）再用 Redocly 校验
npm run lint[:fix]     # ESLint 10 flat config
npm run format         # Prettier --write（*.md 与锁文件在 .prettierignore 里）
npm run fmt[:check]    # cargo fmt --all [--check]
npm run clippy         # cargo clippy --workspace --all-targets -- -D warnings
npm run tauri:dev      # Tauri 桌面开发壳
npm run tauri:build    # 生产 NSIS 安装包
npm run tauri:check    # cargo check --manifest-path src-tauri/Cargo.toml
npm run icons          # 从 src-tauri/app-icon.svg 重新生成全平台图标

npm test               # 前端单测（自动发现 src/**/*.test.ts）
npm run test:watch     # 同上，watch
npm run test:mcp       # MCP 端到端（需已编译二进制）
npm run test:web:auth      # serve-web bearer 鉴权集成
npm run test:web:basepath  # serve-web 反代基础路径集成
npm run perf:bench     # serve-web p50/p95/p99（--assert 卡 200ms p95）
npm run release        # 一次发布：4 个版本源 + 2 个锁文件 → PR → rebase merge → 注解 tag（-- --dry-run 只打印计划，-- --publish 填正文并取消草稿）

cargo test --workspace
cargo test -p simple-post-core
node scripts/dump-openapi.mjs > openapi.yaml
```

Vite `5173` 的 `/api` 代理到 axum `4173`；**不代理 `/mcp`** —— 那是 MCP 守护进程自己的端口，开发时由操作系统分配。

**推之前 `npm run check:all` 必须是绿的。** 它比 `npm run check` 多一条 `test:rust`：`check` 刻意不含 Rust 测试（它是编辑期的秒级回路），故只改 Rust 的一次改动可以让 `check` 绿而 Rust 断言 panic。CI 另跑三样 `check` 没有的：生产构建、`cargo test --workspace`、容器冒烟。

**测试目录外的东西不在任何门里**：`scripts/` 下需要活进程或已编译二进制的脚本（`mcp-integration.test.mjs`、`mcp-repl.mjs`、`perf-bench.mjs`、`e2e/*`）不会被跑，删改路由 / 命令 / MCP 工具时手动扫一遍；`src/lib/__tests__/scripts-manifest.test.ts` 是这轮清扫的静态半边（`package.json` 里的 `node scripts/…` 必须存在、`scripts/` 下的文件都要从某个 npm 脚本可达或在 `STANDALONE` 里写明理由、脚本里的 `/api/…` 字面量必须是 `web-routes.ts` 的路径前缀）。

## Code Conventions & Common Patterns

- **Zustand selector 会死循环。** 相等性是 `Object.is`，返回新数组 / 新对象的 selector（`.map` / `.filter` / 展开）每次渲染都触发重渲染。要么返回稳定引用或 `.find(...)`，要么在组件里用 `useMemo` 派生。slice 之间不互相 import，共享状态走 `AppState` 接口与 `get()` / `set()`。
- **`renderToStaticMarkup` 读不到改过的 store。** `useStore` 把 `getInitialState()` 当 `getServerSnapshot`，故测试里 `setState` 之后断言渲染结果只会「看起来跑了」。要覆盖状态就把渲染提成收 props 的组件，或把判断提成纯函数。
- **一个 modal 层。** `src/components/ui/modal-surface.tsx` 是「模态」的唯一实现（portal、Esc、背板点击、焦点陷阱、背景 `inert`）。`Dialog` 与 `CommandPalette` 建在它上面；第三个也必须复用它。Esc 只在 `ModalSurface` 的捕获阶段处理一次并 `stopPropagation()`；焦点恢复发生在 `inert` 解除**之后**；`z-index` 只从 `src/components/ui/layers.ts` 取。
- **Axum 0.8 路径参数是 `{id}`**（`/api/projects/{id}`）。`:id` 在构造 router 时 panic，一次漏改会让整个 crate 的测试同时挂掉。
- **命名。** Rust：`snake_case` 函数 / 模块 / 列，`PascalCase` 类型；Tauri 命令 Rust 侧 `snake_case`、前端 `COMMANDS` 里 `camelCase`。测试文件 `*.test.ts`；Rust 测试放**同级** `*_test.rs` 并由 `#[path]` 挂载。
- **三条能力是刻意不做的**（AI 文档生成、AI 用例生成、Git 同步工作区，issue #9/#10/#11 已关闭）：外部 AI 客户端经 `endpoint_list` / `endpoint_get` 读、经 `endpoint_create` / `endpoint_update` 写就够。**别加 AI provider 抽象、API key 偏好或文档生成工具** —— 那正是关闭它们要避开的密钥管理面。
- **IME 守卫。** `src/App.tsx` 的全局快捷键必须判 `e.isComposing`，否则中文输入法回车会发请求。
- **`auth::is_public_path` 里点名但从未注册的路由会返回 `200` + SPA 壳。** `/healthz` 就曾这样骗过只 grep `200 OK` 的容器 HEALTHCHECK。健康探针要断言**响应体**，不是状态行。
- 协议（`http` / `graphql` / `sse` / `websocket`）是端点属性、存在 `endpoint_details.protocol`，不是新资源；细节见 `docs/agent/transports.md`。
- 地址只存在 `environments.module_base_urls_json` 的 **(环境 × 模块)** 格里，没有项目级地址表、没有回退层；细节见 `docs/agent/request-assembly.md`。
- 源码注释有四条硬规则（只写现在成立的、中文一句一个结论、每句点名施行者、不用比喻），另有三处**读源码**的回归哨兵会被注释改动弄红——见 `docs/agent/comments-and-traps.md`。
- OpenAPI 的单一来源是 `crates/simple-post-server/src/web_api/spec/table.rs::public_route_specs()`；**不在表里的路由算内部**，不进文档。旧 `docs/http-api-reference.md` 已删，那些「生成规格表达不出的陷阱」收在 `docs/agent/server-architecture.md`。

## Testing & QA

**没有 Vitest/Jest/RTL。** 前端用 Node 内置 runner（`tsx --test` + `node:assert/strict`），用例在 `src/*/__tests__/*.test.ts`；**界面动作的语义写在有测试的层**（store 动作 + 纯函数），JSX 里的编排无人观测，凡是顺序 / 可选参数有意义的组合都收成一条命名的 store 动作并在 `src/stores/__tests__/` 断言。跨语言规则由 `fixtures/` 两侧读取器钉住。其余（Rust 测试放置、源码哨兵清单、删除用例的判据、headless store 的 `_browser-shim.ts`、无种子数据、`fresh_pool()`）见 `docs/agent/testing.md`。

## Runtime/Tooling Preferences

- **Node `>=20 <23`**（`package.json` engines），包管理器是 **npm**（lockfile v3）；不加 pnpm/yarn/bun 的锁文件。根 `package.json` 没有 workspaces。
- **Rust** edition 2021，MSRV **1.87**，工具链 track `stable` 且装 `rustfmt` + `clippy`（`src-tauri/rust-toolchain.toml`）。地板由 `rquickjs` 定：只有它同时给出真实的墙钟中断与内存上限。要钉基线就写 `channel = "1.87.0"` 或更新并先 `rustup toolchain install`。Windows 上用 PowerShell 助手（`scripts/*.ps1`）时写 UTF-8 要显式 `-Encoding utf8NoBOM`，5.1 的默认 ANSI 会喂坏 `rustc`。
- 前端 lint/format：**ESLint 10 flat config + Prettier 3**。ESLint 管正确性（`rules-of-hooks`、未使用变量），样式全归 Prettier（`eslint-config-prettier` 排在配置末尾）。Rust 侧 `cargo fmt` 与 `cargo clippy -D warnings`。
- **行尾 LF**（`.gitattributes` 的 `* text=auto eol=lf` 覆盖 `core.autocrlf`），只有 `*.bat` / `*.cmd` 是 CRLF。`.editorconfig` 镜像同一套（UTF-8、末尾换行、去尾空白、2 空格 / `*.rs` 4 空格）。
- **跑 worktree 构建前先改数据目录。** `serve-web` 与 GUI 默认都用 `dirs::data_dir()` 下**同一个** `store.db`；`SCHEMA_VERSION` 不匹配时它会被**删掉**。任何 UI 验证都先设 `SIMPLE_POST_DATA_DIR=<scratch dir>` 或传 `--data-dir`（`npm run dev:split` 不会替你设）。
- TypeScript 别名 `@/*` → `src/*`；`moduleResolution: bundler`、`noEmit`。workspace 产物在 `<repo>/target/`，不在 `src-tauri/target/`。
- 多 worker 约定：并行 worker 只交付代码；端到端 / 浏览器验证由主会话统一做一次（共用 dev server、浏览器会话与可写文件会互相踩，症状像随机失败而不像冲突）。
