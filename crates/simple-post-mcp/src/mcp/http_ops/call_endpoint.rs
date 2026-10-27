//! `api_call_endpoint` —— 按端点 id 解析并发出。
//!
//! 端点行读出来之后交给 `variable_fill::resolve_endpoint` 解析，故「变量替换 /
//! base URL 拼接 / 鉴权派生 / header 排序 / body 判空」这些规则与临时请求共用
//! 同一份实现。

use serde_json::Value;

use crate::mcp::{AppState, CallToolResult};
use simple_post_core::domain::{EndpointDetail, Project};
use simple_post_core::error::AppError;

use super::args::CallEndpointArgs;
use super::dynamic::resolve_dynamic_values;
use super::input::{extract_endpoint_settings, format_method};
use super::send::{resolve_settings, send_resolved};
use super::EMPTY_PROJECT;

pub(super) async fn tool_api_call_endpoint(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: CallEndpointArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_call_endpoint args: {e}")))?;
    let endpoint_id = a.endpoint_id.clone();

    let endpoint: EndpointDetail = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, &endpoint_id)
    })?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &endpoint.node_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "api_call_endpoint").await?;

    // 环境：端点自己绑定的优先；没绑定就取项目当前激活的那个 —— 与界面那侧一致
    // （界面用的是工作区当前环境，而工作区的切换会写 `environments.is_active`）。
    let env =
        crate::mcp::resolve_env(&state.pool, endpoint.environment_id.as_deref(), &project_id)?;

    let project = if !project_id.is_empty() {
        simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::projects::get(c, &project_id)
        })
        .ok()
    } else {
        None
    };

    // `overrides` 反序列化失败是**错误**，不是「当作没传」：用 `.ok()` 会把一整份
    // 覆盖值悄悄换成空值，调用方看到的是「按端点自己那份发出去的请求」，而不是
    // 「你传的 overrides 格式不对」。
    let overrides: crate::variable_fill::EndpointOverrides = match a.overrides.as_ref() {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| AppError::Validation(format!("api_call_endpoint overrides: {e}")))?,
        None => Default::default(),
    };

    // `overrides.module_id` 缺省时要查 `nodes` 补齐：端点属于哪个模块只在树里，
    // detail 行上没有。调用方自己指定了就不覆盖 —— `resolve_endpoint` 是纯函数，
    // 别让它被一次查库改写。不补的结果是服务列表收窄到空，请求裸奔没有 base URL。
    let mut overrides = overrides;
    if overrides.module_id.is_none() {
        overrides.module_id = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::nodes::module_id_of(c, &endpoint.node_id)
        })
        .ok()
        .flatten();
    }

    let project_ref: &Project = project.as_ref().unwrap_or(&EMPTY_PROJECT);
    // 动态变量的值必须在替换之前取到：取到的值并进变量池，同名时压过环境变量。
    let dynamic =
        resolve_dynamic_values(state, env.as_ref(), project_ref, &endpoint, &overrides).await?;
    let resolved = crate::variable_fill::resolve_endpoint_with(
        &endpoint,
        env.as_ref(),
        project_ref,
        Some(&overrides),
        &dynamic,
    )?;

    let settings = resolve_settings(
        &state.pool,
        extract_endpoint_settings(&a.settings_override),
        a.settings_override.as_ref(),
    );

    send_resolved(
        state,
        &format_method(&endpoint.method),
        resolved,
        env.as_ref(),
        Some(endpoint.node_id.clone()),
        settings,
    )
    .await
}
