//! `api_run_collection` —— 把一个容器（集合或模块）下面的接口逐个发出去，给一份报告。
//!
//! **顺序执行**：环境变量的写回由脚本产生，并发时写回的先后不可预期，报告也就不可
//! 复现。顺序执行时「第几条看到的是哪一份变量」是确定的。
//!
//! **与应用内那份同一套规则**：端点清单取自
//! `db::nodes::ordered_subtree_endpoints`（与应用内那份由
//! `fixtures/collection-order-parity.json` 钉住），请求装配走
//! `variable_fill::resolve_endpoint_with` —— 即与单条发送同一个解析器。故两侧对同一棵
//! 集合给出的清单与请求内容是同一份。
//!
//! **两道门**：全局能力门要求 `Capability::Send`，故只有 `full_access` 能看见并调用
//! 本工具；按项目的权限门在这里过，用的是目标节点**真实归属**的项目 id。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::mcp::{AppState, CallToolResult};
use simple_post_core::domain::{
    EndpointDetail, Environment, Project, RequestSettings, ResourceNode,
};
use simple_post_core::error::AppError;

use super::args::RunCollectionArgs;
use super::dynamic::resolve_dynamic_values;
use super::input::{extract_endpoint_settings, format_method, resolve_tls_override};
use super::send::{execute_resolved, resolve_settings};

/// 一条端点跑完之后的结论。
struct ItemOutcome {
    success: bool,
    /// HTTP 状态码。没拿到响应时是 0。
    status: u16,
    status_text: String,
    duration_ms: u64,
    /// 失败分类：`http`（拿到了响应但不是 2xx）/ `transport`（没拿到响应）/
    /// `blocked`（压根没发出去）。成功时是 `None`。
    failure: Option<&'static str>,
    error: Option<String>,
    missing_vars: Vec<String>,
    missing_path_params: Vec<String>,
}

pub(super) async fn tool_api_run_collection(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: RunCollectionArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("api_run_collection args: {e}")))?;

    // 目标：集合或模块，二选一。两个都给或都不给都是调用方没说清楚要跑哪个。
    let root_id = match (&a.collection_id, &a.module_id) {
        (Some(c), None) => c.clone(),
        (None, Some(m)) => m.clone(),
        (Some(_), Some(_)) => {
            return Err(AppError::Validation(
                "api_run_collection takes collection_id or module_id, not both".into(),
            ))
        }
        (None, None) => {
            return Err(AppError::Validation(
                "api_run_collection needs collection_id or module_id".into(),
            ))
        }
    };

    // 真实归属项目。按项目的权限门要的就是它：`agent_permissions.project_id` 是指向
    // `projects(id)` 的外键，编出来的 id 永远没有对应的行，于是这道门形同虚设。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &root_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "api_run_collection").await?;

    // 端点清单：子树里的全部端点，按树的显示顺序。
    let endpoints = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::ordered_subtree_endpoints(c, &root_id)
    })?;

    // 项目行取不到就整批失败：目标节点还在树上而项目没了，是数据不一致，
    // 静默换成空项目会让请求在没有 base URL 的情况下发出去。
    let project = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::projects::get(c, &project_id)
    })?;
    let project_ref: &Project = &project;

    // 环境快照：整批都用这一份，脚本写回的变量不会渗进后面那几条。
    let envs: Vec<Environment> = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::environments::list_by_project(c, &project_id)
    })?
    .iter()
    .map(freeze_environment)
    .collect();

    let settings = resolve_settings(
        &state.pool,
        extract_endpoint_settings(&a.settings_override),
        a.settings_override.as_ref(),
    );

    let mut items: Vec<Value> = Vec::with_capacity(endpoints.len());
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    let mut no_response = 0usize;
    let mut total_duration_ms = 0u64;
    let mut status_counts: BTreeMap<u16, usize> = BTreeMap::new();

    for node in &endpoints {
        // 单条失败**不中断**整批：这一条记成失败，下一条照发。
        let outcome = match run_one(
            state,
            node,
            &envs,
            &project_id,
            project_ref,
            &settings,
            a.settings_override.as_ref(),
        )
        .await
        {
            Ok(o) => o,
            Err(err) => ItemOutcome {
                success: false,
                status: 0,
                status_text: String::new(),
                duration_ms: 0,
                failure: Some("transport"),
                error: Some(err.to_string()),
                missing_vars: Vec::new(),
                missing_path_params: Vec::new(),
            },
        };

        total_duration_ms += outcome.duration_ms;
        if outcome.success {
            success += 1;
        } else {
            failed += 1;
        }
        // 四类互斥且穷尽，故总数不会算错：压根没发出去 / 发了但没拿到响应 / 拿到了
        // 响应（按状态码进分布）。
        match outcome.failure {
            Some("blocked") => blocked += 1,
            _ if outcome.status == 0 => no_response += 1,
            _ => {
                *status_counts.entry(outcome.status).or_insert(0) += 1;
            }
        }

        items.push(json!({
            "endpoint_id": node.id,
            "name": node.name,
            "success": outcome.success,
            "status": outcome.status,
            "status_text": outcome.status_text,
            "duration_ms": outcome.duration_ms,
            "failure": outcome.failure,
            "error": outcome.error,
            "missing_vars": outcome.missing_vars,
            "missing_path_params": outcome.missing_path_params,
        }));
    }

    let status_counts: Vec<Value> = status_counts
        .into_iter()
        .map(|(status, count)| json!({ "status": status, "count": count }))
        .collect();

    Ok(CallToolResult::json(json!({
        "ok": true,
        "total": endpoints.len(),
        "success": success,
        "failed": failed,
        "not_run": 0,
        "status_counts": status_counts,
        "no_response": no_response,
        "blocked": blocked,
        "total_duration_ms": total_duration_ms,
        "items": items,
    })))
}

/// 跑一条端点，给出它的结论。
///
/// `Err` 只在「这一步压根没法开始」时返回（端点没有请求配置这类数据问题）；传输失败
/// 与状态码不对都算「跑完了，结论是失败」，由 `ItemOutcome` 表达。
async fn run_one(
    state: &AppState,
    node: &ResourceNode,
    envs: &[Environment],
    project_id: &str,
    project: &Project,
    settings: &RequestSettings,
    settings_override: Option<&Value>,
) -> Result<ItemOutcome, AppError> {
    let detail: EndpointDetail = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, &node.id)
    })?;

    // 环境：端点自绑的优先，否则项目当前激活的那个 —— 选择规则由 `resolve_env` 那一处
    // 决定；这里换成快照里的那一份，脚本写回的变量才不会渗进后面那几条。
    let selected =
        crate::mcp::resolve_env(&state.pool, detail.environment_id.as_deref(), project_id)?;
    let env = selected.and_then(|e| envs.iter().find(|f| f.id == e.id));

    // 端点属于哪个模块只在树里，detail 行上没有，故要查一次：不补的结果是服务列表
    // 收窄到空，请求裸奔没有 base URL。数据库出错则这一条失败，不静默当成「没有模块」。
    let module_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::module_id_of(c, &node.id)
    })?;

    let overrides = crate::variable_fill::EndpointOverrides {
        module_id: module_id.clone(),
        ..Default::default()
    };

    // 动态变量的值必须在**替换之前**取到：取到的值并进变量池，同名时压过环境变量。
    let dynamic = resolve_dynamic_values(state, env, project, &detail, &overrides).await?;
    let resolved = crate::variable_fill::resolve_endpoint_with(
        &detail,
        env,
        project,
        Some(&overrides),
        &dynamic,
    )?;

    let tls_override = resolve_tls_override(settings_override, detail.tls.clone())?;

    let executed = execute_resolved(
        state,
        &format_method(&detail.method),
        resolved,
        env,
        // 端点 id：后端据此装配脚本链（祖先集合的两段 + 端点自己的两段）。
        Some(node.id.clone()),
        settings.clone(),
        tls_override,
    )
    .await?;

    // 路径参数缺值时压根没发出去 —— 它按 OpenAPI 惯例是必填的，带着 `{id}` 发出去只会
    // 换一个含义不明的 404。
    if executed.blocked {
        return Ok(ItemOutcome {
            success: false,
            status: 0,
            status_text: String::new(),
            duration_ms: 0,
            failure: Some("blocked"),
            error: Some(format!(
                "path parameter(s) have no value: {}",
                executed.missing_path_params.join(", ")
            )),
            missing_vars: executed.missing_vars,
            missing_path_params: executed.missing_path_params,
        });
    }

    let (status, status_text, duration_ms) = match &executed.http.response {
        Some(resp) => (
            resp.status,
            resp.status_text.clone(),
            resp.duration_ms as u64,
        ),
        None => (0, String::new(), 0),
    };
    let success = (200..300).contains(&status);
    let failure = if success {
        None
    } else if status > 0 {
        Some("http")
    } else {
        Some("transport")
    };
    let error = if success {
        None
    } else {
        executed
            .http
            .error
            .as_ref()
            .map(|e| format!("{}: {}", e.kind, e.message))
            .or_else(|| Some(format!("HTTP {status}")))
    };
    Ok(ItemOutcome {
        success,
        status,
        status_text,
        duration_ms,
        failure,
        error,
        missing_vars: executed.missing_vars,
        missing_path_params: executed.missing_path_params,
    })
}

/// 拍一份环境快照。
///
/// 脚本可以把变量写回环境。若批量运行读的是库里的环境，那么第 1 条写回的变量会出现在
/// 第 2 条的替换里，「这一批看到的是同一份变量」就不成立了。故批次开始时把环境复制
/// 一份，整批都用这份副本：写回仍然落库（core 的脚本宿主做），但**不影响**这一批
/// 后面那几条看到的值。
fn freeze_environment(env: &Environment) -> Environment {
    Environment {
        variables: env.variables.clone(),
        disabled_variables: env.disabled_variables.clone(),
        module_base_urls: env
            .module_base_urls
            .iter()
            .map(|(def_id, cells)| (def_id.clone(), cells.clone()))
            .collect(),
        ..env.clone()
    }
}
