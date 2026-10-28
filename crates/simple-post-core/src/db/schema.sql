-- Simple Post schema.
-- 由 `db/mod.rs::migrate` 在启动时执行一次。幂等：每个 `CREATE` 都是
-- `IF NOT EXISTS`，故在已迁移的库上重跑是无操作。外键被强制；ON DELETE CASCADE 让删
-- project / module / collection 很廉价（对根一次 DELETE 带走整棵子树）。
--
-- 迁移建表并插入**零行**。全新安装按设计是空的——测试 fixture 在 `#[cfg(test)]` 背后的
-- `fixtures.json`，不在这里。
--
-- 时间戳约定：每个 `*_at` 列都是 unix 毫秒整数（JS / TS `Date.now()` 的返回值），
-- 比 RFC-3339 字符串更易比较、排序与经 serde_json 传输。

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  -- 项目的 ★ 服务：未指名服务的 endpoint 所解析的对象。每项目一个而非每模块一个——
  -- 服务表是项目级的，一个按模块划分的 ★ 会指名别的模块看不见的服务。NULL 是正常状态
  -- （项目还没被给过默认服务），不是一个缺失的值。
  default_base_url_definition_id TEXT,
  color TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 一条具名服务，由**项目**拥有。每个模块都能看到这份列表里的全部服务——没有按模块的服务表，
-- 故两个模块不可能持有同名的服务。这正是该模型的要义：
-- 「服务应该绑定到项目，这样每一个模块都会全部包含这些服务」。
--
-- 服务行不携带 URL。值住在 `environments.module_base_urls_json` 里，以 (服务 × 模块)
-- 为键，故这一行只回答「存在哪些服务」，别无其他。
--
-- 下面的 `module_id` 与 `is_default` 是**刻意保留的死列**。它们曾服务于本构建已推翻的
-- 「模块持有」模型（按模块的服务表与按模块的 ★）；`db::migrate` 把它们置空/置零，此后
-- 无人读写。留着是因为删列 = 重建表 = bump `SCHEMA_VERSION`，而 bump 会让
-- `ensure_current_schema` 删掉用户的库。`db::ADDED_COLUMNS` 同样把这两列补回那些早于它们的
-- 库，理由一致：迁移必须能**读**旧形状才能把它中和掉。
--
-- 这两列是本表首度发布之后才加的，故上面的 `CREATE TABLE IF NOT EXISTS` 对已有该表的库
-- 是无操作。
CREATE TABLE IF NOT EXISTS base_url_definitions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- 恒为 NULL。之所以可空而非 NOT NULL，是因为 `ADDED_COLUMNS` 只能给活着的表追加可空列。
  module_id TEXT REFERENCES nodes(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  -- 恒为 0。见上。
  is_default INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_base_url_defs_project
  ON base_url_definitions(project_id);

-- 曾经住在这里的 `modules` / `collections` / `endpoints` 三张表已 gone，由下面的 `nodes` +
-- `endpoint_details` 取代。为什么要拆成两张表而不是一张，见那里的小节头。

CREATE TABLE IF NOT EXISTS environments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 0,
  -- 已废弃，只为让该列存活。它曾是 `def_id -> url`，即服务还不携带模块之前的单一值层。
  -- 现在无人读写；`db::migrate` 已把每个现役值复制进 `module_base_urls_json`，
  -- 并是本 crate 里仅剩的读者。
  --
  -- 迁移**不**清空它可能留下的那一种形状（见 `migrate_base_urls_into_value_layer`）：
  -- 那些库在之后每次运行都重新推导出逐字节相同的值，故残留 blob 是惰性的。
  base_urls_json TEXT NOT NULL DEFAULT '{}',
  -- def_id -> module_id -> url。**base URL 唯一所在的一层。**
  -- 每个单元格独立：某模块对一个服务没有条目即对它没有 URL，且不会继承别的模块的或项目的。
  module_base_urls_json TEXT NOT NULL DEFAULT '{}',
  variables_json TEXT NOT NULL DEFAULT '{}',
  -- 被停用的变量名。停用的变量照样存、照样显示，但不参与替换。
  -- 独立一列而不是并进 variables_json：后者会改掉那一列的既有格式，
  -- 旧行读不回来，就成了要数据迁移的地带。
  disabled_variables_json TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS idx_envs_project ON environments(project_id);

CREATE TABLE IF NOT EXISTS agent_permissions (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  permission TEXT NOT NULL CHECK(permission IN ('read','read_call','full'))
);

-- 设置旋钮（超时 / 跟随重定向 / 响应体积上限）的现役容器是下面这张 `preferences` 表里的
-- `request_settings` **行**，见 `db::preferences::keys::REQUEST_SETTINGS`。
--
-- 这里曾有过一张同名的 `request_settings` **表**（单例行，`CHECK id=1`，三列即上述旋钮）。
-- 它从建立起就没有任何读者——界面与 MCP 侧读写的一直是 `preferences` 里的那一行，故那张表
-- 只是一处残留。它已被清除：`schema.sql` 里不再有它的 `CREATE`，`db::schema::migrate` 在
-- `SCHEMA_SQL` 之后无条件丢掉既有文件里的它。

-- 自由键值对袋子，用于渲染层的上次打开项目、UI 偏好、最近环境等。值经 JSON 编码，故
-- schema 不必预先知道具体有哪些键。
CREATE TABLE IF NOT EXISTS preferences (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- ===========================================================================
-- 资源树——modules / collections / endpoints 的合并体。
--
-- 每个树节点一行，由 `type` 区分。`parent_id` 是唯一的层级连接；`root_id` 把所属项目
-- 反范式化到每个节点上，故整棵树是一次走索引的读，而不是逐节点沿父链上溯。
--
-- 项目刻意**不**是节点。项目是隐式的根上下文——UI 恰好绑定一个激活项目——故 `root_id`
-- 保持单值，模块的父以 NULL 表达。
--
-- 纯增量，故 SCHEMA_VERSION 不需要 bump——见它在 db/mod.rs 里的文档注释。
-- ===========================================================================

CREATE TABLE IF NOT EXISTS nodes (
  id          TEXT PRIMARY KEY,
  -- 所属项目。级联，故删项目仍在单条 DELETE 里带走整棵树。
  root_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- 模块为 NULL：其父是项目，而项目不是节点。自引用的 CASCADE 提供了旧三级外键链
  -- 给过的子树拆除。
  parent_id   TEXT REFERENCES nodes(id) ON DELETE CASCADE,
  type        TEXT NOT NULL CHECK(type IN ('module','collection','endpoint')),
  name        TEXT NOT NULL,
  description TEXT,
  -- 按父而非 (父, type) 划定作用域。同一个父下的子节点反正都是同一类型，故任何按显示序
  -- 读的东西行为都不变。
  sort_order  INTEGER NOT NULL,
  -- 模块与 collection 此前没有时间戳；放在这里不花代价，还让每条 CRUD 路径省掉一次
  -- 按类型分支。
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
-- 树的两个读取：「这个项目的整棵树」与「这个节点的子节点，按序」。
CREATE INDEX IF NOT EXISTS idx_nodes_root   ON nodes(root_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_nodes_type   ON nodes(type, name);

-- endpoint 的配置，拆出来是因为这里没有一列对模块或 collection 有意义。折叠进 `nodes`
-- 会让每个模块和 collection 行带上一组永远为空的列，也会让搜索里的 `method = ?`
-- 过滤无物可分。
--
-- `node_id` 既是主键也是连接键。这里没有 name、sort_order 或时间戳：那些在 `nodes` 上
-- 只住一份。
CREATE TABLE IF NOT EXISTS endpoint_details (
  node_id                TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
  method                 TEXT NOT NULL,
  -- 只有路径。host 在发送时取自环境的 moduleBaseUrls，故一个 endpoint 可改指向而无需重写。
  url                    TEXT NOT NULL,
  environment_id         TEXT,
  base_url_definition_id TEXT,
  base_url_override      TEXT,
  params_json            TEXT NOT NULL DEFAULT '[]',
  -- URL 路径里的占位符（`/users/{id}` 的 `id`）与它们各自的值。与 `params_json` 分开：
  -- 查询参数进查询串，路径参数要替换进路径本身。
  -- 纯增量：`db::ADDED_COLUMNS` 会给既有库补上这一列，故不 bump `SCHEMA_VERSION`。
  path_params_json       TEXT NOT NULL DEFAULT '[]',
  headers_json           TEXT NOT NULL DEFAULT '[]',
  body_json              TEXT NOT NULL DEFAULT '{}',
  auth_json              TEXT NOT NULL DEFAULT '{}',
  docs                   TEXT,
  notes                  TEXT,
  tags_json              TEXT NOT NULL DEFAULT '[]',
  -- 请求脚本。端点的两段脚本住在这一行（与 body / headers 同级），collection 的住在
  -- `node_scripts` 表。放这里而不是 `nodes`：脚本对模块没有意义，而 `nodes` 同时装
  -- module / collection / endpoint。
  --
  -- NULL = 没有脚本。空串在链装配时同样视作「没有」。
  --
  -- 纯增量：`db::ADDED_COLUMNS` 会给既有库补上这两列，故不 bump `SCHEMA_VERSION`。
  pre_request_script     TEXT,
  test_script            TEXT
);

-- collection 节点上的两段脚本。
--
-- 端点的脚本在 `endpoint_details` 里（那里只对 endpoint 存在一行），collection 塞不进去，
-- 故另起一张表。`node_id` 既是主键也是回到 `nodes` 的连接键，级联删除：删掉一个
-- collection，它的脚本随之消失。
--
-- 纯增量新表：`CREATE TABLE IF NOT EXISTS` 已覆盖既有库，不需要 `ADDED_COLUMNS` 条目，
-- 也不 bump `SCHEMA_VERSION`。
CREATE TABLE IF NOT EXISTS node_scripts (
  node_id            TEXT PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
  pre_request_script TEXT,
  test_script        TEXT
);

-- 只有 collection 能带脚本。
--
-- `db::node_scripts::set` 已经查过一次节点类型，这是第二道防线：MCP 工具与将来的调用者
-- 都可能绕过应用层，故把规则也放进库里。
--
-- 刻意**不**在此覆盖的：脚本内容本身。空串与 NULL 都表示「没有脚本」，由应用层归一。
DROP TRIGGER IF EXISTS trg_node_scripts_collection_only;

CREATE TRIGGER trg_node_scripts_collection_only
BEFORE INSERT ON node_scripts
WHEN (SELECT type FROM nodes WHERE id = NEW.node_id) IS NOT 'collection'
BEGIN
  SELECT RAISE(ABORT, 'node_scripts: only collections carry scripts');
END;

-- 一条已持久化的 HTTP 响应，按 endpoint 保留，好让响应历史条带挺过重启。
--
-- `endpoint_id` 级联，而 `nodes.parent_id` / `nodes.root_id` 又依次级联，故删掉一个
-- endpoint——或一路向上的任一祖先，直到项目——会在同一条 DELETE 里带走它的历史。
-- 刻意没有应用层的清理逻辑需要去记：外键就是契约。
--
-- 列表视图渲染的一切（`status`、`created_at`、`method`、`url`、`duration_ms`）都是真实
-- 列而非 JSON blob 里的字段，因为条带正是按它们排序与过滤的。`body` 在写入时被裁到
-- `domain::RESPONSE_HISTORY_MAX_BODY_BYTES`，`body_truncated` 记录发生过这件事，好让阅读器
-- 能说出来，而不是把半个文档当整个静默显示。
CREATE TABLE IF NOT EXISTS response_history (
  id              TEXT PRIMARY KEY,
  endpoint_id     TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  status          INTEGER NOT NULL,
  status_text     TEXT NOT NULL DEFAULT '',
  duration_ms     INTEGER NOT NULL DEFAULT 0,
  size_bytes      INTEGER NOT NULL DEFAULT 0,
  content_type    TEXT NOT NULL DEFAULT '',
  -- 是 key → value 而非有序对：响应到达此处时早已在 `src/lib/http.ts` 里过了
  -- `tuplesToRecord`，故渲染层持有的是 map，而 map 才是无需凭空发明一个 SQLite 无法还原
  -- 的顺序就能往返的东西。
  headers_json    TEXT NOT NULL DEFAULT '{}',
  body            TEXT NOT NULL DEFAULT '',
  body_json       TEXT,
  error           TEXT,
  method          TEXT,
  url             TEXT,
  body_truncated  INTEGER NOT NULL DEFAULT 0,
  created_at      INTEGER NOT NULL,
  -- 用户收藏了这一行。收藏的行不参与保留条数的裁剪（见 `db::response_history` 的
  -- `trim_to_cap`），故它由用户自己决定留多久。
  --
  -- 纯增量：`db::ADDED_COLUMNS` 会给既有库补上这一列，故不 bump `SCHEMA_VERSION`。
  is_favorite     INTEGER NOT NULL DEFAULT 0,
  -- 那次发送出去的请求（URL / header / body），JSON 编码。写这一列之前就有的行没有它，
  -- 故可以为 NULL。存的是**解析之后**的那一份：线上真正发出去的地址与头。
  request_json    TEXT
);
-- 这张表唯一的读取：历史条带问的是**一个** endpoint 的行，而这也是渲染层唯一发出的查询。
--
-- `rowid` 刻意**不**进索引，尽管 ORDER BY 用它决胜——SQLite 在索引定义里拒绝 `rowid`
--（"no such column: rowid"），绕开它也没有收益。它留下的排序每 endpoint 最多跑十行，
-- 因为让这张表保持那么小的是 `db::response_history::append`。
CREATE INDEX IF NOT EXISTS idx_response_history_endpoint
  ON response_history(endpoint_id, created_at DESC);

-- 刻意 DROP 而非留着。`CREATE INDEX IF NOT EXISTS` 无法移除任何东西，否则由曾经持有这个
-- 项目范围索引的构建写下的库会永远留着它。删索引不是 schema 版本事件：没有行被碰到，
-- `user_version` 不动，`ensure_current_schema` 也不参与。
DROP INDEX IF EXISTS idx_response_history_root;

-- ---------------------------------------------------------------------------
-- oauth2_tokens —— 换来的 access token 落在这里，让「按需刷新」跨进程成立
-- ---------------------------------------------------------------------------
--
-- 主键刻意是配置的哈希（`oauth2::auth_key`）而不是 `endpoint_id`：token 属于**这份
-- OAuth2 配置**，不属于某个端点。两个端点填同一份配置共用一个 token；改了
-- token URL / scope / client secret 则哈希变、拿不到旧 token，于是不会把一个 scope
-- 不同的旧 token 发给新的服务端——按 endpoint_id 存就会踩这个坑。
--
-- 表里**只有** token 本身。取 token 所需的凭据在 `endpoint_details.auth_json` 里，
-- 这里不复制一份，故也不存在两处凭据漂移的可能。
--
-- 纯增量新表：不 bump `SCHEMA_VERSION`（理由同上面 `response_history` 一段），
-- 既有库在下次 `db::migrate` 白得它。
CREATE TABLE IF NOT EXISTS oauth2_tokens (
  auth_key       TEXT PRIMARY KEY,
  access_token   TEXT NOT NULL,
  refresh_token  TEXT,
  -- RFC 6749 §7.1 的 `token_type`，多数 IdP 回 `Bearer`，少数回 `MAC`。写进表里
  -- 是因为刷新响应**可能**不重复它，届时仍要用旧值拼 `Authorization`。
  token_type     TEXT NOT NULL DEFAULT 'Bearer',
  scope          TEXT,
  -- Unix 秒。NULL = 不知道何时过期（IdP 没给 `expires_in`），此时一直视为有效，
  -- 只能由 UI 的「重新获取」替换。
  expires_at     INTEGER,
  obtained_at    INTEGER NOT NULL
);

-- ---------------------------------------------------------------------------
-- dynamic_variables —— 一条动态变量的定义
-- ---------------------------------------------------------------------------
--
-- 一条声明：变量名 → 数据源 → 抽取规则 → 缓存时长。消费端点的请求体里写 `{{sns}}`，
-- 发送时程序按本表取数、抽取并替换。
--
-- 作用域是**全局**：定义不属于任何项目，任何项目的端点都能引用它，整个工作区里变量名
-- 唯一（下面那条唯一索引）。理由是基础数据源不该在每个项目里各写一遍——基线项目取到的
-- 那份站点清单，多客户分发项目直接引用同一条定义即可。
--
-- 数据源二选一：`source_endpoint_id` 引用树里的一个端点（复用那个端点自己的 base URL、
-- 鉴权与前置脚本），`source_inline_json` 是一份内联请求。两个都为空即「没有数据源」，
-- 解析时报 `source_error`。
--
-- `source_endpoint_id` 用 `ON DELETE SET NULL` 而不是 `CASCADE`：数据源端点被删掉时，
-- 定义本身要留下来并标成「数据源已删除」，而不是跟着消失——用户删错一个端点不该连带
-- 丢掉一份定义。
--
-- 取数用的环境与服务由定义自己点名：`source_environment_id` 为空表示用数据源接口所属
-- 项目的第一个可用环境，`source_base_url_definition_id` 为空表示用该项目的默认服务（★）。
-- 取数上下文于是只由定义决定，与谁消费它无关——同一个 `{{name}}` 在任何项目里取到的值
-- 都一样。
CREATE TABLE IF NOT EXISTS dynamic_variables (
  id                 TEXT PRIMARY KEY,
  name               TEXT NOT NULL,
  description        TEXT,
  -- 数据源：引用树里的一个端点，或内联一份请求。两者二选一。
  source_endpoint_id TEXT REFERENCES nodes(id) ON DELETE SET NULL,
  source_inline_json TEXT,
  -- 取数用的环境。为空 = 数据源接口所属项目的第一个可用环境。
  source_environment_id TEXT REFERENCES environments(id) ON DELETE SET NULL,
  -- 取数用的服务。为空 = 数据源接口所属项目的默认服务（★）。
  source_base_url_definition_id TEXT REFERENCES base_url_definitions(id) ON DELETE SET NULL,
  -- JSONPath 表达式。`$` 表示整段响应。
  extract_path       TEXT NOT NULL DEFAULT '$',
  -- 值的序列化形态：`auto` / `json` / `text`。
  value_mode         TEXT NOT NULL DEFAULT 'auto',
  -- 缓存有效期（秒）。
  ttl_seconds        INTEGER NOT NULL DEFAULT 60,
  -- 取数失败时是否允许用已过期的缓存值兜底。默认不用。
  stale_fallback     INTEGER NOT NULL DEFAULT 0,
  enabled            INTEGER NOT NULL DEFAULT 1,
  sort_order         INTEGER NOT NULL DEFAULT 0,
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
-- 变量名在整个工作区里唯一：冲突要在写入处报错，而不是静默覆盖另一条定义。
CREATE UNIQUE INDEX IF NOT EXISTS idx_dynamic_vars_name
  ON dynamic_variables(name);

-- ---------------------------------------------------------------------------
-- dynamic_variable_cache —— 动态变量的当前值
-- ---------------------------------------------------------------------------
--
-- 一行 = 一个（定义 × 取数环境）。`environment_id` 为空串表示「没有环境」。
--
-- 取数环境由定义自己决定（点名的那个，或数据源接口所属项目的第一个可用环境），
-- 故一条定义只在一个环境上取值。这一列仍在键里：定义改了环境之后，旧环境上取到的那份
-- 值留在原处，用户改回去时那份值还在。
--
-- 存放位置是 SQLite 而不是进程内存：桌面端、`serve-web`、MCP 守护进程是三个独立进程，
-- 各自的内存缓存互不可见；持久化之后三者读写同一份缓存。
--
-- 表里不记 `value_mode`：值的序列化形态由定义决定，与缓存无关，故改 `value_mode`
-- 不必让缓存失效。
--
-- `is_override = 1` 表示这个值是用户手动改的，不是自动取回来的。覆盖值带自己的过期
-- 时间；`override_expires_at` 为 NULL 表示「一直有效，直到手动清除」。
CREATE TABLE IF NOT EXISTS dynamic_variable_cache (
  definition_id       TEXT NOT NULL REFERENCES dynamic_variables(id) ON DELETE CASCADE,
  environment_id      TEXT NOT NULL DEFAULT '',
  value_json          TEXT,
  fetched_at          INTEGER NOT NULL,
  expires_at          INTEGER NOT NULL,
  is_override         INTEGER NOT NULL DEFAULT 0,
  override_expires_at INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_dynamic_cache_key
  ON dynamic_variable_cache(definition_id, environment_id);

-- 父子矩阵的第二道防线。
--
-- `db::nodes` 用 `ResourceNodeType::may_under` 检查每一次写入，但 MCP 工具是本项目主要的
-- 外部写路径，绕过 `db::nodes` 的将来调用者否则可以自由搭出一棵没有读者走得动的树。
-- 三表布局是从外键拿到整个矩阵的——`endpoints.collection_id REFERENCES collections(id)`
-- 是其中之一——而单一多态 `parent_id` 表达不了：SQLite 的 CHECK 约束读不到别的行，
-- 自引用的外键也说不清哪些类型能坐在哪些类型之下。
--
-- 这两个触发器在每次 `migrate` 时 DROP 再建，而不是 `IF NOT EXISTS`。触发器体是契约的
-- 一部分，而 `IF NOT EXISTS` 会把第一个碰到某个 `store.db` 的构建写下的东西永久冻结：
-- 为让 endpoint 能坐在模块下而放宽矩阵，会让每个既有库继续执行应用层已不再认同的旧规则。
-- 改为 bump `SCHEMA_VERSION` 则会把那些库直接删掉，而为了放宽一个谓词丢掉用户的树
-- 不是值得做的交易。
--
-- 刻意**不**在此覆盖的：`root_id` 与父的一致性。`move_many` 会为整棵子树重新推导它，
-- 而子树是一次改写一行的——故父子在事务中途短暂不一致是合法的，约束恰好在最需要它的
-- 时刻必须被撤下。创建时由 `db::nodes::validate_placement` 检查，此后归 `move_many` 管。
DROP TRIGGER IF EXISTS trg_nodes_parent_matrix_insert;
DROP TRIGGER IF EXISTS trg_nodes_parent_matrix_update;

CREATE TRIGGER trg_nodes_parent_matrix_insert
BEFORE INSERT ON nodes
BEGIN
  -- 模块的父是项目，而项目不是节点。
  SELECT RAISE(ABORT, 'nodes: a module has no node parent')
  WHERE NEW.type = 'module' AND NEW.parent_id IS NOT NULL;

  SELECT RAISE(ABORT, 'nodes: parent type does not accept this child type')
  WHERE NEW.parent_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM nodes p
    WHERE p.id = NEW.parent_id
      AND (
        (NEW.type = 'collection' AND p.type IN ('module', 'collection'))
        OR (NEW.type = 'endpoint'   AND p.type IN ('module', 'collection'))
      )
  );
END;

CREATE TRIGGER trg_nodes_parent_matrix_update
BEFORE UPDATE OF parent_id ON nodes
-- NULL 表示「提升到项目顶层」，那只对模块合法——由第一条子句处理。其余类型都需要一个父，
-- 而下面的 NOT EXISTS 也把隐式外键的「父不存在」情形变成了中止。
WHEN NEW.parent_id IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'nodes: parent type does not accept this child type')
  WHERE NOT EXISTS (
    SELECT 1 FROM nodes p
    WHERE p.id = NEW.parent_id
      AND (
        (NEW.type = 'collection' AND p.type IN ('module', 'collection'))
        OR (NEW.type = 'endpoint'   AND p.type IN ('module', 'collection'))
      )
  );
END;

-- schema 版本标记，在发出任何连接之前由 `db::ensure_current_schema` 读回。须与
-- `db::SCHEMA_VERSION` 保持同步——那里的不一致由 `schema_version_matches_the_constant`
-- 抓住。
--
-- 版本历史：
--   0  版本号之前（原 modules/collections/endpoints 三表布局）。任何标记为 0 的文件
--      当前构建读不了，会被从零重建。
--   2  `nodes` / `endpoint_details` 以增量方式落地，故更旧的 v2 文件在下次
--      `CREATE TABLE IF NOT EXISTS` 时白得它们，不需要自己的版本。
--   3  当前。没有任何一列改动——改动的是 `endpoint_details.auth_json` **装的东西**。它存
--      序列化的 `domain::AuthConfig`，而撤掉 `type` → `kind` 的 serde rename（以及
--      `in` → `inLocation`）意味着 v2 的行不再反序列化得出：`kind` 是必填字段，故连 `'{}'`
--      默认值都失败。是不可读而非陈旧，故 bump 版本而不是在读者侧加兼容层。
--
-- 3 之后：服务表移到了项目，值层变成
-- `environments.module_base_urls_json` 里的 (环境 × 模块 × 服务)。**仍是**版本 3——
-- `module_base_urls_json` 是增量的、经 `db::ADDED_COLUMNS` 到达，而该模型退役的两列
-- （`base_url_definitions.module_id` / `.is_default`）被保留并中和而非删除，因为 DROP =
-- 重建 = bump 版本 = `ensure_current_schema` 删掉每个用户的库。由两种前任布局之一写下的
-- 行由 `db::mod` 里的 `migrate_base_urls_into_value_layer` 修复。
--
-- `response_history` 落地后**仍是**版本 3：新表是最纯粹的增量改动——上面每个 `CREATE`
-- 都是 `IF NOT EXISTS`，故既有库在下次 `db::migrate` 白得它，既不需要 `ADDED_COLUMNS`
-- 条目（那份列表是给「给已存在的表加列」用的）也不需要 bump 版本。
--
-- `oauth2_tokens` 同理：新表，无新列，故仍是版本 3。
--
-- `environments.disabled_variables_json` 同理：新列，经 `db::ADDED_COLUMNS` 到达，
-- 旧行读回来是空集合（= 全部启用），故仍是版本 3。
--
-- 请求脚本落地后**仍是**版本 3：`endpoint_details` 的两列是可空新列（经
-- `db::ADDED_COLUMNS` 到达），`node_scripts` 是新表（`IF NOT EXISTS` 已覆盖）。
-- bump 版本 = `ensure_current_schema` 删掉每个用户的库，而脚本功能不值得这个代价。
--
-- `dynamic_variables` 与 `dynamic_variable_cache` 同理：两张都是新表，故仍是版本 3。
--
-- 动态变量改为全局作用域后，这两张表的形状变了（去掉 `project_id`，加上环境与服务两列）。
-- 版本仍是 3：`db::schema` 在 `migrate` 里认出旧形状并把两张表丢掉重建，故旧的定义不会
-- 残留，而用户的项目、接口与环境不受影响。
--
-- 残留的 `request_settings` **表**被清掉后**仍是**版本 3。那张表（单例行，`CHECK id=1`，
-- 三列即超时 / 跟随重定向 / 响应体积上限）从建立起就没有任何读者：设置旋钮的现役容器是
-- `preferences` 表里的 `request_settings` **行**（`db::preferences::keys::REQUEST_SETTINGS`），
-- 界面与 MCP 侧读写的都是那一行。删掉一张没有读者的表不改变任何一行的可读性，故不需要
-- 版本事件；而 bump 版本会让 `ensure_current_schema` 删掉每个用户的库——为清一张空表赔上
-- 全部项目、环境与资源树。既有文件里的它由 `db::schema::migrate` 在 `SCHEMA_SQL` 之后丢掉。
PRAGMA user_version = 3;