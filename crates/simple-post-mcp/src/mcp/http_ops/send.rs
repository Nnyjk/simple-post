//! 发送路径 —— 一份已解析好的请求 → 发出 → 结果信封。
//!
//! 两个发送工具共用它，故「解析之后发生什么」只有一份实现：oauth2 的 token 注入、
//! multipart 的装配、脚本链、发送、结果封装。

use serde_json::Value;

use crate::mcp::{AppState, CallToolResult};
use crate::variable_fill::ResolvedEndpoint;
use simple_post_core::db::DbPool;
use simple_post_core::domain::{Environment, RequestSettings};
use simple_post_core::error::AppError;

use super::envelope::build_result_envelope;
use super::multipart;

/// 一份已解析好的请求 → 发出 → 结果信封。
///
/// `endpoint_id` 决定脚本链能不能装配（有它才有 collection 那几段）；临时请求没有
/// 端点，故传 `None` —— 它的请求里没有脚本可跑。
pub(super) async fn send_resolved(
    state: &AppState,
    method: &str,
    resolved: ResolvedEndpoint,
    env: Option<&Environment>,
    endpoint_id: Option<String>,
    settings: RequestSettings,
) -> Result<CallToolResult, AppError> {
    let ResolvedEndpoint {
        url,
        headers,
        body,
        body_mode,
        auth,
        missing_vars,
        missing_path_params,
        ..
    } = resolved;

    // `send_resolved` 在路径参数缺值时就不发：它按 OpenAPI 惯例是必填的，带着
    // `{id}` 发出去只会换一个含义不明的 404，而用户看不出是自己漏填了一个值。
    if !missing_path_params.is_empty() {
        return Err(AppError::Validation(format!(
            "path parameter(s) have no value: {}. Set them on the endpoint's path \
             parameters, or pass overrides.pathParams when calling.",
            missing_path_params.join(", ")
        )));
    }
    let mut url = url;
    let mut headers = headers;

    // `send_resolved` 在发请求之前才取 oauth2 的 token：`resolve_endpoint`（纯函数）
    // 拿不到它，故注入放在这里而不是那里——`build_headers` 对 bearer/basic/apikey 的
    // 处理不受影响。
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

    // `send_resolved` 在这里才把 multipart 装配成形：boundary 必须现生成，Content-Type
    // 与 body 用同一个值。`resolve_endpoint` 只做变量替换，故它给不出带 boundary 的
    // Content-Type。
    //
    // 判据读 `resolved.body_mode`（**套过 overrides 的那个 mode**），不读端点行里存的那个：
    // 调用方给 `overrides.body` 就能换掉 mode，而 Content-Type 那边读的是
    // `resolve_endpoint` 内部同一个 eff_body.mode。两边读不同的值会出两种坏结果——
    // 存 json / 覆盖 multipart 时整条请求没有 Content-Type 且分片原样发出；存 multipart /
    // 覆盖 json 时 body 被换成只有收尾分隔符的壳，调用方传的内容静默消失。
    let mut body = body;
    if body_mode == simple_post_core::domain::BodyMode::Multipart {
        body = body
            .take()
            .map(|content| multipart::apply(&mut headers, &content));
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
            },
            env: env_snapshot(env),
            settings,
            // MCP 端没有「界面上未保存的草稿」这回事，用的就是存的那份脚本。
            script_override: None,
        },
    )
    .await?;

    state.metrics.inc();
    Ok(build_result_envelope(
        method,
        &outcome.request.url,
        &outcome.http,
        None,
        Some(missing_vars),
        outcome.script,
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
