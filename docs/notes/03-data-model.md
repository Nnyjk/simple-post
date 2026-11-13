# 数据模型

> 适用范围：`crates/simple-post-core/src/db/`（schema、CRUD、事务）、`domain/` 里与落库形状绑定的类型；不涉及前端 store 与 HTTP 层。
> 权威来源：`crates/simple-post-core/src/db/schema.sql`、`db/schema.rs`、`db/mod.rs`、`db/pool.rs` 及各表 CRUD 文件、`port_file.rs`；三处 `db::init` 调用点（`src-tauri/src/lib.rs`、`crates/simple-post-server/src/lib.rs`、`web_api/serve.rs`）；`AGENTS.md § Key Directories`。
> 最后核对：2026-11-13（v0.1.34）
> 前置阅读：`01-overview.md`

## 结论

- **默认情形**（三种形态都不给 `--data-dir`、且 `SIMPLE_POST_DATA_DIR` 一致时）全应用只有**一个**库文件，路径恒为**调用方传给 `db::init` 的那个目录** + `store.db`（`db/pool.rs:24-26`，不加任何后缀）。例外是 `serve-web --data-dir D`：它把主进程的库挪到 `D\store.db`，而 daemon 子进程照旧用基准目录那份，于是**两份库互不相干**——细节见本节下面的「数据目录与落点（三种形态）」。默认调用方传 `port_registry::data_dir()`：`SIMPLE_POST_DATA_DIR`（非空时）否则 `dirs::data_dir()`，**原样**返回、不带 `simple-post` 段，故 Windows 默认是 `%APPDATA%\store.db`；`mcp.port` 则在 `<同一基准目录>\simple-post\mcp.port`——两者**不同层**。WAL + `foreign_keys=ON` + `busy_timeout=3000`，连接池固定 4 条。读走 `db::with_conn`，写走 `db::with_tx`。
- 资源树是**单表多态**：`nodes` 用 `type` 判别 module / collection / endpoint，`endpoint_details` 只装 endpoint 专属列，`node_scripts` 只装 collection 的脚本。项目**不是**节点，由 `root_id` 指名。
- schema 只增不迁：`SCHEMA_SQL`（`CREATE ... IF NOT EXISTS`）+ `ADDED_COLUMNS`（给既有表补列）+ 打开时 DROP 废弃索引/表 + 动态变量回填。版本号不符的新库（`user_version` > 当前）**报错**，更旧的文件**删库重建**。
- 所有主键由 `domain::uid()`（`sp_` + 纳秒 + 进程内计数器）产生；`spec::mint_id(prefix)` 在其上加业务前缀 `proj_` / `mod_` / `col_` / `ep_` / `env_`。
- 结构化字段一律 JSON-in-TEXT，serde 的形状**就是**线格式：改字段名等于改库内容可读性，必须 bump `SCHEMA_VERSION`（见 `auth_json` 的 v3）。

### 数据目录与落点（三种形态）

`db::init(data_dir)` 只做 `data_dir.join("store.db")`（`db/pool.rs:24-26`），**不**附加后缀；`port_file::port_file_path()` 恒为 `<同一基准目录>/simple-post/mcp.port`（`port_file.rs:61` 是 `base.join("simple-post")`）。故 `store.db` 在基准目录根上，`mcp.port` 在其 `simple-post/` 子目录里——两者**不同层**。

| 形态 | 传给 `db::init` 的目录 | `store.db`（Windows 默认） | `mcp.port` 落点 |
|---|---|---|---|
| 桌面（Tauri） | `port_registry::data_dir()` = `SIMPLE_POST_DATA_DIR`（非空时）否则 `dirs::data_dir()`（`src-tauri/src/lib.rs:131,138`） | `%APPDATA%\store.db` | `%APPDATA%\simple-post\mcp.port`（supervisor 写） |
| serve-web | `--data-dir` 给了就用；否则 `default_data_dir()` = `port_registry::data_dir()`（`cli.rs:92-101`、`web_api/serve.rs:35,120-122`） | `%APPDATA%\store.db`；给了 `--data-dir D` 则是 `D\store.db` | 仍由 supervisor 按 `port_registry::data_dir()` 算：`%APPDATA%\simple-post\mcp.port` |
| MCP daemon | `port_registry::data_dir()`（`crates/simple-post-server/src/lib.rs:100,121`）；子进程按环境变量自行解析，`--data-dir` 不会传给它 | `%APPDATA%\store.db` | 由 supervisor 写；独立跑 `mcp` 时自己写（`lib.rs:176-186`） |

macOS / Linux 只是把 `%APPDATA%` 换成 `dirs::data_dir()` 的平台值（`~/Library/Application Support`、`$XDG_DATA_HOME` 或 `~/.local/share`）；`mcp.port` 的完整清单见 `port_file.rs` 文件头。

- **`daemon_supervisor::host_data_dir()` 不是库位置**：它返回 `dirs::data_dir()/simple-post`，只用于填端口记录里的 `data_dir` 字段（`daemon_supervisor.rs:62-65,315`）；按它去找 `store.db` 会落空。
- **设 `SIMPLE_POST_DATA_DIR=X` 后两者不同层**：库在 `X\store.db`（连同 `-wal`、`-shm`），端口文件在 `X\simple-post\mcp.port`。按目录备份 / 挪库时只拷 `X` 根会漏 `mcp.port`，只拷 `X\simple-post\` 会漏库。
- **`--data-dir` 只挪主进程的库**：桌面端不传它；`serve-web --data-dir D` 时只有主进程的 REST 库搬到 `D\store.db`，`daemon_cli_args` 不带数据目录、也不为子进程设 `SIMPLE_POST_DATA_DIR`，故 daemon 子进程仍按环境变量（未设则 `%APPDATA%`）打开**另一份** `store.db`，端口文件也仍在 `%APPDATA%\simple-post\`。要整体搬迁必须设 `SIMPLE_POST_DATA_DIR`，不要只给 `--data-dir`。

## 结构

| 表 | 装什么 | 谁写 | 谁读 |
|---|---|---|---|
| `projects` | 项目：`id`、`name`、`description`、`color`、`created_at`、`updated_at` | `db/projects.rs`、REST/Tauri 命令、MCP | 同左；`create_with_defaults` 一次种下默认环境 + 默认模块 |
| `environments` | 环境：`(环境 × 模块)` 的 `module_base_urls_json`、`variables_json`、`disabled_variables_json`、`created_at` | `db/environments.rs`、导入提交（`commit_in_tx` 写 base URL 格子） | 发送链、设置面板、MCP |
| `agent_permissions` | 逐项目 agent 权限，每项目最多一行：`permission ∈ {read, read_call, full}` | `db/permissions.rs::set` | 权限闸；缺行默认 `ReadCall` |
| `preferences` | 单键值袋：`key` TEXT PK、`value` JSON 编码字符串 | `db/preferences.rs::set` | 渲染层启动补水、MCP 策略、请求设置 |
| `nodes` | 树节点：`root_id`、`parent_id`、`type`、`name`、`description`、`sort_order`、时间戳 | `db/nodes/*`、`spec/upsert`、MCP `project_ops` | 树/列表/搜索/权限定位 |
| `endpoint_details` | endpoint 配置：`method`、`url`、`base_url_override`、params / path_params / headers / body / auth / tags / tls JSON、脚本、docs / notes | `db/endpoint_details.rs`，成对写入只经 `nodes::create` / `delete` | 发送链、编辑器、搜索（url） |
| `node_scripts` | collection 的两段脚本；`node_id` 既是 PK 也是外键 | `db/node_scripts.rs::set`（先查类型） | 链装配 |
| `response_history` | 已持久化响应：状态、耗时、大小、headers/body/body_base64/body_json、请求快照、`is_favorite`、`body_truncated` | `db/response_history.rs::append` | 历史条带，按 endpoint 取 |
| `oauth2_tokens` | 换来的 token；PK 是配置哈希 `auth_key`（不是 endpoint id） | `oauth2/store.rs` | 发送前按需刷新 |
| `dynamic_variables` | 定义：`project_id`、`name`、数据源（端点或内联）、`extract_path`、`transform_script`、`value_mode`、`ttl_seconds`、`stale_fallback`、`enabled` | `db/dynamic_variables/write.rs`、MCP | 变量解析、设置界面 |
| `dynamic_variable_cache` | 一个（定义 × 环境）的当前值：`value_json`、`fetched_at`、`expires_at`、`is_override`、`override_expires_at` | `db/dynamic_variable_cache.rs::upsert`、解析器 | 三个进程共享的缓存（桌面 / serve-web / MCP） |

索引（见 `schema.sql`）：`idx_envs_project`；`idx_nodes_root(root_id, sort_order)`、`idx_nodes_parent(parent_id, sort_order)`、`idx_nodes_type(type, name)`；`idx_response_history_endpoint(endpoint_id, created_at DESC)`；`idx_dynamic_vars_project_name`（部分索引，`project_id IS NOT NULL`）、`idx_dynamic_vars_orphan_name`（`project_id IS NULL`）、`idx_dynamic_cache_key(definition_id, environment_id)` 唯一。`endpoint_details` / `node_scripts` / `oauth2_tokens` 只有主键。

### nodes 树

- `parent_id` 是唯一层级连接；module 的父是项目，故存 `NULL`。`root_id` 把项目反范式化到每个节点：`list_by_root` 一次走索引，`root_id_of` 一次主键命中——逐项目权限闸的第一步。
- 父子矩阵：`ResourceNodeType::may_under`（`domain/resource.rs`）——module 无节点父；collection 可挂 module / collection；endpoint 可挂 module / collection。应用层在 `nodes::placement::validate_placement` 每次写入时执行；`schema.sql` 的 `trg_nodes_parent_matrix_insert` / `_update` 是第二道防线（MCP 是主要外部写路径，会绕过 `db::nodes`）。触发器每次 `migrate` DROP 再建，因为触发器体**就是**契约。
- 显示序：`ORDER BY sort_order ASC, id ASC`（`nodes/read.rs`），父子作用域按 `parent_id` 而非 `(parent, type)` 划定（同父下类型本就一致）。`move_many` 把目标父下的兄弟重编号成连续 `0..N`，并为整棵子树重算 `root_id`；`move_to_root` 专管模块跨项目移动（还要重编**旧**项目的顶层序）。
- `duplicate` 只支持 endpoint，且**必须**在 `with_tx` 内：先按显示序压平同父兄弟（`sibling_ids`），副本插到原件后一位。直接给「原件之后的兄弟 +1」会踩两个洞：同 `sort_order` 的兄弟不在「之后」；`sort_order = i64::MAX` 时加一在 SQLite 里溢出成 REAL，而列读成 `i64`，那一行就读不出来了。
- 搜索：`like_pattern` 把用户输入里的 `!` `%` `_` 用 `!` 转义，SQL 侧 `LIKE ?1 ESCAPE '!'`；url 匹配需 `LEFT JOIN endpoint_details` 且以 `type = 'endpoint'` 守卫。

### endpoint_details

- 分表理由：`method` / `url` / `body` / `auth` / `tags` 对 module 与 collection 毫无意义；折叠进 `nodes` 会让它们带一组永远为空的列，也让搜索里的 `method = ?` 无物可分。`node_id` 既是 PK 也是外键，树列（name / sort_order / 时间戳）只住 `nodes`。
- `*_json` 清单：`params_json`、`path_params_json`、`headers_json`、`body_json`、`auth_json`、`tags_json`、`tls_json`。`path_params_json` 与 `params_json` 分开：前者替换进路径，后者进查询串。
- `tls_json` 是唯一可空的结构化列：`NULL` = 继承全局（`preferences.request_settings.tls`）；非空即端点自带一整套 HTTPS 设置，整体取代全局。
- endpoint 行**不绑定环境**（v5 起没有 `environment_id`）：接口地址由发送时选中的 `(环境 × 模块)` 格子决定。

### preferences

- 一张 `key TEXT PRIMARY KEY` / `value TEXT` 的表；`value` 是 serde JSON 编码后的字符串，`get<T>` / `set<T>` 用泛型还原。渲染层启动时 `list_all` 一次补水。
- 现役键（`preferences::keys`）：`ui.theme`、`mcp.auto_start`、`mcp.policy`、`mcp.http_config`、`request_settings`、`response_history.keep`。新设置应加常量，不要散写字面量。
- `set` 对 `request_settings` 额外做两件事：把刚序列化的值重新反序列化成 `domain::RequestSettings`（**解析失败必须报错**，不能当「不是设置」照旧存下去，否则请求体里 `timeoutMs` 类型写错就绕过了校验），然后 `tls.validate()`；最终存的是重新序列化的规范形态，不是请求体原样（避免陌生键与粘贴进路径框的证书正文落库）。校验放在此处，是因为这一行有桌面命令、`PUT /api/request-settings`、`PUT /api/preferences` 三条写路径。

### schema 版本策略

- `SCHEMA_VERSION = 5`（`db/mod.rs`），与 `schema.sql` 末尾 `PRAGMA user_version = 5` 必须一致（`schema_version_matches_the_constant` 测试盯着）。
- `ensure_current_schema`（`db/schema.rs`）在池建立**之前**跑：文件不存在 → 新建；`user_version == 当前` → 放行；`> 当前` → `Validation` 错误（新构建的库不该被旧构建毁掉）；`< 当前` → 删 `store.db`、`-wal`、`-shm` 并重建（留着旧 WAL 会把本该丢掉的数据重放进新库）。
- `migrate` 只服务增量，顺序固定：`drop_obsolete_dynamic_variable_tables`（旧预览形状）→ `add_missing_columns`（`ADDED_COLUMNS`，靠 `PRAGMA table_info` 判重）→ `execute_batch(SCHEMA_SQL)` → `drop_obsolete_request_settings_table` → `drop_obsolete_dynamic_variable_name_index` → `backfill_dynamic_variable_projects`。
- 新表 / 可空新列 = 追加进 `schema.sql` 或 `ADDED_COLUMNS`，**不** bump；「不能塞进追加可空列」的改动（删列、改名、拆表、改 JSON 字段名）→ bump = 删库。删死表/死索引也是打开时 DROP，不是版本事件。

## 不变量

1. 每个 endpoint 恰有一行 `endpoint_details`；module / collection 一行都没有。`nodes::create` 之前由 `check_detail_agreement` 强制，两半必须同一事务落。
2. `nodes.parent_id` 的类型矩阵恒成立（应用层 + 触发器）；`root_id` 与父一致在**创建**时校验，此后由 `move_many` 维护——父子在事务中途短暂不一致是合法的，触发器刻意不检查 `root_id`。
3. module 的 `parent_id IS NULL`；其余节点必须有父。顶层节点必须指名存在的项目（否则两条传输层的空 `rootId` 会直达 INSERT，报成外键故障）。
4. `node_scripts` 只许 collection：`node_scripts::set` 查一次类型，`trg_node_scripts_collection_only` 再查一次；空串与 NULL 都归一为「没有脚本」。
5. `tls_json IS NULL` 恒等于「继承全局 TLS」。
6. 环境的默认者是项目内 `created_at` 最小（`ORDER BY created_at ASC, id ASC LIMIT 1`）；`create` 在调用方给 0 时补 `now_ms()`，否则 0 会永远占住默认位。新建项目必带一个环境与一个模块（`projects::create_with_defaults`）。
7. 动态变量名在**项目内**唯一；`project_id IS NULL` 的未归属行按名字全局唯一。数据源二选一且必须有一个；`source_endpoint_id ON DELETE SET NULL`，定义留下并显示「数据源已删除」。
8. 缓存键是 `(definition_id, environment_id)`，无环境用空串；一行同时装自动值与覆盖值，靠 `is_override` 区分；`value_mode` 不在缓存里，故改它不必失效。
9. `response_history.body` 在写入时裁到 `domain::RESPONSE_HISTORY_MAX_BODY_BYTES`，并置 `body_truncated`；收藏行不参与条数裁剪（上限 `RESPONSE_HISTORY_KEEP_MAX = 500`，写入前夹到 `[1, 500]`）。
10. oauth2 token 属于**这份配置**（`auth_key` 哈希），不属于 endpoint；凭据只在 `endpoint_details.auth_json` 存一份。

## 陷阱

- **id 不能来自时间戳**。曾用「前缀 + 毫秒时钟」造 id，同一毫秒两次创建撞 `nodes.id`。唯一来源是 `domain::uid()`；`spec::mint_id` 只是换前缀。`uid()` 本身编码了生成时刻，计数器随进程重启归零——同一时钟节拍内重启会重复前缀，这是明示接受的本地库取舍。
- **`double_option` 三态**：`parent_id`、`description`、`path_params`、`docs`、`notes`、`pre_request_script`、`test_script`、`tls`、`base_url_override` 上「字段缺席 = 不动 / `null` = 清空 / 值 = 设」。serde 原生 `Option` 会把前两者折叠，故这些字段必须用 `deserialize_with = "double_option"`；新加可清空字段时忘了它，PATCH `null` 就静默无效。
- **补丁类型带 `deny_unknown_fields`**，故「GET → 改一处 → 整份 PATCH 回去」是契约：`ResourceNodePatch` / `EndpointDetailPatch` 必须认得 `id`、`rootId`、`kind`、`createdAt`、`updatedAt`、`nodeId`。这些身份字段**只被接受、不被写入**（`skip_serializing` 而非 `skip_serializing_if`），否则 `is_empty()` 会误判「有东西要写」。
- **`is_empty()` 由序列化结果回答**：`serialises_to_empty_object` 依赖各字段的 `skip_serializing_if`。序列化失败返回 `false`（偏向「非空」），让真实错误在 DB 层暴露而不是被吞成空操作。
- **`preferences.value` 必须带 JSON 引号**。手工把 `value` 写成 `full_access`（裸字符串）会让 `from_str::<String>` 失败：`mcp_policy` 静默退回 `ReadOnly`（看起来像「设置没生效」），`mcp.http_config` 则直接报错（防止把局域网守护进程静默降级成 loopback）。表现不同是刻意的，别统一。
- **两套权限旋钮不是一回事**：全局 `mcp.policy`（`read_only` / `safe_write` / `full_access`，缺行 = `read_only`）与逐项目 `agent_permissions.permission`（`read` / `read_call` / `full`，缺行 = `read_call`）。改一处不会改另一处。
- **`nodes::update` 接受 `sort_order` 直写**，绕过了 `move_many` 的 `0..N` 压平：它能造出重复或带洞的顺序。顺序的权威操作是 `move_many` / `move_to_root`；`sort_order` 只是决胜前的第一键。
- **`duplicate` 必须先压平**（见上节），且 `new_id` 让桌面端做乐观复制；MCP 不传，由本函数生成。
- **不要在 `with_tx` 闭包里再调 `with_tx`**。闭包拿到的是 `&mut Connection`，`with_tx` 会向池再要一条连接并 `BEGIN IMMEDIATE`；外层已持写锁，内层三次 50 ms 重试耗尽后以 `AppError::Db` 失败——这就是自死锁。要复用事务就传「已持有连接」的版本（`interchange::commit::commit_in_tx` 的存在理由）。
- `with_tx` 的闭包是 `Fn`，`SQLITE_BUSY` 时会被**重跑**：闭包内不要做不可重入的外部副作用（发网络请求、写日志文件）。
- `PRAGMA foreign_keys` 逐连接由池的 `with_init` 应用，不是库级设置；绕过池自建连接会失去外键与 busy_timeout。
- 删项目走 `ON DELETE CASCADE` 一条 DELETE 带走整棵树、环境、历史、脚本与权限行；`response_history` 的 `endpoint_id` 也级联，故没有应用层清理循环需要维护。`dynamic_variable_cache` 随定义级联。
- `idx_response_history_root` 是公开 DROP 的旧索引；`CREATE INDEX IF NOT EXISTS` 删不掉东西，加新索引时想撤旧的必须显式 `DROP INDEX IF EXISTS`。
- 动态变量解析在取数失败且 `stale_fallback = 0` 时不会回退旧值；`stale_fallback = 1` 才允许过期值。缓存表不存 `value_mode`，换形态不会让缓存失效。

## 相关笔记

- [01-overview.md](01-overview.md)：三进程（桌面 / serve-web / MCP）与它们共享同一个 `store.db`。
- [02-architecture.md](02-architecture.md)：crate 分层与双传输（Tauri 命令 / REST），决定谁调用 `db::*`。
- [04-request-pipeline.md](04-request-pipeline.md)：`(环境 × 模块)` 格子解析、`tls_json` 继承、变量替换。
- [05-mcp.md](05-mcp.md)：`agent_permissions` 那两套权限旋钮的另一半（`mcp.policy` 与逐项目 `permission`）。
- [08-interchange-and-parity.md](08-interchange-and-parity.md)：`commit_in_tx` 的事务边界与去重指纹。
- `AGENTS.md § Architecture & Data Flow`、§ Code Conventions（id、patch、事务的仓库级约定）。

## 文档与实现不一致

- `db/preferences.rs::keys::MCP_AUTO_START` 的注释写「**默认 `false`**」，而同文件 `mcp_auto_start()` 的注释与实现都是缺行回退 `true`（`Ok(raw.unwrap_or(true))`）。实现为真，键注释为旧。
- `db/permissions.rs` 顶部注释说缺行默认 `ReadCall` 是「刚导入的项目无需显式授权即可使用」；实际 `ReadCall` 允许的是只读 + 发请求，与全局 `mcp.policy` 缺行默认 `read_only` 并列时容易读成同一个开关（见「陷阱」）。措辞易混，非行为错误。
- `db/schema.sql` 中 `nodes` 段落写「纯增量，故 SCHEMA_VERSION 不需要 bump」，但该表是 `SCHEMA_SQL` 的一部分、随 v2/v3 的重建路径到达；注释本身不矛盾，只是别据此认为旧库能保留 `nodes` 之前的行。
- `[未核实]` `permissions.rs` 注释称「每项目一行」，schema 只以 `project_id` 为 PK 保证唯一，未见额外的项目存在性校验；`set` 对不存在的项目会以原始外键错误收场（有 FK 与 CASCADE，故不会留下悬挂行）。
