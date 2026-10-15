//! `oauth2_tokens` 缓存：配置哈希作键，落库前补齐缺省字段。

use sha2::{Digest, Sha256};

use crate::db::{self, DbPool};
use crate::domain::{now_ms, OAuth2Auth};
use crate::error::AppError;

use super::OAuth2Token;
use super::EXPIRY_SKEW_SECS;

/// 这份配置对应的缓存键：除 `auth_code` 外全部字段的 sha256 十六进制。
///
/// 排除 `auth_code` 是必须的：它是**一次性**输入，用户贴一次新码就换一个值，若进了
/// 哈希，缓存行会在每次换码后立刻变成孤儿——而配置语义上根本没变。
///
/// 哈希而不是原始 JSON：主键里带着 `clientSecret` 的明文会把一份凭据复制到第二张表，
/// 与本模块「凭据只存一份」的承诺冲突。
pub fn auth_key(cfg: &OAuth2Auth) -> String {
    let mut canonical = cfg.clone();
    canonical.auth_code = String::new();
    // serde_json 对结构体按声明顺序输出，故同一份配置恒得同一段文本。
    let raw = serde_json::to_string(&canonical).unwrap_or_default();
    let digest = Sha256::digest(raw.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub(super) fn store(
    pool: &DbPool,
    key: &str,
    mut token: OAuth2Token,
    keep_refresh: Option<&str>,
) -> Result<OAuth2Token, AppError> {
    // 刷新响应可能不带 refresh_token（RFC 6749 §6）；沿用旧值，否则这份配置下次的
    // 刷新机会就此消失。
    if token.refresh_token.is_none() {
        token.refresh_token = keep_refresh.map(str::to_string);
    }
    if token.token_type.trim().is_empty() {
        token.token_type = "Bearer".into();
    }
    db::with_tx(pool, |c| db::oauth2_tokens::upsert(c, key, &token))?;
    // 只记 key 前缀：完整 key 是配置哈希，记它没有信息量；而任何一行日志都可能被贴进
    // issue 或工单。
    tracing::debug!(
        "oauth2: stored token for key {}… (expires_at {:?})",
        &key[..8.min(key.len())],
        token.expires_at
    );
    Ok(token)
}

/// 缓存 token 是否还能直接用。`expires_at` 为 `None` = 不知道过期时间，于是用着——
/// IdP 没给 `expires_in` 时这是唯一可行的判断。
pub(super) fn is_expired(token: &OAuth2Token) -> bool {
    match token.expires_at {
        Some(at) => unix_secs() + EXPIRY_SKEW_SECS >= at,
        None => false,
    }
}

/// 「这次是不是刚取来的」——给 [`super::resolve`] 的 `refreshed` 标记用：命中缓存时它该是
/// `false`，否则 UI 会把一个两小时前的 token 说成刚刷新。
pub(super) fn is_fresh(token: &OAuth2Token) -> bool {
    unix_secs().saturating_sub(token.obtained_at) < 5
}

pub(crate) fn unix_secs() -> i64 {
    now_ms() / 1000
}
