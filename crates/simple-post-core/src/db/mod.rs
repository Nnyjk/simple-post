//! SQLite 持久化层。
//!
//! - `db::init` 打开（或创建）库文件并执行 `schema.sql`；**不种任何种子数据**——
//!   行只来自用户或 MCP 客户端。
//! - 读用 `db::with_conn`，写用 `db::with_tx`；所有 CRUD 模块只能走这两者之一。
//! - `init` 里启用 WAL：读者互不阻塞，单个写者也不阻塞读者。
//! - 库文件在 `%APPDATA%\com.simple-post\store.db`（`dirs::data_dir()`），
//!   目录和文件缺失时自动创建；首次启动建完表即止。
//! - `db::ping` 是廉价存活探针。
//!
//! `db/fixtures.json` 仅供测试：只经 `#[cfg(test)] pub(crate) mod test_support`
//! 可达，生产构建里根本不存在。见 `test_support::init_seeded`。
//!
//! 每表 CRUD 在同目录 `db/*.rs`，全部接收 `&mut rusqlite::Connection`，
//! 由调用方决定是否在事务内。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use r2d2::ManageConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OpenFlags};

use crate::error::AppError;

// 子模块：对外公开，让编排层 / Tauri commands 可直接调用单个 CRUD 函数。
pub mod base_url_defs;
pub mod endpoint_details;
pub mod environments;
pub mod nodes;
pub mod permissions;
pub mod preferences;
pub mod projects;
pub mod response_history;

// 公开类型。

/// 共享连接池。`DbPool` 只是 `Arc` 包裹的廉价克隆，可随意传递。
/// 池大小固定 4：SQLite 本就串行化写入，池更大只增加争用、不增吞吐。
pub type DbPool = r2d2::Pool<SqliteConnectionManager>;

// schema 版本。

/// 本构建读写的布局，由 `schema.sql` 末尾写入 `PRAGMA user_version`。
///
/// **仅当旧版本文件无法满足新形状时才可 bump**（删列/改名、一表拆多表）。纯增量的
/// 改动（新表、可空新列）不需要：`CREATE TABLE IF NOT EXISTS` 已覆盖，缺列靠
/// [`ADDED_COLUMNS`] 补。
///
/// 改 `domain.rs` 里 auth 的字段名同样要 bump：`auth_json` 存的是 serde 原样输出，
/// 改名后旧行反序列化不出来，文件即不可读。
///
/// 版本号历史写在 `schema.sql` 末尾，改动请同步。
pub const SCHEMA_VERSION: i64 = 3;

// 公开 API。

/// 打开（或创建）`<data_dir>/store.db`，升到 [`SCHEMA_VERSION`]，返回连接池。
///
/// `data_dir` 通常是 `dirs::data_dir().unwrap().join("com.simple-post")`，缺失即创建。
///
/// 旧布局的文件是**替换而非升级**（见 [`ensure_current_schema`]）：本应用没有迁移路径，
/// 故只记日志，不在每次启动时打扰用户。
pub fn init(data_dir: &Path) -> Result<DbPool, AppError> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("store.db");

    // 先于任何连接发出：陈旧文件上开池，读会成功但表已非本构建所理解。
    let rebuilt = ensure_current_schema(&db_path)?;

    let manager = SqliteConnectionManager::file(&db_path)
        .with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)
        // WAL 跨池连接生效：每条新连接继承首条连接设置的日志模式。显式设一次，
        // 免得冷启动时读写互相竞态。
        .with_init(|c| {
            // PRAGMA 无法跨连接携带用户参数，故走裸语句路径。
            c.execute_batch(
                "PRAGMA journal_mode = WAL;\
                 PRAGMA synchronous = NORMAL;\
                 PRAGMA foreign_keys = ON;\
                 PRAGMA busy_timeout = 3000;",
            )
        });

    // 提前探一次文件：损坏/只读要在调用方接好其余部分之前就失败。
    let mut probe = manager.connect()?;
    migrate(&mut probe)?;
    drop(probe);

    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(manager)
        .map_err(AppError::Pool)?;

    // 新建池的首次 checkout 会真正建连接——锁文件/坏路径在这里暴露。
    ping(&pool)?;

    if rebuilt {
        tracing::warn!(
            "database schema v{SCHEMA_VERSION}: replaced an older store.db at {} — \
             its contents are gone",
            db_path.display()
        );
    }

    Ok(pool)
}

/// 在池中取一条连接执行闭包。只读用它；写优先用 `with_tx`。
///
/// PRAGMA（foreign_keys、journal_mode 等）由池的 `with_init` 钩子逐连接应用，
/// 故每次 checkout 拿到的都是配置好的句柄。
pub fn with_conn<F, R>(pool: &DbPool, f: F) -> Result<R, AppError>
where
    F: FnOnce(&mut Connection) -> Result<R, AppError>,
{
    let mut conn = pool.get()?;
    f(&mut conn)
}

/// 在写事务内执行闭包，`SQLITE_BUSY` 重试 3 次、间隔 50 ms：够等其他写者提交，
/// 又不至于卡顿；3 次仍失败则作为 `AppError::Db` 上抛。
///
/// 不用 rusqlite 的 `transaction()`：它是 `BEGIN DEFERRED`，`SQLITE_BUSY` 会拖到
/// **首次写入**才暴露。要的是一进事务就取锁，即 `BEGIN IMMEDIATE`。
pub fn with_tx<F, R>(pool: &DbPool, f: F) -> Result<R, AppError>
where
    F: Fn(&mut Connection) -> Result<R, AppError>,
{
    const RETRIES: u32 = 3;
    const BACKOFF: Duration = Duration::from_millis(50);

    let mut last_err: Option<AppError> = None;
    for attempt in 0..RETRIES {
        let mut conn = pool.get()?;
        match run_immediate_tx(&mut conn, &f) {
            Ok(v) => return Ok(v),
            Err(e) => {
                let busy = matches!(
                    &e,
                    AppError::Sqlite(rusqlite::Error::SqliteFailure(err, _))
                        if err.code == rusqlite::ErrorCode::DatabaseBusy
                );
                if !busy {
                    return Err(e);
                }
                last_err = Some(e);
                if attempt + 1 < RETRIES {
                    std::thread::sleep(BACKOFF);
                }
            }
        }
    }
    Err(match last_err {
        Some(e) => AppError::Db(format!("sql busy after {RETRIES} retries: {e}")),
        None => AppError::Db("sql busy".into()),
    })
}

/// 执行内嵌 `schema.sql`，把既有库补到当前列集，再修复旧布局写的行。幂等，每次启动可调。
///
/// 所有 `CREATE` 都是 `IF NOT EXISTS`，对已迁移的库是空操作。父子矩阵触发器是唯一例外：
/// 先 DROP 再建——触发器体**就是**契约，`IF NOT EXISTS` 会让老库继续执行应用层已放弃的矩阵。
/// **不插入任何行**：全新安装是空库，由用户或 MCP 客户端填充；测试种子在 `test_support`。
pub fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    conn.execute_batch(SCHEMA_SQL)?;
    // 必须在数据迁移之前、且不能放进 `SCHEMA_SQL`：那个 pass 要读写的
    // `module_base_urls_json` / `module_id` / `is_default` 在够旧的库里根本不存在，
    // 而 `SCHEMA_SQL` 的 `CREATE TABLE IF NOT EXISTS` 对这些库正是空操作——
    // 所以列只能在这里补。
    add_missing_columns(conn)?;
    migrate_base_urls_into_value_layer(conn)
}

/// 本项目某个构建写过、但并非每个构建都有的列。
///
/// `CREATE TABLE IF NOT EXISTS` 对已存在的表是空操作，后加的列因此永远到不了已有文件；
/// 在此补列是那些文件唯一的补救途径。幂等性靠问 SQLite 而非记账：已存在的列被跳过，
/// 故每次启动都跑、第二次无事发生。
///
/// **刻意不 bump `SCHEMA_VERSION`**：它不是迁移计数器，[`ensure_current_schema`] 遇到
/// 不匹配会**删库**——一个只是拓宽表的补丁版本会清空所有用户的项目、环境与资源树。
/// 「纯增量改动不 bump」的承诺，全靠 `schema.sql` 的 `IF NOT EXISTS` 加上这一遍。
///
/// 首项是现役值层。末两项是已下线的「服务挂在模块上」模型，只因数据迁移必须能**读**
/// 那个布局才留着——由 `SCHEMA_SQL` 建出这三列的库与由本列表补出的库，对迁移是同一种形状，
/// 两者都必须可修复。
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    (
        "environments",
        "module_base_urls_json",
        "TEXT NOT NULL DEFAULT '{}'",
    ),
    (
        "base_url_definitions",
        "module_id",
        // 可空：`ALTER TABLE ... ADD COLUMN` 只能追加带默认值的列，而外键需要默认值；
        // 该形状下每行的这一列本来就是 NULL。
        "TEXT REFERENCES nodes(id) ON DELETE CASCADE",
    ),
    (
        "base_url_definitions",
        "is_default",
        "INTEGER NOT NULL DEFAULT 0",
    ),
];

/// 为 [`ADDED_COLUMNS`] 中缺列的表补上该列。
///
/// `PRAGMA table_info` 扫描刻意直白：`schema.sql` 尚未创建的表不返回行，于是被跳过，
/// 而不是让整个启动失败。
fn add_missing_columns(conn: &mut Connection) -> Result<(), AppError> {
    for (table, column, decl) in ADDED_COLUMNS {
        if table_has_column(conn, table, column)? {
            continue;
        }
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
        tracing::info!("schema: added {table}.{column} to an existing store");
    }
    Ok(())
}

/// `table` 是否已带 `column` 列？
fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, AppError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

// 数据迁移：服务表回到项目级，URL 落到 (环境 × 模块 × 服务)。
//
// 这是数据修复而非版本变更：`SCHEMA_VERSION` 刻意不动（bump 会让
// `ensure_current_schema` 删库），列由 [`ADDED_COLUMNS`] 补齐，本 pass 修复旧布局写的行。
//
// **两种旧布局都在野外的库里存在**，按证据逐项目判定，绝不假定：
// - shape B：每个 (模块 × name) 一行，值在 `base_urls_json[clone_id]`，★ 按模块记在
//   `is_default`。
// - shape A：项目级服务表，`module_id` 为 NULL，值在 `base_urls_json[def_id]`，
//   外加 `module_base_urls_json[def_id][module_id]` 的按模块覆盖。
// `module_id` 非 NULL 只可能是 B；无此行但 `base_urls_json` 有值则是 A。
// 本构建写的库两者皆无，pass 即空操作。

/// 旧布局写下的一条服务行。`module_id` 是布局判别式：`Some` = 模块持有，`None` = 项目持有。
struct ServiceRow {
    id: String,
    name: String,
    module_id: Option<String>,
    sort_order: i64,
    /// 模块持有模型按行记的 ★。shape A 无此标记，故此处恒为 false。
    is_default: bool,
}

/// 项目的一个模块，即「第一个模块」的含义：显示序 → 创建时间 → id，
/// 让选择跨次运行稳定，而不是取决于扫描顺序。
struct ModuleRow {
    id: String,
    sort_order: i64,
    created_at: i64,
}

impl ModuleRow {
    /// 显示序，写作 `(sort_order, created_at, id)`。
    fn rank(&self) -> (i64, i64, &str) {
        (self.sort_order, self.created_at, self.id.as_str())
    }
}

/// 两层 URL 都可读的环境行。本 crate 仅剩这一个 `base_urls_json` 的读者，且必须同时读
/// `module_base_urls_json`——shape A 的覆盖层是答案的一部分。
struct EnvRow {
    id: String,
    /// `def_id -> url`。作为 shape A 的兜底与 shape B 的来源来读；只在 shape B 走完后
    /// 被清空一次（见 [`migrate_project`]）。
    base_urls: std::collections::BTreeMap<String, String>,
    /// `def_id -> module_id -> url`，现役值层。
    module_base_urls:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// 本 pass 仍需处理的项目 id。两类独立证据取并集：仍有服务指名模块 ⇒ shape B；
/// 仍有服务级 URL 的环境 ⇒ shape A。两者皆无即已是现役形状，严格不碰——
/// 这也是首轮之后每次启动都零成本的原因。
fn pending_projects(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut pending: std::collections::BTreeSet<String> = Default::default();

    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT project_id FROM base_url_definitions \
             WHERE module_id IS NOT NULL",
        )?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?;
        pending.extend(ids);
    }

    for env in read_envs_needing_repair(conn)? {
        pending.insert(env);
    }

    Ok(pending.into_iter().collect())
}

/// `base_urls_json` 仍有内容的环境所属项目 id。
/// 解析而非与 `'{}'` 比较：仅空白或键序不同的 blob 也算证据。
fn read_envs_needing_repair(conn: &mut Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT project_id, base_urls_json FROM environments \
         WHERE base_urls_json IS NOT NULL AND base_urls_json <> '{}'",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (project_id, raw) = r?;
        match serde_json::from_str::<std::collections::BTreeMap<String, String>>(&raw) {
            Ok(map) if !map.is_empty() => out.push(project_id),
            Ok(_) => {}
            // 解析不了就没东西可搬：否则会为一个本构建已当死列的字段拒绝启动。
            Err(e) => tracing::warn!(
                "environments.base_urls_json is not readable ({e}); \
                 migrating this environment without its service-level URLs"
            ),
        }
    }
    Ok(out)
}

/// 把每个项目的服务搬回项目级，URL 搬进 `module_base_urls_json[def_id][module_id]`。
/// 所有项目共用一个事务：中途崩溃则整遍回滚，下次启动从同一初始状态重来，
/// 不存在半迁移形状。
fn migrate_base_urls_into_value_layer(conn: &mut Connection) -> Result<(), AppError> {
    let pending = pending_projects(conn)?;
    if pending.is_empty() {
        return Ok(());
    }

    // 手写 `BEGIN IMMEDIATE` / `COMMIT` 而非 `conn.transaction()`，理由同
    // [`run_immediate_tx`]：`rusqlite::Transaction` 只把不可变的连接借用交回来，
    // 而下面每个 helper 都要本模块通用的 `&mut Connection`。用 IMMEDIATE 而非默认的
    // deferred，是让并发启动在 BEGIN 处就失败，而不是改写改到一半才失败。
    conn.execute_batch("BEGIN IMMEDIATE")?;
    for project_id in &pending {
        if let Err(e) = migrate_project(conn, project_id) {
            // 尽力回滚——原始错误才有诊断价值。
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    conn.execute_batch("COMMIT")?;
    tracing::info!(
        "schema: moved {} project service list(s) back onto the project \
         and their URLs into module_base_urls",
        pending.len()
    );
    Ok(())
}

/// [`migrate_base_urls_into_value_layer`] 的逐项目一半。
fn migrate_project(conn: &mut Connection, project_id: &str) -> Result<(), AppError> {
    let modules = read_modules(conn, project_id)?;
    let services = read_services(conn, project_id)?;
    if services.is_empty() {
        return Ok(());
    }
    let envs = read_envs(conn, project_id)?;

    // shape B = 「还有行持有模块」。逐项目判定，才让从未被模块持有版碰过的库走保 id 的
    // 廉价路径；且这是唯一可用证据——两种布局的 `base_urls_json` 都是有值的。
    let module_owned = services.iter().any(|s| s.module_id.is_some());

    // 在任何改写之前读出项目自己的 ★：shape A 保留它（endpoint 本就按它解析），
    // shape B 已无处可留（那个模型把 ★ 移到了模块上）。
    let star_before: Option<String> = conn.query_row(
        "SELECT default_base_url_definition_id FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )?;

    let canonical = pick_canonical(&services, &modules, module_owned);
    // clone id -> 存活的 canonical id，仅 shape B：指名 clone 的 endpoint 要改指到留下的那一行。
    let survivor_of: std::collections::HashMap<&str, &str> = canonical
        .iter()
        .filter(|c| !c.replaced.is_empty())
        .flat_map(|c| {
            c.replaced
                .iter()
                .map(move |old| (old.as_str(), c.id.as_str()))
        })
        .collect();

    write_value_layer(conn, &envs, &modules, &canonical, &services, module_owned)?;

    if module_owned {
        remap_endpoints_onto_canonical(conn, project_id, &survivor_of)?;
    }

    // ★：shape A 保留原指向（NULL 就保持 NULL——凭空造一个默认，等于给原本解析为 `''`
    // 的 endpoint 一个它从未有过的 host）；shape B 必须选，因为按模块的 ★ 即将消失，
    // 见 [`choose_star_for_module_owned`]。
    let star = if module_owned {
        choose_star_for_module_owned(&services, &modules, &canonical)
    } else {
        star_before.filter(|d| canonical.iter().any(|c| c.id == *d))
    };
    conn.execute(
        "UPDATE projects SET default_base_url_definition_id = ?1 WHERE id = ?2",
        params![star, project_id],
    )?;

    // 先写值层再退场 clone：读的时候那些 id 还得可解析。
    for c in &canonical {
        for old in &c.replaced {
            conn.execute(
                "DELETE FROM base_url_definitions WHERE id = ?1",
                params![old],
            )?;
        }
    }
    conn.execute(
        "UPDATE base_url_definitions SET module_id = NULL, is_default = 0 \
         WHERE project_id = ?1",
        params![project_id],
    )?;

    // 仅 shape B：`base_urls_json` 就是值层，其中每个值都已进 `module_base_urls_json`
    // （各 clone 的 URL 归到它自己的模块，剩余键指向已不存在的行）。留着不只是脏：
    // 下次启动看到没有 `module_id` 的库会判定为 shape A，于是把 `base_urls[canonical]`
    // 展开进每个模块的空单元格——模块 B 被填上模块 A 的 host，且每次启动都重来。
    // 清空它才使这一形状幂等。shape A 不动：它的键就是自己的 def id，之后每遍都重新
    // 推导出逐字节相同的值，是唯一残留 blob 真正无害的布局。
    if module_owned {
        conn.execute(
            "UPDATE environments SET base_urls_json = '{}' WHERE project_id = ?1",
            params![project_id],
        )?;
    }
    Ok(())
}

/// 迁移后存活的服务行，加上它顶替的那些 id。
struct CanonicalService {
    id: String,
    name: String,
    sort_order: i64,
    /// 被本行顶替的 per-module clone id（仅 shape B）。为空表示没有折叠，
    /// shape A 每行皆如此。
    replaced: Vec<String>,
}

/// 决定每个服务名由哪一行代表。
///
/// - shape A：每行已是项目级，全部存活、无需合并，故重命名不是合并——同 `name` 两行
///   是应用层自己造不出的状态。
/// - shape B：每 (模块 × name) 一行，须收敛为每 name 一行。存活者取**第一个**模块下的
///   那行（显示序 → 创建时间 → id）：那是项目配置最多的模块，换一个就得有一条
///   「哪个模块赢」的规则，而数据里没有依据。同模块内以该行自身显示序决胜。
/// - 无模块的行排在所有真模块之后，只在该 name 仅剩它一行时胜出——混合库里模块持有版
///   在旧行折叠前新建服务会留下这种状态。
fn pick_canonical(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    module_owned: bool,
) -> Vec<CanonicalService> {
    if !module_owned {
        return services
            .iter()
            .map(|s| CanonicalService {
                id: s.id.clone(),
                name: s.name.clone(),
                sort_order: s.sort_order,
                replaced: Vec::new(),
            })
            .collect();
    }

    let rank_of = |module_id: Option<&str>| -> (u8, (i64, i64, &str)) {
        match module_id.and_then(|m| modules.iter().find(|x| x.id == m)) {
            // 有模块的行排在无主残留之前。
            Some(m) => (0, m.rank()),
            None => (1, (0, 0, "")),
        }
    };

    let mut by_name: std::collections::BTreeMap<&str, &ServiceRow> = Default::default();
    for s in services {
        let entry = by_name.entry(s.name.as_str()).or_insert(s);
        let better = rank_of(s.module_id.as_deref()) < rank_of(entry.module_id.as_deref())
            // 同模块（或都无主）时退回该行自身的显示序，使选择不依赖扫描顺序。
            || (rank_of(s.module_id.as_deref()) == rank_of(entry.module_id.as_deref())
                && (s.sort_order, s.id.as_str()) < (entry.sort_order, entry.id.as_str()));
        if better {
            *entry = s;
        }
    }

    by_name
        .values()
        .map(|keep| CanonicalService {
            id: keep.id.clone(),
            name: keep.name.clone(),
            sort_order: keep.sort_order,
            replaced: services
                .iter()
                .filter(|s| s.name == keep.name && s.id != keep.id)
                .map(|s| s.id.clone())
                .collect(),
        })
        .collect()
}

/// 模块持有模型下，谁成为项目的 ★。
///
/// 取第一个模块的 ★——最可能就是用户本意，它的请求正是从这里发出的。**两模块冲突时没有
/// 正确答案**：该模型给每个模块独立默认值，从未把两者关联，既无继承可循，也无多数可问；
/// 「第一个模块胜出」是明示的决胜规则，不是还原本意。另一个选择（不设 ★）会静默剥掉每个
/// 未指名服务的 endpoint 的 base URL。首个模块自己无 ★ 时，退回 `sort_order` 最小的服务，
/// 即选择器里排在最前的那个。
fn choose_star_for_module_owned(
    services: &[ServiceRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
) -> Option<String> {
    let first_module = modules.iter().min_by(|a, b| a.rank().cmp(&b.rank()));
    if let Some(first) = first_module {
        let stars: Vec<&ServiceRow> = services
            .iter()
            .filter(|s| s.is_default && s.module_id.as_deref() == Some(first.id.as_str()))
            .collect();
        if let Some(pick) = stars
            .iter()
            .min_by(|a, b| (a.sort_order, &a.id).cmp(&(b.sort_order, &b.id)))
        {
            // ★ 自身可能就是 clone，此时项目默认须指名存活的行，而不是即将被删的那一行。
            if let Some(keep) = canonical
                .iter()
                .find(|c| c.name == pick.name)
                .map(|c| c.id.clone())
            {
                return Some(keep);
            }
        }
    }
    canonical
        .iter()
        .min_by_key(|c| c.sort_order)
        .map(|c| c.id.clone())
}

/// 把每个现役 URL 复制进 `module_base_urls_json[def_id][module_id]`。
///
/// 单元格的值就是**旧**模型为该 (服务 × 模块) 对解析出的值，不多不少：
/// - shape A：有按模块覆盖就用覆盖，否则用环境级 URL——正是旧解析的顺序。
/// - shape B：同名 clone 在该模块下的 URL；该模块没有此服务则缺席，这正是要点：
///   空单元格表示「此模块对此服务无地址」，不得从兄弟模块回填。
///
/// 已有值的单元格保持不变，故本 pass 只增不改、重跑无事。两边都无值时**不写**该单元格：
/// 「未设置」是 UI 会显示、解析器会当作无 base URL 的状态，物化成 `""` 会让两者在存储的
/// 数据里无法区分。
fn write_value_layer(
    conn: &mut Connection,
    envs: &[EnvRow],
    modules: &[ModuleRow],
    canonical: &[CanonicalService],
    services: &[ServiceRow],
    module_owned: bool,
) -> Result<(), AppError> {
    for env in envs {
        let mut cells = env.module_base_urls.clone();
        let mut changed = false;

        for c in canonical {
            for m in modules {
                if cells.get(&c.id).is_some_and(|per| per.contains_key(&m.id)) {
                    continue;
                }
                let url = if module_owned {
                    services
                        .iter()
                        .find(|s| s.name == c.name && s.module_id.as_deref() == Some(m.id.as_str()))
                        .and_then(|s| env.base_urls.get(&s.id))
                } else {
                    env.module_base_urls
                        .get(&c.id)
                        .and_then(|per| per.get(&m.id))
                        .or_else(|| env.base_urls.get(&c.id))
                };
                if let Some(url) = url {
                    cells
                        .entry(c.id.clone())
                        .or_default()
                        .insert(m.id.clone(), url.clone());
                    changed = true;
                }
            }
        }

        if changed {
            conn.execute(
                "UPDATE environments SET module_base_urls_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(&cells)?, env.id],
            )?;
        }
    }
    Ok(())
}

/// 把项目内每个 endpoint 改指到它原名所指名服务的那一行。仅 shape B——shape A 的 id 从不变。
/// 未指名服务的保持未指名：它走项目 ★，而 ★ 在同一 pass 里已改指。
/// 指名了本项目不持有的 id 的一律不动：那是跨项目引用，写路径本就拒绝，
/// 不该由数据 pass 静默改写。
fn remap_endpoints_onto_canonical(
    conn: &mut Connection,
    project_id: &str,
    survivor_of: &std::collections::HashMap<&str, &str>,
) -> Result<(), AppError> {
    if survivor_of.is_empty() {
        return Ok(());
    }
    let rows: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT e.node_id, e.base_url_definition_id \
             FROM endpoint_details e \
             JOIN nodes n ON n.id = e.node_id \
             WHERE n.root_id = ?1 AND e.base_url_definition_id IS NOT NULL \
             ORDER BY e.node_id ASC",
        )?;
        let out = stmt
            .query_map(params![project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, _>>()?;
        out
    };
    for (endpoint_id, named) in rows {
        let Some(keep) = survivor_of.get(named.as_str()) else {
            continue;
        };
        conn.execute(
            "UPDATE endpoint_details SET base_url_definition_id = ?1 WHERE node_id = ?2",
            params![keep, endpoint_id],
        )?;
    }
    Ok(())
}

fn read_modules(conn: &mut Connection, project_id: &str) -> Result<Vec<ModuleRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, sort_order, created_at FROM nodes \
         WHERE root_id = ?1 AND type = 'module' \
         ORDER BY sort_order ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ModuleRow {
            id: row.get(0)?,
            sort_order: row.get(1)?,
            created_at: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_services(conn: &mut Connection, project_id: &str) -> Result<Vec<ServiceRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, module_id, sort_order, is_default \
         FROM base_url_definitions \
         WHERE project_id = ?1 \
         ORDER BY sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(ServiceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            module_id: row.get(2)?,
            sort_order: row.get(3)?,
            is_default: row.get::<_, i64>(4)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn read_envs(conn: &mut Connection, project_id: &str) -> Result<Vec<EnvRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, base_urls_json, module_base_urls_json FROM environments \
         WHERE project_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        let base_urls: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&row.get::<_, String>(1)?)
                .map_err(|e| json_col_err("base_urls_json", e))?;
        let module_base_urls: std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, String>,
        > = match row.get::<_, String>(2) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| json_col_err("module_base_urls_json", e))?
            }
            Err(_) => Default::default(),
        };
        Ok(EnvRow {
            id: row.get(0)?,
            base_urls,
            module_base_urls,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 把磁盘上的库升到 [`SCHEMA_VERSION`]，版本不符则替换。
/// 返回 `true` 表示文件已被删除、将重建，由调用方告知数据丢失。
///
/// **没有迁移路径。** v0.1 未曾发布给真实用户，数据迁移框架是守护一个人群的投机重量；
/// 版本号之前的三表布局（`modules` / `collections` / `endpoints`）文件直接替换而非升级。
/// 这个判断一旦改变，只有本函数需要动。
fn ensure_current_schema(db_path: &Path) -> Result<bool, AppError> {
    if !db_path.exists() {
        // 全新安装——`migrate` 会创建文件并写入版本号。
        return Ok(false);
    }
    let found = read_user_version(db_path)?;
    if found == SCHEMA_VERSION {
        return Ok(false);
    }
    if found > SCHEMA_VERSION {
        // 文件来自**更新**的构建。重建会毁掉本构建无权毁掉的数据。
        return Err(AppError::Validation(format!(
            "database at {} is schema v{found}, but this build only understands \
             v{SCHEMA_VERSION}. Update Simple Post, or point \
             SIMPLE_POST_DATA_DIR somewhere else to start a separate workspace.",
            db_path.display()
        )));
    }
    remove_db_files(db_path)?;
    Ok(true)
}

/// 不取写锁地读 `PRAGMA user_version`。
/// 刻意只读打开：`init` 在池存在之前调用，读写句柄会创建我们正要决定是否删除的文件。
fn read_user_version(db_path: &Path) -> Result<i64, AppError> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// 删除数据库及其 WAL 附属文件。附属文件不是可选清理：留着 `store.db-wal` 而 `migrate`
/// 重建了新 `store.db`，SQLite 会把旧 write-ahead log 重放进新文件——「干净」的重建于是
/// 带回了本该丢掉的数据。
fn remove_db_files(db_path: &Path) -> Result<(), AppError> {
    for suffix in ["", "-wal", "-shm"] {
        let path = if suffix.is_empty() {
            db_path.to_path_buf()
        } else {
            let mut name = db_path.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            // 别的进程可能先清过了，或打开失败时附属文件本就不存在。都无妨。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // Windows 上「文件被另一进程占用」就长这样——第二个 `serve-web` 或残留的
            // daemon 还握着句柄。宁可大声失败，也不要重建一半。
            Err(e) => {
                return Err(AppError::Io(std::io::Error::new(
                    e.kind(),
                    format!("cannot remove {}: {e}", path.display()),
                )))
            }
        }
    }
    Ok(())
}

/// 存活探针。`SELECT 1` 走一次池中连接，用于启动时与 Tauri command 的健康灯。
pub fn ping(pool: &DbPool) -> Result<(), AppError> {
    let conn = pool.get()?;
    conn.execute_batch("SELECT 1")?;
    Ok(())
}

/// 执行 `BEGIN IMMEDIATE`，分派给调用方，成功 COMMIT、失败 ROLLBACK。返回调用方的值或错误。
/// 从 `with_tx` 拆出，是为了让重试循环能在不带泛型生命周期参数的情况下借用闭包；
/// 用 `Fn`（而非 `FnOnce`）才能反复调用而不移走闭包。
fn run_immediate_tx<F, R>(conn: &mut Connection, f: &F) -> Result<R, AppError>
where
    F: Fn(&mut Connection) -> Result<R, AppError>,
{
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match f(conn) {
        Ok(v) => {
            conn.execute_batch("COMMIT")?;
            Ok(v)
        }
        Err(e) => {
            // 尽力回滚——连 ROLLBACK 都失败时也保留原始错误，连接随后被丢弃或复用。
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// 内嵌静态资源：`include_str!` 使 schema 变更参与 `cargo build` 的重编判定。

const SCHEMA_SQL: &str = include_str!("schema.sql");

// 仅测试。整个模块是 `#[cfg(test)]`，只编进 `cargo test`，别处不存在。
// `db::init` 没有进这里的路径，故生产构建永远插不进种子行——全新安装是空库。
// `fixtures.json` 留在仓库里纯粹作为测试数据。

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::domain::{ResourceNode, ResourceNodeType};
    use rusqlite::params;
    use serde::Deserialize;

    /// 内嵌的 fixture 文件。纯测试数据——`db::init` 永不读取。
    const FIXTURES_JSON: &str = include_str!("fixtures.json");

    /// 解析内嵌 fixture。这里 panic 是正确的：`fixtures.json` 畸形是仓库缺陷，不是运行期状况。
    fn fixtures() -> Fixtures {
        serde_json::from_str(FIXTURES_JSON).expect("fixtures.json must parse")
    }

    /// 在 `dir` 开库并灌入内嵌 fixture 集。
    /// 供 `#[path]` 挂载的测试模块使用：播种是显式的一步，而不是 `db::init` 的副作用。
    pub(crate) fn init_seeded(dir: &Path) -> DbPool {
        let pool = init(dir).expect("init");
        seed(&pool);
        pool
    }

    /// 把 fixture 集灌进已初始化的库。
    /// 用 `with_conn` 而非 `with_tx`：`seed_fixtures` 自开事务，外层再套一个
    /// `BEGIN IMMEDIATE` 会报 "cannot start a transaction within a transaction"。
    pub(crate) fn seed(pool: &DbPool) {
        with_conn(pool, |c| seed_fixtures(c, &fixtures())).expect("seed fixtures");
    }

    /// `fixtures.json` 的顶层形状。只反序列化播种真正需要的字段，文件可增章节而不破旧
    /// 二进制（新字段须有默认值）。
    #[derive(Debug, Deserialize)]
    struct Fixtures {
        /// fixture 集打上的固定时钟，确定性——播种后的库逐字节一致。
        #[serde(default)]
        now: i64,
        #[serde(default)]
        projects: Vec<FixtureProject>,
        #[serde(default)]
        modules: Vec<FixtureModule>,
        #[serde(default)]
        collections: Vec<FixtureCollection>,
        #[serde(default)]
        endpoints: Vec<FixtureEndpoint>,
        #[serde(default)]
        environments: Vec<FixtureEnvironment>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureProject {
        id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        base_url_definitions: Vec<FixtureBaseUrlDef>,
        #[serde(default)]
        default_base_url_definition_id: Option<String>,
        #[serde(default)]
        base_url: Option<String>,
        color: String,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureBaseUrlDef {
        id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureModule {
        id: String,
        project_id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        sort_order: i64,
        /// 仅 UI 的展开状态。带入 fixture 以便渲染测试能读到真实树，永不落库。
        #[serde(default)]
        #[allow(dead_code)]
        expanded: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureCollection {
        id: String,
        module_id: String,
        #[serde(default)]
        parent_collection_id: Option<String>,
        name: String,
        #[serde(default)]
        description: Option<String>,
        sort_order: i64,
        /// 仅 UI 的展开状态，见 [`FixtureModule::expanded`]。
        #[serde(default)]
        #[allow(dead_code)]
        expanded: Option<bool>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEndpoint {
        id: String,
        collection_id: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
        method: String,
        url: String,
        #[serde(default)]
        environment_id: Option<String>,
        #[serde(default)]
        base_url_definition_id: Option<String>,
        #[serde(default)]
        base_url_override: Option<String>,
        #[serde(default)]
        params: Vec<serde_json::Value>,
        #[serde(default)]
        headers: Vec<serde_json::Value>,
        body: serde_json::Value,
        auth: serde_json::Value,
        #[serde(default)]
        docs: Option<String>,
        #[serde(default)]
        notes: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
        sort_order: i64,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEnvironment {
        id: String,
        project_id: String,
        name: String,
        /// `def_id -> module_id -> url`，现役值层。下线的 `base_urls_json` 完全不播种：
        /// fixture 库是现役模型的库，留下有值的旧列会让每个播种测试启动都跑一遍数据迁移。
        #[serde(default)]
        module_base_urls: serde_json::Value,
        #[serde(default)]
        variables: serde_json::Value,
        is_active: bool,
    }

    /// 插入资源树的一行。fixture 的三个树段落都落到这一种形状，变的只有 `kind` 与
    /// `parent_id` 是否为空。收 [`ResourceNode`] 而非散列，是为了让翻译写成几个结构体字面量，
    /// 而不是十个位置参数的调用。
    fn insert_node(tx: &rusqlite::Transaction<'_>, n: &ResourceNode) -> Result<(), AppError> {
        tx.execute(
            "INSERT INTO nodes \
         (id, root_id, parent_id, type, name, description, sort_order, created_at, updated_at) \
         VALUES (?,?,?,?,?,?,?,?,?)",
            params![
                n.id,
                n.root_id,
                n.parent_id,
                n.kind.as_str(),
                n.name,
                n.description,
                n.sort_order,
                n.created_at,
                n.updated_at
            ],
        )?;
        Ok(())
    }

    /// 按依赖拓扑顺序插入 fixture 全部行：projects → base_url_defs + modules + envs →
    /// collections → endpoints。外键自然满足，无需 `PRAGMA defer_foreign_keys`。
    fn seed_fixtures(conn: &mut Connection, fx: &Fixtures) -> Result<(), AppError> {
        let tx = conn.transaction()?;

        for p in &fx.projects {
            tx.execute(
            "INSERT INTO projects \
             (id, name, description, default_base_url_definition_id, color, created_at, updated_at) \
             VALUES (?,?,?,?,?,?,?)",
            params![
                p.id,
                p.name,
                p.description,
                p.default_base_url_definition_id,
                p.color,
                p.created_at,
                p.updated_at,
            ],
        )?;
            // 每个 def 单独入表，endpoint 的外键链才成立。
            for (i, d) in p.base_url_definitions.iter().enumerate() {
                tx.execute(
                    "INSERT INTO base_url_definitions \
                 (id, project_id, name, description, sort_order) \
                 VALUES (?,?,?,?,?)",
                    params![d.id, p.id, d.name, d.description, i as i64],
                )?;
            }
            // `baseUrl` 是 Project 上的遗留字段，schema 无对应列，不落库。
            let _ = p.base_url.as_ref();
        }

        // fixture 保持三段式（modules / collections / endpoints），便于人读样例树；
        // 翻译在此发生：每项一行 `nodes`，endpoint 另加一行 `endpoint_details`。
        // `root_id` 是 fixture 不带的那个值——原本由所属模块隐含，此处推导并按 owner
        // 记忆化，使查表是一次 map 读，而非每个后代都重走一遍。
        let mut owner: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();

        for m in &fx.modules {
            insert_node(
                &tx,
                &ResourceNode {
                    id: m.id.clone(),
                    root_id: m.project_id.clone(),
                    // 模块的父是项目，不是另一个节点。
                    parent_id: None,
                    kind: ResourceNodeType::Module,
                    name: m.name.clone(),
                    description: m.description.clone(),
                    sort_order: m.sort_order,
                    created_at: fx.now,
                    updated_at: fx.now,
                },
            )?;
            owner.insert(&m.id, &m.project_id);
        }

        for c in &fx.collections {
            let root_id = owner.get(c.module_id.as_str()).copied().ok_or_else(|| {
                AppError::Validation(format!(
                    "fixture collection {} names unknown module {}",
                    c.id, c.module_id
                ))
            })?;
            // 嵌套集合的 `parent_id` 是集合而非模块——类型矩阵两者都允许，
            // fixture 用是否给 `parentCollectionId` 表示。
            let parent = c
                .parent_collection_id
                .as_deref()
                .unwrap_or(c.module_id.as_str());
            insert_node(
                &tx,
                &ResourceNode {
                    id: c.id.clone(),
                    root_id: root_id.to_string(),
                    parent_id: Some(parent.to_string()),
                    kind: ResourceNodeType::Collection,
                    name: c.name.clone(),
                    description: c.description.clone(),
                    sort_order: c.sort_order,
                    created_at: fx.now,
                    updated_at: fx.now,
                },
            )?;
            owner.insert(&c.id, root_id);
        }

        for e in &fx.endpoints {
            let root_id = owner
                .get(e.collection_id.as_str())
                .copied()
                .ok_or_else(|| {
                    AppError::Validation(format!(
                        "fixture endpoint {} names unknown collection {}",
                        e.id, e.collection_id
                    ))
                })?;
            insert_node(
                &tx,
                &ResourceNode {
                    id: e.id.clone(),
                    root_id: root_id.to_string(),
                    parent_id: Some(e.collection_id.clone()),
                    kind: ResourceNodeType::Endpoint,
                    name: e.name.clone(),
                    description: e.description.clone(),
                    sort_order: e.sort_order,
                    created_at: e.created_at,
                    updated_at: e.updated_at,
                },
            )?;
            // 节点与配置一起落：没有 detail 行的 endpoint 打不开。
            tx.execute(
                "INSERT INTO endpoint_details \
             (node_id, method, url, environment_id, base_url_definition_id, \
              base_url_override, params_json, headers_json, body_json, \
              auth_json, docs, notes, tags_json) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    e.id,
                    e.method,
                    e.url,
                    e.environment_id,
                    e.base_url_definition_id,
                    e.base_url_override,
                    serde_json::to_string(&e.params)?,
                    serde_json::to_string(&e.headers)?,
                    serde_json::to_string(&e.body)?,
                    serde_json::to_string(&e.auth)?,
                    e.docs,
                    e.notes,
                    serde_json::to_string(&e.tags)?,
                ],
            )?;
        }

        for env in &fx.environments {
            let module_base_urls_json = serde_json::to_string(&env.module_base_urls)?;
            let variables_json = serde_json::to_string(&env.variables)?;
            tx.execute(
                "INSERT INTO environments \
             (id, project_id, name, is_active, module_base_urls_json, variables_json) \
             VALUES (?,?,?,?,?,?)",
                params![
                    env.id,
                    env.project_id,
                    env.name,
                    env.is_active as i64,
                    module_base_urls_json,
                    variables_json,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

// CRUD 模块共用的辅助类型与函数。

/// `Arc<str>` 的重导出，供需要廉价克隆 id 句柄的调用方使用。
#[allow(dead_code)]
pub(crate) fn arc_str(s: impl Into<String>) -> Arc<str> {
    Arc::from(s.into())
}

/// JSON-in-TEXT 列解码失败时的错误类型。rusqlite 的 `FromSqlConversionFailure`
/// 要 `Box<dyn StdError + Send + Sync + 'static>`，故用这个字符串承载类型把消息装下。
#[derive(Debug)]
pub(crate) struct JsonColError(pub String);

impl std::fmt::Display for JsonColError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsonColError {}

/// 把 JSON 列的 serde 失败映射成 rusqlite 转换错误并带上列名——调用方能看出是哪一列坏了，
/// 而不是面对一个不透明的解码失败。所有把结构化数据放 TEXT 列的模块共用。
pub(crate) fn json_col_err(col: &str, e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(JsonColError(format!("{col}: {e}"))),
    )
}

// 顶层表面积（pool / init / migrate / ping / with_tx 重试）的测试。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

/// 每进程计数器，给每个测试一个唯一临时目录。独立成模块，免得测试文件在
/// `mod tests` 里争同一个可变全局。
#[doc(hidden)]
pub mod test_counter {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    pub fn next() -> u64 {
        N.fetch_add(1, Ordering::Relaxed)
    }
}
