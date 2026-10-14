//! OAuth2 换来的 token 缓存。
//!
//! 一行 = 一份 OAuth2 配置（由 [`crate::oauth2::auth_key`] 哈希而来）当前持有的
//! token。只存 token，取它要的凭据仍在 `endpoint_details.auth_json` 里，故不存在
//! 两处凭据漂移的可能。
//!
//! 读用 `db::with_conn`、写用 `db::with_tx`（由调用方决定）。这里不做过期判断：
//! 「还能用吗、要不要刷新」是 [`crate::oauth2`] 的事，它才知道该刷新还是该重取。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppError;

/// 缓存里的一行 token。字段与 `oauth2_tokens` 表一一对应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuth2Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// RFC 6749 §7.1 的 `token_type`，多数 IdP 回 `Bearer`。
    pub token_type: String,
    pub scope: Option<String>,
    /// Unix 秒。`None` = IdP 没给 `expires_in`，故无从判断过期。
    pub expires_at: Option<i64>,
    pub obtained_at: i64,
}

/// 读某份配置缓存的 token。没有则 `Ok(None)`。
pub fn get(conn: &mut Connection, auth_key: &str) -> Result<Option<OAuth2Token>, AppError> {
    conn.query_row(
        "SELECT access_token, refresh_token, token_type, scope, expires_at, obtained_at \
         FROM oauth2_tokens WHERE auth_key = ?1",
        params![auth_key],
        |r| {
            Ok(OAuth2Token {
                access_token: r.get(0)?,
                refresh_token: r.get(1)?,
                token_type: r.get(2)?,
                scope: r.get(3)?,
                expires_at: r.get(4)?,
                obtained_at: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

/// 写入（或覆盖）某份配置的 token。
///
/// 刷新响应按 RFC 6749 §6 **可能不带** `refresh_token`，此时必须沿用旧值而不是写成
/// NULL——丢掉它等于让这份配置下次的刷新机会永久消失。`refresh_token` 为 `None`
/// 才表示「确实没有 / 该流程不签发」，此时清空旧值是对的。
pub fn upsert(conn: &mut Connection, auth_key: &str, token: &OAuth2Token) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO oauth2_tokens \
           (auth_key, access_token, refresh_token, token_type, scope, expires_at, obtained_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(auth_key) DO UPDATE SET \
           access_token  = excluded.access_token, \
           refresh_token = excluded.refresh_token, \
           token_type    = excluded.token_type, \
           scope         = excluded.scope, \
           expires_at    = excluded.expires_at, \
           obtained_at   = excluded.obtained_at",
        params![
            auth_key,
            token.access_token,
            token.refresh_token,
            token.token_type,
            token.scope,
            token.expires_at,
            token.obtained_at,
        ],
    )?;
    Ok(())
}

/// 丢弃某份配置的 token。凭据被改（换了 client secret / token URL）后调用，让旧
/// token 不再被任何路径取到。
pub fn delete(conn: &mut Connection, auth_key: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM oauth2_tokens WHERE auth_key = ?1",
        params![auth_key],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "oauth2_tokens_test.rs"]
mod tests;