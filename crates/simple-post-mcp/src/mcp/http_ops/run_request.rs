//! `api_run_request` —— 不挂端点的一次性请求。
//!
//! 临时请求先被拼成一份内存里的 `EndpointDetail`，再交给
//! `variable_fill::resolve_endpoint` 解析，故「变量替换 / base URL 拼接 / 鉴权派生 /
//! header 排序 / body 判空」这些规则与存着的端点只有一份实现。

use serde_json::Value;

use crate::mcp::{AppState, CallToolResult};
use simple_post_core::domain::{EndpointDetail, RequestBody};
use simple_post_core::error::AppError;

use super::args::RunRequestArgs;
use super::dynamic::resolve_dynamic_values;
use super::input::{
    extract_settings, flatten_key_values, format_method, parse_auth, parse_body_mode,
    resolve_tls_override,
};
use super::project::project_for;
use super::send::{resolve_settings, send_resolved};

pub(super) async fn tool_api_run_request(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: RunRequestArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_run_request args: {e}")))?;
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }

    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    let headers = flatten_key_values(a.headers.as_ref(), "headers")?;
    let params = flatten_key_values(a.params.as_ref(), "params")?;
    let path_params = flatten_key_values(a.path_params.as_ref(), "path_params")?;
    let auth = parse_auth(a.auth.as_ref())?;
    let content = a.body.clone().unwrap_or_default();
    let body_mode = parse_body_mode(a.body_mode.as_deref(), !content.is_empty())?;

    // `project_id` 的来源：环境属于项目，故给了环境 id 就知道项目。没给环境就没有
    // 项目归属，base URL 那一格也无从读。
    let project_id: Option<String> = match a.env_id.as_deref() {
        Some(env_id) => simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::environments::lookup_project_id(c, env_id)
        })
        .ok(),
        None => None,
    };

    // 环境：给了 id 就用它；没给就取项目当前激活的那个 —— 与 `api_call_endpoint`
    // 同一套规则（`mcp::resolve_env`）。取不到（环境已被删）时当作这次请求不做
    // 变量替换，不让整次调用失败。
    let env = crate::mcp::resolve_env(
        &state.pool,
        a.env_id.as_deref(),
        project_id.as_deref().unwrap_or(""),
    )?;

    let project = project_for(&state.pool, project_id.as_deref());

    // 拼出的 `detail` 没有 `nodes` 行，故 `node_id` 是占位值 —— 解析器不用它。
    let detail = EndpointDetail {
        node_id: String::new(),
        method: method.clone(),
        url: a.url.clone(),
        environment_id: a.env_id.clone(),
        base_url_override: None,
        params,
        path_params,
        headers,
        body: RequestBody {
            mode: body_mode,
            content,
            schema: None,
        },
        auth,
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        // 临时请求没有端点，故它没有「端点自带的那份」HTTPS 设置。
        tls: None,
    };

    let overrides = crate::variable_fill::EndpointOverrides {
        module_id: a.module_id.clone(),
        ..Default::default()
    };
    // 动态变量的值必须在替换之前取到：取到的值并进变量池，同名时压过环境变量。
    let dynamic =
        resolve_dynamic_values(state, env.as_ref(), &project, &detail, &overrides).await?;
    let resolved = crate::variable_fill::resolve_endpoint_with(
        &detail,
        env.as_ref(),
        Some(&overrides),
        &dynamic,
    )?;

    let settings = resolve_settings(
        &state.pool,
        extract_settings(&a.settings_override, &a),
        a.settings_override.as_ref(),
    );

    // HTTPS 出站设置：临时请求没有端点，故只有本次调用的临时覆盖这一档；它缺席时
    // 继承全局默认。
    let tls_override = resolve_tls_override(a.settings_override.as_ref(), None)?;

    send_resolved(
        state,
        &format_method(&method),
        resolved,
        env.as_ref(),
        None,
        settings,
        tls_override,
    )
    .await
}
