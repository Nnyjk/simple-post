//! 连接池与顶层入口：`DbPool` / `init` / `with_conn` / `with_tx` / `ping`。

use std::path::Path;
use std::time::Duration;

use r2d2::ManageConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags};

use crate::error::AppError;

use super::schema::{ensure_current_schema, migrate};
use super::SCHEMA_VERSION;

/// 共享连接池。`DbPool` 只是 `Arc` 包裹的廉价克隆，可随意传递。
/// 池大小固定 4：SQLite 本就串行化写入，池更大只增加争用、不增吞吐。
pub type DbPool = r2d2::Pool<SqliteConnectionManager>;

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
