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

use std::sync::Arc;

// 子模块：对外公开，让编排层 / Tauri commands 可直接调用单个 CRUD 函数。
pub mod base_url_defs;
pub mod endpoint_details;
pub mod environments;
pub mod nodes;
pub mod oauth2_tokens;
pub mod permissions;
pub mod preferences;
pub mod projects;
pub mod response_history;

// 内部子模块：定义在这些文件里，面板面（`db::init` / `db::migrate` / …）由本文件重导出。
mod base_url_migration;
mod pool;
mod schema;

pub use pool::{init, ping, with_conn, with_tx, DbPool};
pub use schema::migrate;

// 仅测试。整个模块是 `#[cfg(test)]`，只编进 `cargo test`，别处不存在。
// `db::init` 没有进这里的路径，故生产构建永远插不进种子行——全新安装是空库。
// `fixtures.json` 留在仓库里纯粹作为测试数据。
#[cfg(test)]
pub(crate) mod test_support;

// schema 版本。

/// 本构建读写的布局，由 `schema.sql` 末尾写入 `PRAGMA user_version`。
///
/// **仅当旧版本文件无法满足新形状时才可 bump**（删列/改名、一表拆多表）。纯增量的
/// 改动（新表、可空新列）不需要：`CREATE TABLE IF NOT EXISTS` 已覆盖，缺列靠
/// `db::schema` 的 `ADDED_COLUMNS` 补。
///
/// 改 `domain.rs` 里 auth 的字段名同样要 bump：`auth_json` 存的是 serde 原样输出，
/// 改名后旧行反序列化不出来，文件即不可读。
///
/// 版本号历史写在 `schema.sql` 末尾，改动请同步。
pub const SCHEMA_VERSION: i64 = 3;

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
mod migration_fixtures;
#[cfg(test)]
mod test_helpers;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_tests;

#[cfg(test)]
#[path = "base_url_migration_test.rs"]
mod base_url_migration_tests;

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
