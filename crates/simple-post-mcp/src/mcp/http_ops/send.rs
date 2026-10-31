//! 发送路径 —— 一份已解析好的请求 → 发出 → 结果信封。
//!
//! 两个发送工具共用它，故「解析之后发生什么」只有一份实现：oauth2 的 token 注入、
//! multipart 的装配、脚本链、发送、结果封装。

use serde_json::Value;

use crate::mcp::{AppState, CallToolResult};
use crate::variable_fill::ResolvedEndpoint;
use simple_post_core::db::DbPool;
use simple_post_core::domain::{Environment, RequestSettings, TlsSettings};
use simple_post_core::error::AppError;

use super::envelope::build_result_envelope;
use super::multipart;

/// 一份已解析好的请求 → 发出 → 结果信封。
///
/// `endpoint_id` 决定脚本链能不能装配（有它才有 collection 那几段）；临时请求没有
/// 端点，故传 `None` —— 它的请求里没有脚本可跑。
///
/// 解析之后、结果封装之前的一次执行。
///
/// 「解析之后发生什么」只有一份实现：OAuth2 的 token 注入、multipart 的装配、脚本链、
/// 发送。单条发送与批量运行都走它，故两者不会长出两套 —— 两者只是**结果封装**不同：
/// 单条把结果封成一次调用的信封，批量把每一条的结论收进一份报告。
pub(super) struct ExecutedRequest {
    /// 真正发出去的那份 URL。OAuth2 注入 token 之后是那一份。
    pub url: String,
    /// 传输层的信封。没拿到响应时 `response` 是 `None`，原因在 `error` 里。
    pub http: simple_post_core::http_executor::HttpResult,
    /// 脚本的产出。链上没有任何脚本时是 `None`。
    pub script: Option<simple_post_core::domain::ScriptOutcome>,
    /// 没解析出来的变量名。
    pub missing_vars: Vec<String>,
    /// 没取到值的路径占位符名。
    pub missing_path_params: Vec<String>,
    /// 压根没发出去（路径参数缺值）。为 true 时没有响应。
    pub blocked: bool,
}

/// 解析好的请求 → 发出 → 执行结果。
///
/// `endpoint_id` 决定脚本链能不能装配（有它才有 collection 那几段）；临时请求没有
/// 端点，故传 `None` —— 它的请求里没有脚本可跑。
///
/// `tls_override` 是这次请求生效的 HTTPS 设置里**端点以上的那一份**：全局默认由
/// `settings.tls` 带来，端点自带的那份与本次调用的临时覆盖已在调用方合成好。
/// `None` = 继承全局默认。合成由 core 的 `send_with_scripts` 完成（它在
/// `crate::tls::resolve` 里把两档合起来），故 MCP 侧只交出该交的那一份。
pub(super) async fn execute_resolved(
    state: &AppState,
    method: &str,
    resolved: ResolvedEndpoint,
    env: Option<&Environment>,
    endpoint_id: Option<String>,
    settings: RequestSettings,
    tls_override: Option<TlsSettings>,
) -> Result<ExecutedRequest, AppError> {
    let ResolvedEndpoint {
        url,
        headers,
        body,
        body_base64,
        body_mode,
        auth,
        missing_vars,
        missing_path_params,
        ..
    } = resolved;

    // 路径参数缺值时就不发：它按 OpenAPI 惯例是必填的，带着 `{id}` 发出去只会换一个
    // 含义不明的 404，而用户看不出是自己漏填了一个值。
    //
    // 这里**不**取 OAuth2 的 token：那条请求压根不会发出去，取 token 是一次没有意义
    // 的出网请求。
    if !missing_path_params.is_empty() {
        return Ok(ExecutedRequest {
            url,
            http: simple_post_core::http_executor::HttpResult {
                ok: false,
                response: None,
                error: None,
            },
            script: None,
            missing_vars,
            missing_path_params,
            blocked: true,
        });
    }
    let mut url = url;
    let mut headers = headers;

    // token 在发请求之前才取：`resolve_endpoint`（纯函数）拿不到它，故注入放在这里
    // 而不是那里——`build_headers` 对 bearer/basic/apikey 的处理不受影响。
    //
    // **必须同时匹配 `kind`**：`resolve_auth` 对未选中的方案原样保留载荷，所以一个从
    // oauth2 切到 bearer 的端点仍带着 `oauth2: Some(..)`。只匹配载荷时 header 档碰巧
    // 被 `build_headers` 已写好的 `Authorization` 挡住，而 query 档**不挡**——一个用户
    // 已经不再选择的 token 会被拼进 URL 发出去。
    if let simple_post_core::domain::AuthConfig {
        kind: simple_post_core::domain::AuthType::Oauth2,
        oauth2: Some(cfg),
        ..
    } = &auth
    {
        let token = simple_post_core::oauth2::ensure_token(&state.pool, cfg).await?;
        simple_post_core::oauth2::inject(cfg, &token, &mut headers, &mut url);
    }

    // multipart 在这里才装配成形：boundary 必须现生成，Content-Type 与 body 用同一个
    // 值。`resolve_endpoint` 只做变量替换，故它给不出带 boundary 的 Content-Type。
    //
    // 判据读 `resolved.body_mode`（**套过 overrides 的那个 mode**），不读端点行里存的那个：
    // 调用方给 `overrides.body` 就能换掉 mode，而 Content-Type 那边读的是
    // `resolve_endpoint` 内部同一个 eff_body.mode。两边读不同的值会出两种坏结果——
    // 存 json / 覆盖 multipart 时整条请求没有 Content-Type 且分片原样发出；存 multipart /
    // 覆盖 json 时 body 被换成只有收尾分隔符的壳，调用方传的内容静默消失。
    let mut body = body;
    let mut body_base64 = body_base64;
    if body_mode == simple_post_core::domain::BodyMode::Multipart {
        if let Some(content) = body.take() {
            let assembled = multipart::apply(&mut headers, &content);
            body = assembled.text;
            body_base64 = assembled.base64;
        }
    }

    // `send_with_scripts` 把脚本链跑在同一条序列里：前置脚本改写请求 → 发出 → 测试
    // 脚本断言并写回变量。整条序列由 core 一处实现，故桌面端与 MCP 端不会分叉。
    let outcome = simple_post_core::script::send_with_scripts(
        &state.pool,
        simple_post_core::script::ScriptedSendInput {
            endpoint_id,
            request: simple_post_core::script::ScriptRequest {
                method: method.to_string(),
                url: url.clone(),
                headers: headers.clone(),
                body,
                body_base64,
            },
            env: env_snapshot(env),
            settings,
            // MCP 端没有「界面上未保存的草稿」这回事，用的就是存的那份脚本。
            script_override: None,
            // HTTPS 出站设置：全局默认（`settings.tls`）与端点以上的那一份
            // （`tls_override`）由 core 的 `send_with_scripts` 合成，MCP 侧不合成。
            tls_override,
        },
    )
    .await?;

    state.metrics.inc();
    Ok(ExecutedRequest {
        url: outcome.request.url,
        http: outcome.http,
        script: outcome.script,
        missing_vars,
        missing_path_params,
        blocked: false,
    })
}

/// 解析好的请求 → 发出 → 一次调用的结果信封。
///
/// 与 [`execute_resolved`] 的差别只在最后一步：那条把结果封成一次调用的信封，这一条
/// 在路径参数缺值时报错（单条调用没有「整批」的上下文，一个失败就该让调用方看见）。
pub(super) async fn send_resolved(
    state: &AppState,
    method: &str,
    resolved: ResolvedEndpoint,
    env: Option<&Environment>,
    endpoint_id: Option<String>,
    settings: RequestSettings,
    tls_override: Option<TlsSettings>,
) -> Result<CallToolResult, AppError> {
    let missing_path_params = resolved.missing_path_params.clone();
    let executed = execute_resolved(
        state,
        method,
        resolved,
        env,
        endpoint_id,
        settings,
        tls_override,
    )
    .await?;
    if executed.blocked {
        return Err(AppError::Validation(format!(
            "path parameter(s) have no value: {}. Set them on the endpoint's path \
             parameters, or pass overrides.pathParams when calling.",
            missing_path_params.join(", ")
        )));
    }
    Ok(build_result_envelope(
        method,
        &executed.url,
        &executed.http,
        None,
        Some(executed.missing_vars),
        executed.script,
    ))
}

/// 脚本看得到的环境。**与解析用的是同一套变量池**：停用的变量不参与，脚本看到的
/// 环境与这次请求看到的是同一个。
pub(crate) fn env_snapshot(env: Option<&Environment>) -> simple_post_core::script::EnvSnapshot {
    let variables = env
        .map(|e| {
            e.variables
                .iter()
                .filter(|(k, _)| !e.disabled_variables.iter().any(|d| d == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    simple_post_core::script::EnvSnapshot {
        id: env.map(|e| e.id.clone()),
        variables,
    }
}

/// 本次发送用的出站旋钮（超时 / 跟随重定向 / 最大响应体积）。
///
/// 起点是**用户配置的那一份**（`preferences` 的 `request_settings` 行，桌面端在设置
/// 面板里改的就是它），调用方的覆盖值再盖上去。故界面上把超时调成 5 秒，MCP 侧也是
/// 5 秒；行缺失时用 `RequestSettings::default()`。
///
/// **`settings.tls` 不在这里被覆盖**：它是 HTTPS 出站的**全局默认**，而本次调用的
/// 临时覆盖要压过端点自带的那一份，故它走 `send_resolved` 的 `tls_override` 参数，
/// 由 core 的 `send_with_scripts` 与全局默认合成。写进 `settings.tls` 的话，端点
/// 自带的那份就被顶掉了。
pub(crate) fn resolve_settings(
    pool: &DbPool,
    flat: (Option<u64>, Option<bool>, Option<u64>),
    override_: Option<&Value>,
) -> RequestSettings {
    let mut settings = simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::preferences::request_settings(c)
    })
    .unwrap_or_default();
    if let Some(t) = flat.0 {
        settings.timeout_ms = t;
    }
    if let Some(r) = flat.1 {
        settings.follow_redirects = r;
    }
    if let Some(m) = flat.2 {
        settings.max_response_size_kb = m;
    }
    if let Some(v) = override_ {
        if let Some(n) = v.get("timeout_ms").and_then(Value::as_u64) {
            settings.timeout_ms = n;
        }
        if let Some(b) = v.get("follow_redirects").and_then(Value::as_bool) {
            settings.follow_redirects = b;
        }
        if let Some(n) = v.get("max_response_size_kb").and_then(Value::as_u64) {
            settings.max_response_size_kb = n;
        }
    }
    settings
}
