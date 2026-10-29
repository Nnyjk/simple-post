//! 磁盘布局：`SCHEMA_SQL` 的执行、增量补列、旧库替换判定。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};

use crate::error::AppError;

use super::SCHEMA_VERSION;

/// 执行内嵌 `schema.sql`，把既有库补到当前列集，再修复旧布局写的行。幂等，每次启动可调。
///
/// 所有 `CREATE` 都是 `IF NOT EXISTS`，对已迁移的库是空操作。父子矩阵触发器是唯一例外：
/// 先 DROP 再建——触发器体**就是**契约，`IF NOT EXISTS` 会让老库继续执行应用层已放弃的矩阵。
/// **不插入任何行**：全新安装是空库，由用户或 MCP 客户端填充；测试种子在 `test_support`。
pub fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    // 必须在 `SCHEMA_SQL` 之前：旧预览形状的动态变量表带着 `project_id` 列，且缺列，
    // `CREATE TABLE IF NOT EXISTS` 对它是空操作，旧行会留在一张形状已经不同的表里。
    drop_obsolete_dynamic_variable_tables(conn)?;
    // 必须在 `SCHEMA_SQL` 之前：既有库要先把新增的列补上，`SCHEMA_SQL` 里那条按新列
    // 建的唯一索引才建得起来（列还不存在时那句会直接失败，整个 `migrate` 随之失败）。
    add_missing_columns(conn)?;
    conn.execute_batch(SCHEMA_SQL)?;
    // 必须在 `SCHEMA_SQL` 之后：那张表不该存在，而 `schema.sql` 里已经没有它的 `CREATE`。
    // 放在之后，于是有人把 `CREATE` 抄回 `schema.sql` 时，那张表会被建出来又被紧接着
    // 删掉——顺序的正确用法是「不要抄」，不是「抄了也安全」。
    drop_obsolete_request_settings_table(conn)?;
    // 必须在补列之后：那条索引按新列建，且要丢掉旧形状的「名字全局唯一」那条。
    drop_obsolete_dynamic_variable_name_index(conn)?;
    // 必须在索引换过之后：归位时新索引已经在管着「项目内不重名」这条规矩。
    backfill_dynamic_variable_projects(conn)
}

/// 丢掉那张建了却没人用过的 `request_settings` 表。
///
/// 它是单例行（`CHECK id=1`），三列是超时 / 跟随重定向 / 响应体积上限，但从建立起就没有
/// 任何一行被写进去、也没有任何读者。请求旋钮的现役容器是 `preferences` 表里的
/// `request_settings` **行**（[`crate::db::preferences::keys::REQUEST_SETTINGS`]）。
///
/// **刻意不 bump `SCHEMA_VERSION`**：删掉一张没人读的表不改变任何一行的可读性，而 bump
/// 版本会让 [`ensure_current_schema`] 删掉每个用户的库——为清一张空表赔上全部项目、
/// 环境与资源树。做法与 [`drop_obsolete_dynamic_variable_tables`] 同源：打开时丢掉。
///
/// **只丢这一张表**：`preferences` 表本身不能碰，请求旋钮就住在它里面。
fn drop_obsolete_request_settings_table(conn: &mut Connection) -> Result<(), AppError> {
    // 一条语句，不先查再删：GUI、守护进程与 `serve-web` 各自打开同一个 `store.db`，两个
    // 进程同时走到这里时，先查后删的写法会让其中一个撞上「no such table」——那会让
    // `migrate` 失败、`db::init` 失败，于是 GUI 与守护进程直接退出。`IF EXISTS` 让这句
    // 在表已经被别人删掉时是空操作。
    conn.execute_batch("DROP TABLE IF EXISTS request_settings;")?;
    tracing::debug!("schema: the obsolete request_settings table is absent");
    Ok(())
}

/// 旧预览形状的动态变量表直接丢掉。
///
/// 旧预览形状是「定义属于项目」：表里有 `project_id` 列，且**没有**
/// `source_environment_id`（取数环境是后来才加的）。当前形状同样带 `project_id`，
/// 故不能只看那一列——得看它有没有后加的那几列。
///
/// 丢掉两张表即可，下一次 `SCHEMA_SQL` 会把它们按当前形状建回来。
///
/// 只丢这两张表：项目、接口、环境等其余数据不动。
fn drop_obsolete_dynamic_variable_tables(conn: &mut Connection) -> Result<(), AppError> {
    if !table_has_column(conn, "dynamic_variables", "project_id")? {
        return Ok(());
    }
    if table_has_column(conn, "dynamic_variables", "source_environment_id")? {
        // dynamic_variables 表已是当前形状（带后加的列），故保留。
        return Ok(());
    }
    // 缓存表的外键指向定义表，故先丢缓存表。
    conn.execute_batch(
        "DROP TABLE IF EXISTS dynamic_variable_cache;\
         DROP TABLE IF EXISTS dynamic_variables;",
    )?;
    tracing::info!("schema: dropped the obsolete project-scoped dynamic variable tables");
    Ok(())
}

/// 丢掉旧形状的「变量名全局唯一」那条索引。
///
/// 旧形状的名字在整个工作区里唯一，新形状是**项目内**唯一：不同项目可以有同名变量。
/// 留着旧索引，新项目里的同名变量根本存不进去——那正是这次要改掉的行为。
///
/// 新形状的两条索引由 `SCHEMA_SQL` 建（它们按 `project_id` 建，故要求那一列先补上）。
fn drop_obsolete_dynamic_variable_name_index(conn: &mut Connection) -> Result<(), AppError> {
    conn.execute_batch("DROP INDEX IF EXISTS idx_dynamic_vars_name;")?;
    Ok(())
}

/// 给迁移遗留的动态变量定下归属项目。
///
/// 旧形状的动态变量是全局的：一条定义不属于任何项目。新形状要求每条定义挂在一个项目上，
/// 故这里给既有行补一个 `project_id`：
///
/// - 数据源端点还在 → 端点所属的项目（那正是这条定义取数时用的项目）；
/// - 数据源是内联请求 → 工作区里的第一个项目（按创建时间，再按 id）；
/// - 工作区里一个项目都没有 → 留空（未归属），等用户认领。
///
/// 旧形状里名字全局唯一，故归位之后同一项目里也不会撞名。
fn backfill_dynamic_variable_projects(conn: &mut Connection) -> Result<(), AppError> {
    let rows: Vec<(String, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, source_endpoint_id FROM dynamic_variables WHERE project_id IS NULL",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for r in mapped {
            out.push(r.map_err(AppError::Sqlite)?);
        }
        out
    };
    if rows.is_empty() {
        return Ok(());
    }
    // 第一个项目：内联数据源的定义归到它名下。
    let fallback_project: Option<String> = conn
        .query_row(
            "SELECT id FROM projects ORDER BY created_at ASC, id ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    for (id, source_endpoint_id) in rows {
        let project_id = match source_endpoint_id.as_deref() {
            Some(endpoint_id) => crate::db::nodes::root_id_of(conn, endpoint_id).ok(),
            None => None,
        };
        let project_id = project_id.or_else(|| fallback_project.clone());
        let Some(project_id) = project_id else {
            // 工作区里一个项目都没有：这条定义留作未归属，等用户认领。
            continue;
        };
        conn.execute(
            "UPDATE dynamic_variables SET project_id = ?1 WHERE id = ?2",
            params![project_id, id],
        )?;
    }
    tracing::info!("schema: assigned a project to the existing dynamic variables");
    Ok(())
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
/// 末尾四项是动态变量的归属项目与处理脚本，以及响应历史的收藏标记与请求快照。
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    (
        "environments",
        "module_base_urls_json",
        "TEXT NOT NULL DEFAULT '{}'",
    ),
    (
        // 停用的变量名。纯增量：旧行读回来是空集合，语义等价于「全部启用」。
        "environments",
        "disabled_variables_json",
        "TEXT NOT NULL DEFAULT '[]'",
    ),
    (
        // 端点的两段请求脚本。纯增量：旧行读回来是 NULL（= 没有脚本）。
        "endpoint_details",
        "pre_request_script",
        "TEXT",
    ),
    ("endpoint_details", "test_script", "TEXT"),
    (
        // URL 路径里的占位符与它们各自的值。纯增量：旧行读回来是空列表，
        // 语义等价于「这个接口没有路径参数」。
        "endpoint_details",
        "path_params_json",
        "TEXT NOT NULL DEFAULT '[]'",
    ),
    (
        // 端点自己的 HTTPS 出站设置。纯增量：旧行读回来是 NULL，
        // 语义等价于「继承全局默认」。
        "endpoint_details",
        "tls_json",
        "TEXT",
    ),
    (
        // 动态变量的归属项目。纯增量：旧行读回来是 NULL（= 未归属），
        // `backfill_dynamic_variable_projects` 随后给它们补上。
        //
        // 可空：`ALTER TABLE ... ADD COLUMN` 只能追加带默认值的列，而外键需要默认值；
        // 该形状下每行的这一列本来就可能是 NULL。
        "dynamic_variables",
        "project_id",
        "TEXT REFERENCES projects(id) ON DELETE CASCADE",
    ),
    (
        // 动态变量的处理脚本。纯增量：旧行读回来是 NULL（= 不跑脚本）。
        "dynamic_variables",
        "transform_script",
        "TEXT",
    ),
    (
        // 用户收藏了这条历史。纯增量：旧行读回来是「未收藏」，语义等价于裁剪规则
        // 只动未收藏的行。
        "response_history",
        "is_favorite",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    (
        // 那次发送出去的请求。纯增量：旧行读回来没有这一份，界面据此不画请求侧栏。
        "response_history",
        "request_json",
        "TEXT",
    ),
    (
        // 响应体的二进制形态（base64）。纯增量：旧行读回来没有这一份，
        // 语义等价于「那次响应是文本的」。
        "response_history",
        "body_base64",
        "TEXT",
    ),
];

/// 为 [`ADDED_COLUMNS`] 中缺列的表补上该列。
///
/// `PRAGMA table_info` 扫描刻意直白：`schema.sql` 尚未创建的表不返回行，于是被跳过，
/// 而不是让整个启动失败。
fn add_missing_columns(conn: &mut Connection) -> Result<(), AppError> {
    for (table, column, decl) in ADDED_COLUMNS {
        // 表还不存在 → 跳过：`SCHEMA_SQL` 随后会按当前形状把它建出来，那份形状里
        // 本来就带着这一列。少了这一步判断，ALTER 会对着不存在的表报错，整个
        // `migrate` 随之失败——而这正是全新安装的路径。
        if !table_exists(conn, table)? {
            continue;
        }
        if table_has_column(conn, table, column)? {
            continue;
        }
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
        tracing::info!("schema: added {table}.{column} to an existing store");
    }
    Ok(())
}

/// `table` 这张表存在吗？
///
/// 与 [`table_has_column`] 分开：`PRAGMA table_info` 对一张不存在的表返回**空行集**，
/// 于是「表不存在」与「表在但没这一列」在那一问里长得一样。故这里查 `sqlite_master`。
fn table_exists(conn: &Connection, table: &str) -> Result<bool, AppError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
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

/// 把磁盘上的库升到 [`SCHEMA_VERSION`]，版本不符则替换。
/// 返回 `true` 表示文件已被删除、将重建，由调用方告知数据丢失。
///
/// **没有迁移路径。** 本项目还没有发布给真实用户，数据迁移框架是守护一个人群的投机重量；
/// 版本号之前的三表布局（`modules` / `collections` / `endpoints`）文件直接替换而非升级。
/// 这个判断一旦改变，只有本函数需要动。
pub(super) fn ensure_current_schema(db_path: &Path) -> Result<bool, AppError> {
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
pub(super) fn remove_db_files(db_path: &Path) -> Result<(), AppError> {
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

// 内嵌静态资源：`include_str!` 使 schema 变更参与 `cargo build` 的重编判定。

const SCHEMA_SQL: &str = include_str!("schema.sql");
