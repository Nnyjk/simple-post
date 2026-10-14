//! 统一的应用错误类型。
//!
//! 后端每个可能失败的操作都返回 `Result<T, AppError>`。下面的 serde 实现把 `AppError`
//! 塑造成稳定的 `{code, message}` 对象，好让 React 渲染层直接按 `error.code` 分派，
//! 无需解析字符串或另设错误类。

use serde::{Serialize, Serializer};

/// 所有能跨 Rust ↔ JS 边界的错误。
///
/// 每个变体在线上都有一个稳定的 `code` 字符串（即 snake_case 的变体名）加一条给人看的
/// `message`。新变体应保持 `code` 小写且跨版本稳定——前端要按它分派。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 资源查找没有结果（如对不存在的 id 调 `db::projects::get`）。渲染层映射为 404 提示。
    #[error("not found: {0}")]
    NotFound(String),

    /// 调用方给的输入没通过校验（空名字、坏 id 等）。以行内表单错误呈现给用户。
    #[error("validation: {0}")]
    Validation(String),

    /// agent / MCP 调用方无权执行该动作。
    #[error("permission denied: {0}")]
    PermissionDenied(String),

    /// SQLite / r2d2 / serde / io 失败。内层消息进 `tracing::error!`；线上形状保持通用，
    /// 以免把 SQL 片段泄进渲染层。
    #[error("db error: {0}")]
    Db(String),

    /// 响应层以下冒出来的网络 / 传输失败（DNS、连接、TLS、超时等）。
    #[error("http error: {0}")]
    Http(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("pool error: {0}")]
    Pool(#[from] r2d2::Error),
}

impl AppError {
    /// 渲染层用来切换错误类别的简短稳定 code。新增变体时**这里和下面的 match 都要加**，
    /// 否则线上形状会静默改变。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "not_found",
            AppError::Validation(_) => "validation",
            AppError::PermissionDenied(_) => "permission_denied",
            AppError::Db(_) => "db",
            AppError::Http(_) => "http",
            AppError::Io(_) => "io",
            AppError::Serde(_) => "serde",
            AppError::Sqlite(_) => "sqlite",
            AppError::Pool(_) => "pool",
        }
    }
}

/// 线上形状：`{ "code": "not_found", "message": "..." }`。两个键都是字面量，
/// 故渲染层不必知道 Rust 的命名习惯。
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = ser.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

// 测试——线上形状的稳定性。

#[cfg(test)]
#[path = "error_test.rs"]
mod tests;
