//! 两条传输层的共同入口：解析入参 → 拿到可注入的 token → 组线格式。

use serde::{Deserialize, Serialize};

use crate::db::{self, DbPool};
use crate::domain::{AuthConfig, AuthType, OAuth2Auth, OAuth2GrantType};
use crate::error::AppError;

use super::store::{is_expired, is_fresh, store, unix_secs};
use super::token_endpoint::{fetch_via_http, refresh_via_http};
use super::validate::{implicit_needs_manual, validate};
use super::OAuth2Token;
use super::{auth_key, parse_pasted};

/// 两个传输层共用的调用入参（`oauth2_token` 命令 / `POST /api/oauth2/token`）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2TokenRequest {
    pub auth: AuthConfig,
    /// `implicit` 流程没有 token 端点，token 由浏览器 fragment 直接带回：调用方把用户
    /// 粘贴的原文（整条回调 URL 或裸 token）放这里，由 [`parse_pasted`] 解析。
    #[serde(default)]
    pub manual: Option<OAuth2ManualToken>,
    /// 忽略缓存强制重取。UI 的「重新获取」按钮用它；正常发请求一律走
    /// [`ensure_token`]，不传。
    #[serde(default)]
    pub force: bool,
}

/// 用户粘贴的 token 原文。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2ManualToken {
    /// 整条回调 URL（`https://app/cb#access_token=…` 或带 `?code=` 的那种），或裸
    /// token 串。解析在 Rust 侧做，故 MCP 与桌面端对同一段文本得同一份结果。
    pub pasted: String,
}

/// 回给渲染层的形状。
///
/// **刻意不含 `refresh_token`**：它在 `oauth2_tokens` 表里续期，web 形态下没必要、
/// 也不应该让浏览器持有。`accessToken` 必须给——前端要自己把它拼进 `Authorization`，
/// 与它拿到别的请求头没有区别。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuth2TokenDto {
    pub access_token: String,
    pub token_type: String,
    /// Unix 秒。`None` = 不知道何时过期。
    pub expires_at: Option<i64>,
    pub scope: Option<String>,
    /// 本次是否真的发了网络请求。`false` = 命中缓存，UI 据此显示「还有多久过期」
    /// 而不是假装刚取过。
    pub refreshed: bool,
}

/// 两条传输层的共同入口：解析入参 → 拿到可注入的 token → 组线格式。
pub async fn resolve(pool: &DbPool, req: &OAuth2TokenRequest) -> Result<OAuth2TokenDto, AppError> {
    let cfg = match (&req.auth.kind, req.auth.oauth2.as_ref()) {
        (AuthType::Oauth2, Some(cfg)) => cfg,
        // 载荷与 `kind` 不一致时按「没配 oauth2」报，而不是替它猜一个——MCP 侧
        // `{"kind": "oauth2"}`（无载荷）曾经是「被拒绝」的形状，现在它是「配到一半」，
        // 错误信息要能指出缺的是载荷。
        (AuthType::Oauth2, None) => {
            return Err(AppError::Validation(
                "auth.kind 是 oauth2，但缺少 oauth2 载荷".into(),
            ))
        }
        _ => {
            return Err(AppError::Validation(format!(
                "auth.kind 必须是 oauth2，收到 `{}`",
                req.auth.kind.as_str()
            )))
        }
    };

    let (token, refreshed) = match &req.manual {
        // 手动收下的一定是新的：它刚从用户手里过来。
        Some(manual) => (save_manual(pool, cfg, manual)?, true),
        None if req.force => (fetch_and_store(pool, cfg).await?, true),
        None => {
            let t = ensure_token(pool, cfg).await?;
            // 缓存命中不是刷新。报成 true 就是一句谎话：UI 会显示「刚获取」，
            // 而用户看到的其实是一个两小时前就签发的 token。
            let fresh = is_fresh(&t);
            (t, !fresh)
        }
    };
    Ok(OAuth2TokenDto {
        access_token: token.access_token,
        token_type: token.token_type,
        expires_at: token.expires_at,
        scope: token.scope,
        refreshed,
    })
}

/// 取 token 的单飞闸。
///
/// 全局一把锁而不是按 `auth_key` 分桶：取 token 是秒级低频操作，串行化不产生可感知的
/// 竞争，而分桶要维护一张只增不减的表。
///
/// 它挡的是一个具体的失败，而不是泛泛的「线程安全」：同一份配置的两次并发发送都发现
/// 缓存过期，于是都拿同一个**一次性** `refresh_token` 去换。轮换 refresh token 的
/// IdP 会让第二次失败，而我们「刷新失败就整取」的兜底会拿**已消费**的授权码再换一次，
/// 于是用户看到一句完全无从排查的失败。闸内重读缓存让第二次直接复用第一次的结果。
static REFRESH_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 缓存里有一份且还没过期就直接用；过期则先试刷新，刷新不成再整取。发送路径的唯一入口。
///
/// 整段「查缓存 → 刷新 → 落库」在 [`REFRESH_GATE`] 里，并在拿到闸之后**重读**缓存：
/// 并发调用者在这里看到的是前一个刚写下的 token，而不是又去刷新一遍。
///
/// UI 的 `force` 路径（[`fetch_and_store`]）不经过这把闸——它要的就是忽略缓存重取，
/// 与并发重复发请求是两种不同的意图，串行化它只会让「重新获取」按下去没反应。
pub async fn ensure_token(pool: &DbPool, cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    validate(cfg)?;
    let _gate = REFRESH_GATE.lock().await;
    let key = auth_key(cfg);
    let cached = db::with_conn(pool, |c| db::oauth2_tokens::get(c, &key))?;

    let Some(cached) = cached else {
        if !cfg.can_refresh() {
            return Err(implicit_needs_manual());
        }
        return fetch_and_store(pool, cfg).await;
    };

    if !is_expired(&cached) {
        return Ok(cached);
    }
    // 刷新失败不致命：refresh_token 本身可能早被 IdP 作废，退回整取才是对的行为。
    // 两种流程都换不了时才把原始错误交出去。
    if cfg.can_refresh() {
        if let Some(rt) = cached.refresh_token.as_deref() {
            match refresh_via_http(cfg, rt).await {
                Ok(fresh) => return store(pool, &key, fresh, Some(rt)),
                Err(e) => tracing::debug!("oauth2: refresh_token grant failed, refetching: {e}"),
            }
        }
    }
    if !cfg.can_refresh() {
        return Err(implicit_needs_manual());
    }
    fetch_and_store(pool, cfg).await
}

/// 忽略缓存，重新走一遍完整取 token 流程（UI 的「重新获取」）。
pub async fn fetch_and_store(pool: &DbPool, cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    validate(cfg)?;
    let key = auth_key(cfg);
    let fresh = fetch_via_http(cfg).await?;
    store(pool, &key, fresh, None)
}

/// 收下用户粘贴的 `implicit` token 并写进缓存。
///
/// 只对 `implicit` 开放：别的流程都能自己换到手，为它们开这个口子只会让「为什么这次
/// 没过 IdP」变得难查。
pub fn save_manual(
    pool: &DbPool,
    cfg: &OAuth2Auth,
    manual: &OAuth2ManualToken,
) -> Result<OAuth2Token, AppError> {
    if cfg.grant_type != OAuth2GrantType::Implicit {
        return Err(AppError::Validation(
            "手动 token 只用于 implicit 流程；其余流程请检查 token 端点配置".into(),
        ));
    }
    let parsed = parse_pasted(&manual.pasted)?;
    let key = auth_key(cfg);
    let token = OAuth2Token {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        token_type: parsed.token_type,
        scope: None,
        expires_at: parsed.expires_in.map(|secs| unix_secs() + secs),
        obtained_at: unix_secs(),
    };
    store(pool, &key, token, None)
}
