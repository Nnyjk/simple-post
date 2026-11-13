> 适用范围：读代码/笔记遇到不熟的词时回查；写代码前确认自己没把两个词当同一个
> 权威来源：AGENTS.md §Architecture & Data Flow / §Code Conventions、crates/simple-post-core/src/db/schema.sql、crates/simple-post-core/src/domain/、crates/simple-post-interchange/src/ir.rs
> 最后核对：2026-11-13（v0.1.34）
> 前置阅读：01-overview.md

# 10 术语表与不变量速查卡

## 一句话结论

这个仓库的术语大多是**存储层的具体物**（表、列、单元格），混用的代价是直接写出错误代码：把 `root_id` 当成 `parent_id`、把「默认环境」当成「当前环境」、把 IR 的 `key` 当成 `nodes.id`，每一种都会在某个形态里安静地错。

## 结构：术语

### 存储与资源树

| 术语 | 它到底是什么 | 别和什么混 |
| --- | --- | --- |
| `node` | `nodes` 表的一行，`type` 区分 module / collection / endpoint。层级靠 `parent_id`，三种类型共表 | 项目**不是** node：项目是隐式根上下文 |
| `root_id` | 反范式化到每个节点上的**所属项目 id**，一次索引读就能回答「属于哪个项目」，同时是 MCP 权限门的键 | `parent_id`（直接父节点）；跨项目移动时要整棵子树重推 `root_id` |
| `placement` | 父子合法性矩阵：`ResourceNodeType::may_under`（module 只能顶层；collection 可挂 module/collection；endpoint 可挂 module/collection）。SQLite 触发器 + 应用层双保险 | 「排序」或「拖放位置」；它是**合不合法**，不是落在第几位 |
| `endpoint_details` | 只属于 endpoint 的请求配置行（method/url/auth/body/headers/params/tags/tls），按 node id 一行 | module/collection 上不存在这行；它们也不该有 method/url |
| `node_scripts` | collection 级的两段脚本；触发器 `trg_node_scripts_collection_only` 保证 endpoint/module 写不进来 | endpoint 自己的脚本住在 `endpoint_details` 里，不在这张表 |
| `preferences` | 全局 `key → value` 表（`ui.theme`、`mcp.auto_start`、`mcp.policy`、`mcp.http_config`、`request_settings`、`response_history.keep`） | 环境变量（`environments.variables`）；也**不是**每个项目一份 |
| `SCHEMA_VERSION` | 当前是 **5**（`db/mod.rs:58`）。`ensure_current_schema` 只在库的 `user_version` **更低**时**删库重建**（没有迁移路径）；**更高**时返回校验错误、文件原样保留，提示升级构建或换 `SIMPLE_POST_DATA_DIR` | `ADDED_COLUMNS`（增量补列，**不** bump 版本，不删数据） |
| WAL | SQLite 的 journal 模式，带 `-wal` / `-shm` 两个旁文件；只在单一文件系统上成立，故 swarm 副本数固定 1 | 「备份可以直接拷 `store.db`」——运行中拷会拿到不完整事务 |

### 请求装配

| 术语 | 它到底是什么 | 别和什么混 |
| --- | --- | --- |
| `base URL cell` | （环境 × 模块）那一格，存在 `environments.module_base_urls_json` 里，值为 `{url, rewrite}`。没有项目级地址列表，也没有第二层 key | 「项目的 base URL」；模块没有格子就是没有地址，**不继承**任何别的模块 |
| `rewrite rule` (`RewriteRule`) | 格子里那条路径前缀改写：换掉端点 URL 的一段，实现只有 `core::url::apply_rewrite` | 模块地址本身；改写只作用于**端点自己的** URL |
| `host` | URL 的 scheme + authority。**不在 endpoint 上**：它归模块那一格，或归端点自己的 `baseUrlOverride` | 路径；导出时因为 host 不在 endpoint 上，导出方必须自己选一个环境 |
| current environment | 每个客户端自己的实时状态（桌面窗口 / 浏览器标签各一份），只写 store 与 localStorage，**不落库** | default environment（项目属性：`created_at` 最早的那个环境） |
| `PreparedRequest` | 渲染进程 `prepareRequest` 的产物：拼好的 URL、有序头、body、缺变量清单 | `outbound`（真正发出去的那份，含 OAuth2 token） |
| `ResolvedEndpoint` | Rust `resolve_endpoint[_with]` 的产物，与 `PreparedRequest` 是同一件事的两侧实现 | 端点行本身（`EndpointDetail` 是输入，不是产物） |
| parity fixture | `fixtures/*.json` 里那 10 份跨语言样本，Rust 与 TS 各有一个读取器 | 单测快照；它是**两侧**共读的契约，改一边就红 |

### MCP 与授权

| 术语 | 它到底是什么 | 别和什么混 |
| --- | --- | --- |
| daemon | MCP 守护进程：**子进程**，监听自己的端口（`mcp.http_config`），读写同一份 `store.db` | 主服务（4173 的 REST+SPA）；主端口没有 `/mcp` |
| `mcp.port` | 数据目录里的端口记录文件（`pid`/`port`/`bind`/`started_at`/`data_dir`/`version`/`config_fingerprint`），stdio 桥靠它发现端口 | 「配置里的端口」；记录是**已经跑起来的那个** daemon 的自述 |
| `capability` | 全局能力：`Capability::{Read, Create, Update, Delete, Send}`，由 `registry.rs::REQUIRED_CAPABILITY` 逐工具声明 | `CapabilityPolicy`（用户三档：read_only / safe_write / full_access）与 `CapabilityOverride`（`--capability` 本地只收窄） |
| `permission` | 逐项目授权：`Permission::{Read, ReadCall, Full}`，存在 `agent_permissions` 表 | capability；两道门取**更严**的一项，不是二选一 |
| tool descriptor | 发给客户端的工具清单项，线格式是 `inputSchema` / `isError`（camelCase） | 工具 handler；descriptor 写错 snake_case 会让所有工具在合规客户端里变成「无参数」 |
| stdio bridge | `simple-post-mcp` 二进制：stdin/stdout 的 JSON-RPC ↔ HTTP `/mcp` | `simple-post mcp`（守护进程本身） |

### 前端

| 术语 | 它到底是什么 | 别和什么混 |
| --- | --- | --- |
| surface | 全窗口宿主：`WelcomeScreen` / `WorkspaceShell` / `SettingsSurface`，由 `App.tsx` 按 `activeProjectId`（与设置 tab）分流 | 标签页（tab）；surface 之间是整块换掉，不是并排 |
| workspace | 有项目时的那套 chrome：顶栏 + 树 + 标签 + `HttpTester`，整体 `lazy()` | 项目（project）；workspace 是界面壳，项目是数据 |
| `dirty tab` | 有未保存改动的标签，关闭时进 `pendingDirtyClose` 队列，由 `DirtyCloseDialog` 逐个表态 | 「未保存的端点」；非 endpoint 标签也可能脏（行内改名在途） |
| draft endpoint | 还没落库的 endpoint（新建立即编辑），草稿存在 store 里，没有 `nodes` 行 | 已保存但脏的 endpoint；草稿没有 collection 脚本链 |
| `bootstrap` 两层 | 第一层 `projects_list` + `request_settings_get`（await，翻 `loaded`）；第二层 `nodes_list` 等（不 await，失败进 `nodesError`） | 一次加载；`nodes: []` 在 `nodesLoaded === false` 时是**歧义**，不能当空项目 |

### 导入导出与工程

| 术语 | 它到底是什么 | 别和什么混 |
| --- | --- | --- |
| IR | interchange 的中间态（`Exchange` / `IrModule` / `IrCollection` / `IrEndpoint`），三条边性质不同：`format → ir` 有损、`ir → domain` 无损全函数、`domain → ir` 是投影 | `domain` 类型；IR 有自己的诊断、丢失集与无 id 的树 |
| `key` | IR 节点身份：导入时是源格式位置指针（与 `Diagnostic::pointer` 同一套寻址），导出时是 `nodes.id` | `nodes.id`；导入阶段的节点还没有 id |
| `Diagnostic` | `format → ir` 表达不了的东西，必须产出而不是静默丢 | 错误；导入可以带诊断成功 |
| fingerprint | 导入去重的指纹，算在 IR 上，故两种导入器互相认得彼此导入过的条目 | 哈希文件名；它是内容身份 |
| dual-transport | 同一条后端命令的桌面 IPC 壳与 REST 壳 | 「两个实现」；业务实现只有一份，壳有两层 |
| wire DTO | 跨传输的线格式类型，只定义在 `crates/simple-post-server/src/dto.rs` | `domain` 类型；DTO 是传输形状，domain 是存储/领域形状 |
| `sink` | **本仓库没有这个术语**。唯一相近的是 `SourceFetcher`（动态变量取值的抽象，由 MCP crate 实现）；`sink` 只出现在测试里作为 channel 接收端的局部变量名 | `SourceFetcher` |

## 不变量速查卡

| # | 不变量 | 指针 |
| --- | --- | --- |
| 1 | 新增后端命令要改五处：handler / `commands/mod.rs` glob 导出 / `lib.rs` 注册 / `COMMANDS` / REST route + `WEB_ROUTES` | [02](02-architecture.md) |
| 2 | `commands/mod.rs` 必须是 glob `pub use`，列条目会 E0433 | [02](02-architecture.md) |
| 3 | Tauri 命令参数逐字段摊平，不接 `opts: Struct`（桌面侧会缺 key） | [02](02-architecture.md) |
| 4 | 两端共用的 DTO 只定义在 `server::dto`，`commands/dto.rs` 只转发 | [02](02-architecture.md) |
| 5 | 请求规则写两处（TS `prepareRequest` / Rust `resolve_endpoint`），由 `fixtures/` 钉住；改一处必改另一处 | [04](04-request-pipeline.md)、[08](08-interchange-and-parity.md) |
| 6 | 路径前缀改写只有一个实现：`core::url::apply_rewrite` | [04](04-request-pipeline.md) |
| 7 | 当前环境不落库；默认环境 = `created_at` 最早的那个 | [03](03-data-model.md)、[05](05-mcp.md) |
| 8 | `SCHEMA_VERSION` 只在库里版本更低时删库重建（无迁移）；更高时报错并保留文件 | [03](03-data-model.md) |
| 9 | 端点自带的 TLS 设置**整体取代**全局那份，不逐字段合并 | [04](04-request-pipeline.md) |
| 10 | 脚本链顺序 collection → nested collection → endpoint，前置与测试两阶段同序；单个脚本失败则它对请求与变量的改动全部丢弃 | [06](06-dynamic-and-scripts.md) |
| 11 | MCP 端点是 daemon 自己的端口，主端口不提供 `/mcp` | [05](05-mcp.md) |
| 12 | 全局能力门 + 项目权限门，取更严；默认 `read_only` | [05](05-mcp.md) |
| 13 | 首屏红线：重壳 `lazy()`、`manualChunks` 必须是函数、GSAP 不在关键路径 | [07](07-frontend.md)、[02](02-architecture.md) |
| 14 | 历史存的是**解析后**那份请求（含 OAuth2 token），预览不变量是另一份 | [04](04-request-pipeline.md) |
| 15 | `interchange` 的三条边性质不同；`mcp` 刻意不依赖它 | [08](08-interchange-and-parity.md)、[02](02-architecture.md) |

## 陷阱

- **`root_id` ≠ `parent_id`**：权限门与「这个项目有哪些节点」都读 `root_id`；把模块的 `parent_id = NULL` 当成「没有项目」会让它从所有项目视图里消失。
- **「当前环境」不是数据**：没有任何 `is_active` 列，也没有 `POST /api/environments/active`。想「切一次环境全部接口换地址」是设计本身，不是 bug。
- **`default` 与 `current` 只在都没显式选择时才一致**：界面可能记住非默认环境，而 MCP 不点名时用默认环境。
- **IR 的 `key` 在导入阶段不是 id**：拿它去查 `nodes` 一定查不到。
- **`sink` 不要当术语用**：仓库里没有这个抽象，写进设计讨论会让下一个人去找一个不存在的东西。

## 相关笔记

- [01-overview.md](01-overview.md) — 三种形态与目录
- [02-architecture.md](02-architecture.md) — 依赖与五处协同
- [03-data-model.md](03-data-model.md) / [04-request-pipeline.md](04-request-pipeline.md) / [05-mcp.md](05-mcp.md) / [06-dynamic-and-scripts.md](06-dynamic-and-scripts.md) / [07-frontend.md](07-frontend.md) / [08-interchange-and-parity.md](08-interchange-and-parity.md) / [09-engineering-process.md](09-engineering-process.md)

## 文档与实现不一致

- 未发现术语层面的矛盾。`AGENTS.md` 的「47-tool registry」与实际 48 条属计数笔误，已记在 [01-overview.md](01-overview.md)。
