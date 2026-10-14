//! OAuth2：按需取 token、必要时刷新，然后把 token 交给调用方注入请求。
//!
//! 这一层**只管 token 的生命周期**，不碰 URL、header 排序或 `{{var}}` 替换——那些是
//! `variable_fill` 与 `src/lib/http-build.ts` 的活。两边各自调 [`ensure_token`] 拿到
//! [`OAuth2Token`]，再用 [`auth_header_value`] / 各自的 query 追加把 token 挂到请求上。
//!
//! 三条不变量：
//!
//! - **token 不进日志。** 本模块与 [`crate::http_executor`] 都不打 body；[`store`] 的
//!   `tracing` 只记 key 前缀与到期时刻。`refresh_token` 永不离开 Rust 进程（它不进
//!   [`OAuth2TokenDto`]），浏览器的 web 形态拿不到它。
//! - **复用 [`crate::http_executor`]。** token 端点因此和普通请求走同一道
//!   [`crate::http_executor::OutboundPolicy`]（`SIMPLE_POST_BLOCK_PRIVATE_NET`）与同一套
//!   超时 / 重定向 / 体积裁剪，web + Docker 形态下它才不会成为绕过出站策略的第二条通道。
//! - **凭据只存一份。** 表 `oauth2_tokens` 只有 token；client secret 等仍只在
//!   `endpoint_details.auth_json` 里，故没有两处漂移的可能。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{self, DbPool};
use crate::domain::{
    now_ms, AuthConfig, AuthType, OAuth2Auth, OAuth2ClientAuth, OAuth2GrantType,
    OAuth2TokenLocation,
};
use crate::error::AppError;
use crate::http_executor::{self, HttpRequestInput};

pub use crate::db::oauth2_tokens::OAuth2Token;

/// 提前多久把 token 当成已过期。
///
/// 宁可早刷新也不晚：晚意味着拿一个刚失效的 token 发出请求，服务端回 401，用户看到
/// 一次莫名失败。60 秒够盖住「取回来 → 组装 → 发出」这段，也够盖住双方时钟的一点偏差。
const EXPIRY_SKEW_SECS: i64 = 60;

/// token 端点的超时。它不是用户的业务请求，不该继承 `settings.timeoutMs`。
const TOKEN_TIMEOUT_MS: u64 = 30_000;

/// token 响应体积上限（KB）。token 响应只有几百字节；1 MB 的异常响应（IdP 的错误页）
/// 只配拿到一句可读的第一行，不必整页塞进内存。
const TOKEN_MAX_RESPONSE_KB: u64 = 64;

/// IdP 的 `error_description` 最多摘多少字符进错误信息。错误页经常是一整张 HTML，
/// 全量拼进 `AppError` 会顶掉真正有用的那句。
const MAX_ERROR_DETAIL: usize = 300;

// ---------------------------------------------------------------------------
// 对外形状
// ---------------------------------------------------------------------------

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

/// 缓存里有一份且还没过期就直接用；过期则先试刷新，刷新不成再整取。发送路径的唯一入口。
pub async fn ensure_token(pool: &DbPool, cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    validate(cfg)?;
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

// ---------------------------------------------------------------------------
// 缓存键
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

/// 按流程检查必填项。**刻意不在反序列化期拒绝**——`auth_json` 里有四年的 `'{}'` 与
/// `bearer` 行，那些必须继续读得出来（见 [`OAuth2Auth`] 的模块注释）。缺字段推迟到
/// 真正要用 token 时才报，这时信息才指向「你少了哪个」。
pub fn validate(cfg: &OAuth2Auth) -> Result<(), AppError> {
    match cfg.grant_type {
        OAuth2GrantType::Implicit => {
            if cfg.authorization_url.trim().is_empty() {
                return Err(AppError::Validation(
                    "implicit 流程需要 Authorization URL".into(),
                ));
            }
        }
        OAuth2GrantType::ClientCredentials => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "client_credentials 流程需要 Client ID".into(),
                ));
            }
        }
        OAuth2GrantType::Password => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "password 流程需要 Client ID".into(),
                ));
            }
            // 密码允许为空：是否接受空密码是 IdP 的决定，不该由本应用替它答。
            if cfg.username.trim().is_empty() {
                return Err(AppError::Validation("password 流程需要 Username".into()));
            }
        }
        OAuth2GrantType::AuthorizationCode => {
            require_token_url(cfg)?;
            if cfg.client_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "authorization_code 流程需要 Client ID".into(),
                ));
            }
            if extract_code(&cfg.auth_code).is_empty() {
                return Err(AppError::Validation(
                    "authorization_code 流程需要授权码：请在浏览器登录后，把回调 URL \
                     或其中的 code 粘进「授权码」字段"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

fn require_token_url(cfg: &OAuth2Auth) -> Result<(), AppError> {
    if cfg.token_url.trim().is_empty() {
        return Err(AppError::Validation("需要 Token URL".into()));
    }
    match http_executor::check_url(
        cfg.token_url.trim(),
        http_executor::OutboundPolicy::default(),
    ) {
        http_executor::UrlVerdict::Allowed => Ok(()),
        // 出站策略拒绝原样透传：那是运维设的开关，用户需要看到**为什么**发不出去。
        http_executor::UrlVerdict::Refused(reason) => Err(AppError::Validation(reason)),
    }
}

fn implicit_needs_manual() -> AppError {
    AppError::Validation(
        "implicit 流程没有 token 端点：请在浏览器完成授权后，把整条回调 URL 粘进 \
         OAuth2 配置再点「保存 Token」"
            .into(),
    )
}

// ---------------------------------------------------------------------------
// token 请求
// ---------------------------------------------------------------------------

/// 按当前流程拼出一次完整取 token 的 form 字段。
fn build_grant_form(cfg: &OAuth2Auth) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = vec![(
        "grant_type".to_string(),
        match cfg.grant_type {
            OAuth2GrantType::AuthorizationCode => "authorization_code",
            OAuth2GrantType::ClientCredentials => "client_credentials",
            OAuth2GrantType::Password => "password",
            // 到不了这里：`implicit` 没有 token 端点，`validate` 先拦下。
            OAuth2GrantType::Implicit => "implicit",
        }
        .to_string(),
    )];

    if !cfg.scope.trim().is_empty() {
        form.push(("scope".into(), cfg.scope.trim().to_string()));
    }
    match cfg.grant_type {
        OAuth2GrantType::AuthorizationCode => {
            form.push(("code".into(), extract_code(&cfg.auth_code)));
            if !cfg.redirect_uri.trim().is_empty() {
                form.push(("redirect_uri".into(), cfg.redirect_uri.trim().to_string()));
            }
            if !cfg.code_verifier.trim().is_empty() {
                form.push(("code_verifier".into(), cfg.code_verifier.trim().to_string()));
            }
        }
        OAuth2GrantType::Password => {
            form.push(("username".into(), cfg.username.clone()));
            form.push(("password".into(), cfg.password.clone()));
        }
        _ => {}
    }
    append_extra_params(&mut form, cfg);
    append_client_credentials(&mut form, cfg);
    form
}

/// 用 `refresh_token` 换新的 access token（RFC 6749 §6）。
fn build_refresh_form(cfg: &OAuth2Auth, refresh_token: &str) -> Vec<(String, String)> {
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
    ];
    if !cfg.scope.trim().is_empty() {
        form.push(("scope".into(), cfg.scope.trim().to_string()));
    }
    append_extra_params(&mut form, cfg);
    // 刷新时 `client_auth` 仍是同一份凭据，且按 §6 建议收窄 scope 时可省略——
    // 这里沿用配置里的 scope，用户想沿用服务端默认值就把字段清空。
    append_client_credentials(&mut form, cfg);
    form
}

/// 客户端凭据：放在 Basic 头里就不进 form（RFC 6749 §2.3.1）。
fn append_client_credentials(form: &mut Vec<(String, String)>, cfg: &OAuth2Auth) {
    if cfg.client_auth != OAuth2ClientAuth::Body {
        return;
    }
    form.push(("client_id".into(), cfg.client_id.clone()));
    if !cfg.client_secret.is_empty() {
        form.push(("client_secret".into(), cfg.client_secret.clone()));
    }
}

fn append_extra_params(form: &mut Vec<(String, String)>, cfg: &OAuth2Auth) {
    for kv in &cfg.extra_params {
        if kv.enabled && !kv.key.trim().is_empty() {
            form.push((kv.key.clone(), kv.value.clone()));
        }
    }
}

async fn fetch_via_http(cfg: &OAuth2Auth) -> Result<OAuth2Token, AppError> {
    let form = build_grant_form(cfg);
    token_request(cfg, &form).await
}

async fn refresh_via_http(cfg: &OAuth2Auth, refresh_token: &str) -> Result<OAuth2Token, AppError> {
    let form = build_refresh_form(cfg, refresh_token);
    token_request(cfg, &form).await
}

async fn token_request(cfg: &OAuth2Auth, form: &[(String, String)]) -> Result<OAuth2Token, AppError> {
    let mut headers: Vec<(String, String)> = vec![
        (
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ),
        ("Accept".into(), "application/json".into()),
    ];
    // 走 Basic 时凭据不进 body。RFC 6749 §2.3.1 要求先对两个值做
    // form 编码再 base64 —— 直接 base64(`id:secret`) 会在 secret 含
    // `:` / `+` / 空格时被服务端解出另一个值。
    if cfg.client_auth == OAuth2ClientAuth::Basic
        && !cfg.client_id.trim().is_empty()
        && !cfg.client_secret.is_empty()
    {
        let raw = format!("{}:{}", form_encode(cfg.client_id.trim()), form_encode(&cfg.client_secret));
        headers.push((
            "Authorization".into(),
            format!("Basic {}", crate::base64::encode(raw.as_bytes())),
        ));
    }

    let input = HttpRequestInput {
        method: "POST".into(),
        url: cfg.token_url.trim().to_string(),
        headers,
        body: Some(encode_form(form)),
        timeout_ms: TOKEN_TIMEOUT_MS,
        // IdP 用 302 把用户送回 IdP 域的登录页是正常交互，不该在这里撞墙。
        follow_redirects: true,
        max_response_size_kb: TOKEN_MAX_RESPONSE_KB,
    };

    let result = http_executor::execute(&input).await;
    let resp = result.response.ok_or_else(|| {
        let msg = result
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| "token 请求失败".into());
        AppError::Http(msg)
    })?;
    parse_token_response(resp.status, &resp.body, &resp.content_type)
}

// ---------------------------------------------------------------------------
// 响应解析
// ---------------------------------------------------------------------------

/// token 响应 → [`OAuth2Token`]。RFC 6749 §5.1 定义 JSON；少数 IdP 仍回
/// `application/x-www-form-urlencoded`，故两种都收。
fn parse_token_response(
    status: u16,
    body: &str,
    content_type: &str,
) -> Result<OAuth2Token, AppError> {
    let pairs = read_response_pairs(body, content_type);

    if !(200..300).contains(&status) {
        let code = lookup(&pairs, "error").unwrap_or_else(|| format!("http_{status}"));
        let detail = lookup(&pairs, "error_description");
        return Err(AppError::Validation(match detail {
            Some(d) if !d.trim().is_empty() => {
                format!("token 端点返回 {status} {code}: {}", clip(&d))
            }
            _ => format!("token 端点返回 {status} {code}"),
        }));
    }

    let access_token = lookup(&pairs, "access_token").unwrap_or_default();
    if access_token.is_empty() {
        return Err(AppError::Validation(
            "token 响应里没有 access_token".into(),
        ));
    }
    Ok(OAuth2Token {
        access_token,
        // 刷新响应按 §6 可能不重复 refresh_token，调用方负责沿用旧值。
        refresh_token: lookup(&pairs, "refresh_token").filter(|v| !v.is_empty()),
        token_type: lookup(&pairs, "token_type")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "Bearer".into()),
        scope: lookup(&pairs, "scope").filter(|v| !v.is_empty()),
        expires_at: lookup(&pairs, "expires_in")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|s| *s > 0)
            .map(|s| unix_secs() + s),
        obtained_at: unix_secs(),
    })
}

/// 用户粘贴的原文 → token 各字段。支持整条回调 URL（`#access_token=…` 或
/// `?access_token=…`）与裸 token 串。
struct PastedToken {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    expires_in: Option<i64>,
}

fn parse_pasted(pasted: &str) -> Result<PastedToken, AppError> {
    let raw = pasted.trim();
    if raw.is_empty() {
        return Err(AppError::Validation("粘贴的内容是空的".into()));
    }

    // 回调 URL：query 与 fragment 都要看——IdP 走 implicit 时把 token 放 fragment
    // （RFC 6749 §4.2.2），走自定义回调时有人放 query。
    for sep in ['?', '#'] {
        let Some(pos) = raw.find(sep) else { continue };
        let pairs = form_pairs(&raw[pos + 1..]);
        if let Some(at) = lookup(&pairs, "access_token").filter(|v| !v.is_empty()) {
            return Ok(PastedToken {
                access_token: at,
                refresh_token: lookup(&pairs, "refresh_token").filter(|v| !v.is_empty()),
                token_type: lookup(&pairs, "token_type")
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or_else(|| "Bearer".into()),
                expires_in: lookup(&pairs, "expires_in")
                    .and_then(|v| v.trim().parse::<i64>().ok()),
            });
        }
    }

    // 裸 token（dashboard 复制、CLI 输出）。
    Ok(PastedToken {
        access_token: raw.to_string(),
        refresh_token: None,
        token_type: "Bearer".into(),
        expires_in: None,
    })
}

/// 从「授权码」输入里取出真正的 `code`。接受三种写法：整条回调 URL、裸 code、
/// `code=xxx`。URL 的 query 与 fragment 都看——回环回调常把 code 放 fragment。
///
/// **像回调 URL 却取不到 code 时返回空串**，好让 `validate` 报「需要授权码」。
/// 把整条 URL 当 code 送出去只会在 IdP 那边换来一个更费解的 400。
fn extract_code(raw: &str) -> String {
    let raw = raw.trim();
    let mut looks_like_callback = false;
    for sep in ['?', '#'] {
        let Some(pos) = raw.find(sep) else { continue };
        looks_like_callback = true;
        if let Some(code) = lookup(&form_pairs(&raw[pos + 1..]), "code").filter(|v| !v.is_empty()) {
            return code;
        }
    }
    if looks_like_callback {
        return String::new();
    }
    if let Some(rest) = raw.strip_prefix("code=") {
        return form_decode(rest);
    }
    raw.to_string()
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// `Authorization` 的完整取值：`<token_type> <access_token>`（RFC 6749 §7.1）。
///
/// `token_type` 原样透传 IdP 回的值：它多数是 `Bearer`，少数是 `MAC` / `Token`，
/// 由服务端定义，替它改成 `Bearer` 会打错请求。
pub fn auth_header_value(token: &OAuth2Token) -> String {
    let ty = token.token_type.trim();
    if ty.is_empty() {
        token.access_token.clone()
    } else {
        format!("{ty} {}", token.access_token)
    }
}

/// query 位置的 token 取值。与 header 档不同：query 位置按 RFC 6749 §8.3 只带裸
/// `access_token`，**不带** `token_type`——那会变成另一个参数值。
pub fn auth_query_value(token: &OAuth2Token) -> String {
    token.access_token.clone()
}

/// token 该放哪（`tokenLocation` 为 query 时调用方改走 URL 追加）。
pub fn goes_in_query(cfg: &OAuth2Auth) -> bool {
    cfg.token_location == OAuth2TokenLocation::Query
}

// ---------------------------------------------------------------------------
// 缓存读写
// ---------------------------------------------------------------------------

fn store(
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
fn is_expired(token: &OAuth2Token) -> bool {
    match token.expires_at {
        Some(at) => unix_secs() + EXPIRY_SKEW_SECS >= at,
        None => false,
    }
}

/// 「这次是不是刚取来的」——给 [`resolve`] 的 `refreshed` 标记用：命中缓存时它该是
/// `false`，否则 UI 会把一个两小时前的 token 说成刚刷新。
fn is_fresh(token: &OAuth2Token) -> bool {
    unix_secs().saturating_sub(token.obtained_at) < 5
}

// ---------------------------------------------------------------------------
// form 编解码
// ---------------------------------------------------------------------------

fn encode_form(form: &[(String, String)]) -> String {
    let mut out = String::new();
    for (k, v) in form {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(&form_encode(k));
        out.push('=');
        out.push_str(&form_encode(v));
    }
    out
}

/// `application/x-www-form-urlencoded` 编码：未保留字符一律 `%XX`，空格是 `+`。
/// 与 `url::form_urlencoded` 的输出逐字节一致——不引 `url` crate 只为这 20 行。
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push(hex_upper(other >> 4));
                out.push(hex_upper(other & 0x0F));
            }
        }
    }
    out
}

fn form_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi << 4 | lo);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_upper(nibble: u8) -> char {
    char::from_digit(nibble as u32, 16).unwrap_or('0').to_ascii_uppercase()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `a=1&b=2` → 键值对。`&` / `=` 两端都可缺（`?flag` / `=v`）。
fn form_pairs(qs: &str) -> Vec<(String, String)> {
    qs.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (form_decode(k), form_decode(v)),
            None => (form_decode(pair), String::new()),
        })
        .collect()
}

fn lookup(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.to_string())
}

/// 按 content-type 选解析器；没有该头时先试 JSON——IdP 漏发 content-type 的不少。
fn read_response_pairs(body: &str, content_type: &str) -> Vec<(String, String)> {
    if content_type.contains("x-www-form-urlencoded") {
        return form_pairs(body.trim());
    }
    if content_type.contains("json") || content_type.is_empty() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            return flatten_json(&v);
        }
    }
    // 兜底：content-type 写着别的东西，但 body 其实是 JSON（太常见了）。
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        return flatten_json(&v);
    }
    form_pairs(body.trim())
}

/// OAuth2 的响应体是**扁平**的（§5.1），但 IdP 常在同一个 JSON 里附带别的信息，
/// 故递归摊平成 `a.b` 形式，取顶层键的行为不变。
fn flatten_json(v: &serde_json::Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    flatten_into(v, "", &mut out);
    out
}

fn flatten_into(v: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_into(val, &key, out);
            }
        }
        serde_json::Value::String(s) => out.push((prefix.to_string(), s.clone())),
        serde_json::Value::Number(n) => out.push((prefix.to_string(), n.to_string())),
        serde_json::Value::Bool(b) => out.push((prefix.to_string(), b.to_string())),
        _ => {}
    }
}

fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= MAX_ERROR_DETAIL {
        return s;
    }
    let cut: String = s.chars().take(MAX_ERROR_DETAIL).collect();
    format!("{cut}…")
}

fn unix_secs() -> i64 {
    now_ms() / 1000
}

#[cfg(test)]
#[path = "oauth2_test.rs"]
mod tests;