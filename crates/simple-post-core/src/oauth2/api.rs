//! 两条传输层的共同入口：解析入参 → 拿到可注入的 token → 组线格式。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::db::{self, DbPool};
use crate::domain::{AuthConfig, AuthType, OAuth2Auth, OAuth2GrantType};
use crate::error::AppError;

use super::config::{substitute_config, SubstitutedConfig};
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
    /// 变量池：渲染进程侧算好的那一份（`buildPool(env, dynamicVars)`）。
    ///
    /// OAuth2 配置里的 `{{var}}` 由 core 在取 token 之前替换，替换规则住在
    /// [`super::config`]。渲染进程侧不自己替换，故它必须把变量池送过来 ——
    /// 送的是**同一份**池（组装请求时用的那份），否则同一个 `{{var}}` 在 URL 里能
    /// 解析、在 `tokenUrl` 里不能解析。
    ///
    /// 缺席 = 没有变量可替换，配置里的 `{{var}}` 原样留着。
    #[serde(default)]
    pub vars: HashMap<String, String>,
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
        // 载荷与 `kind` 不一致时按「配到一半」报，而不是替它猜一个——MCP 侧
        // `{"kind": "oauth2"}`（无载荷）就是这个形状，错误信息要能指出缺的是载荷。
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
        Some(manual) => (save_manual(pool, cfg, manual, &req.vars)?, true),
        None if req.force => (fetch_and_store(pool, cfg, &req.vars).await?, true),
        None => {
            let t = ensure_token(pool, cfg, &req.vars).await?;
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
/// IdP 会让第二次失败，而我们「刷新失败就整取」的回退路径会拿**已消费**的授权码再换一次，
/// 于是用户看到一句完全无从排查的失败。闸内重读缓存让第二次直接复用第一次的结果。
static REFRESH_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 缓存里有一份且还没过期就直接用；过期则先试刷新，刷新不成再整取。发送路径的唯一入口。
///
/// 整段「查缓存 → 刷新 → 落库」在 [`REFRESH_GATE`] 里，并在拿到闸之后**重读**缓存：
/// 并发调用者在这里看到的是前一个刚写下的 token，而不是又去刷新一遍。
///
/// UI 的 `force` 路径（[`fetch_and_store`]）不经过这把闸——它要的就是忽略缓存重取，
/// 与并发重复发请求是两种不同的意图，串行化它只会让「重新获取」按下去没反应。
pub async fn ensure_token(
    pool: &DbPool,
    cfg: &OAuth2Auth,
    vars: &HashMap<String, String>,
) -> Result<OAuth2Token, AppError> {
    // 替换在这里发生：`vars` 是调用方算好的变量池，OAuth2 配置里那四个字段的
    // `{{var}}` 在取 token 之前替换（规则见 [`super::config`]）。密钥那三个字段
    // 不替换。
    let sub = substitute_config(cfg, vars);
    report_unresolved(&sub)?;
    let cfg = &sub.config;
    validate(cfg)?;
    let _gate = REFRESH_GATE.lock().await;
    let key = auth_key(cfg);
    let cached = db::with_conn(pool, |c| db::oauth2_tokens::get(c, &key))?;

    let Some(cached) = cached else {
        if !cfg.can_refresh() {
            return Err(implicit_needs_manual());
        }
        return fetch_and_store_resolved(pool, cfg).await;
    };

    if !is_expired(&cached) {
        return Ok(cached);
    }
    // 刷新失败不致命：refresh_token 本身可能早被 IdP 作废，退回整取才是对的行为。
    // 两种流程都换不了时才把原始错误交出去。
    if cfg.can_refresh() {
        if let Some(rt) = cached.refresh_token.as_deref() {
            // token 端点也是一次 HTTPS 出站，故它用**全局默认**的设置：token 端点不属于
            // 某个端点，端点级的覆盖对它没有意义。
            let tls = crate::tls::global_default(pool);
            match refresh_via_http(cfg, rt, &tls).await {
                Ok(fresh) => return store(pool, &key, fresh, Some(rt)),
                Err(e) => tracing::debug!("oauth2: refresh_token grant failed, refetching: {e}"),
            }
        }
    }
    if !cfg.can_refresh() {
        return Err(implicit_needs_manual());
    }
    fetch_and_store_resolved(pool, cfg).await
}

/// 忽略缓存，重新走一遍完整取 token 流程（UI 的「重新获取」）。
pub async fn fetch_and_store(
    pool: &DbPool,
    cfg: &OAuth2Auth,
    vars: &HashMap<String, String>,
) -> Result<OAuth2Token, AppError> {
    let sub = substitute_config(cfg, vars);
    report_unresolved(&sub)?;
    fetch_and_store_resolved(pool, &sub.config).await
}

/// 已经替换好的配置 → 落库。
///
/// [`ensure_token`] 与 [`fetch_and_store`] 共用它，故替换只发生一次：配置里某个变量
/// 的取值本身写着 `{{x}}` 时，替换两遍会把它展开第二遍。
async fn fetch_and_store_resolved(
    pool: &DbPool,
    cfg: &OAuth2Auth,
) -> Result<OAuth2Token, AppError> {
    validate(cfg)?;
    let key = auth_key(cfg);
    // token 端点用全局默认的 HTTPS 设置，理由同 [`ensure_token`]。
    let tls = crate::tls::global_default(pool);
    let fresh = fetch_via_http(cfg, &tls).await?;
    store(pool, &key, fresh, None)
}

/// 报出这条流程用到的字段里没解析出来的变量。
///
/// 只报**这条流程真的会发出去**的字段：`tokenUrl` / `clientId` / `scope` 进了 token
/// 请求，字段里留着 `{{name}}` 就等于把字面量发给 IdP，而用户看到的只是一句
/// 「token 获取失败」。`authorizationUrl` 不进 token 请求（它只是给用户点开的登录地址，
/// 而本应用不开浏览器），`implicit` 流程压根没有 token 端点（token 由用户粘贴），
/// 故两者都不报。
fn report_unresolved(sub: &SubstitutedConfig) -> Result<(), AppError> {
    if sub.config.grant_type == OAuth2GrantType::Implicit {
        return Ok(());
    }
    let in_use = ["tokenUrl", "clientId", "scope"];
    let mut problems: Vec<String> = Vec::new();
    for (field, missing) in &sub.missing {
        if !in_use.contains(field) {
            continue;
        }
        problems.push(format!("{field} 里的 {}", missing.join("、")));
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "OAuth2 配置里的变量没有解析出来：{}（变量池里没有这些变量）",
        problems.join("；")
    )))
}

/// 收下用户粘贴的 `implicit` token 并写进缓存。
///
/// 只对 `implicit` 开放：别的流程都能自己换到手，为它们开这个口子只会让「为什么这次
/// 没过 IdP」变得难查。
pub fn save_manual(
    pool: &DbPool,
    cfg: &OAuth2Auth,
    manual: &OAuth2ManualToken,
    vars: &HashMap<String, String>,
) -> Result<OAuth2Token, AppError> {
    if cfg.grant_type != OAuth2GrantType::Implicit {
        return Err(AppError::Validation(
            "手动 token 只用于 implicit 流程；其余流程请检查 token 端点配置".into(),
        ));
    }
    // 替换后再算缓存键：键是配置的哈希，故同一份配置（无论它写着 `{{var}}` 还是展开
    // 后的文本）必须落到同一行上。
    let cfg = substitute_config(cfg, vars).config;
    let parsed = parse_pasted(&manual.pasted)?;
    let key = auth_key(&cfg);
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
