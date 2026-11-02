//! 「这个端点还缺什么」的提示。
//!
//! 端点建好了不等于能发出去。两种最常见的缺口：
//!
//! 1. **base URL 那一格没填。** URL 住在 (环境 × 模块) 的单元格里，那个格子
//!    **没有任何上层可借** —— 没填就是发出去一条裸路径。
//! 2. **环境里没有它引用的变量。** `{{token}}` 原样发出去，服务端收到的是字面量。
//!
//! 两种都不会让建端点失败，所以只能靠提示告诉 agent。提示随写工具的响应回去，
//! agent 一次就知道要补什么，而不用先发一次请求、看报错、再回头补一轮。
//!
//! 判据一律取自现成的实现，不另写一套：base URL 用
//! [`crate::variable_fill::resolve_base_url`]，变量扫描用
//! [`simple_post_core::dynamic::list_names`] —— 后者是发送链路用的同一份「哪些字段
//! 会被替换」的判据。

use simple_post_core::db::DbPool;
use simple_post_core::domain::{EndpointDetail, Environment, Project};
use simple_post_core::error::AppError;

/// 查出一个端点当前还缺的东西。空列表表示它能发出去了。
///
/// 查不出来的东西一律不报：读库失败时返回一个空列表，而不是把「查不到」说成
/// 「不缺」—— 前者让 agent 多问一句，后者让 agent 以为一切正常。
///
/// `pub(crate)`：MCP 工具、`crate::spec::ops`（REST 路由与桌面 command 那一侧）
/// 都用它，故它得走出 `project_ops`。
pub(crate) fn missing_pieces(
    pool: &DbPool,
    node_id: &str,
    detail: &EndpointDetail,
    project: &Project,
) -> Vec<String> {
    let mut out = Vec::new();

    // 环境：端点行不再绑定环境，故这里按项目的默认环境（最早创建的那个）算缺口
    // —— 与发送链路同一套判据。
    let env = crate::mcp::resolve_env(pool, None, &project.id).unwrap_or(None);

    // 模块：base URL 住在 (环境 × 模块) 的单元格里，所以要说清是哪个模块。
    let module_id = simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::nodes::module_id_of(c, node_id)
    })
    .unwrap_or(None);

    // ---- base URL ----
    //
    // 端点自己钉了 host（`base_url_override` 是 `Some`）时不需要格子。`Some("")` 是
    // 「明确不要前缀」，也是一个有意选定的答案，故同样不报。
    let (base_url, proxy) = if detail.base_url_override.is_some() {
        (String::new(), None)
    } else {
        let resolved =
            crate::variable_fill::resolve_base_url(detail, env.as_ref(), module_id.as_deref());
        if resolved.source == crate::variable_fill::BaseUrlSource::None {
            out.push(base_url_gap(pool, env.as_ref(), module_id.as_deref()));
        }
        (resolved.base_url, resolved.proxy)
    };

    // ---- 变量 ----
    let parts = simple_post_core::dynamic::TemplateParts {
        base_url: &base_url,
        // 代理地址会被 `{{var}}` 替换，故它里面的标记同样参与扫描：只出现在代理地址里
        // 的动态变量缺了，这个端点同样发不出去。
        proxy: proxy.as_deref(),
        url: &detail.url,
        params: &detail.params,
        // 路径参数值里的 `{{var}}` 照样会被替换，故它们同样参与扫描。
        path_params: &detail.path_params,
        headers: &detail.headers,
        body: &detail.body,
        auth: &detail.auth,
    };
    let names = simple_post_core::dynamic::list_names(&parts);
    if !names.is_empty() {
        // 动态变量挂在项目里，故只有**本项目**的同名定义才算可用：别的项目里的那条
        // 在这个端点上不会被替换。
        let dynamic_names = enabled_dynamic_names(pool, &project.id);
        for name in names {
            if variable_available(env.as_ref(), &dynamic_names, &name) {
                continue;
            }
            out.push(match env.as_ref() {
                Some(e) => format!("environment '{}' has no variable '{{{{{name}}}}}'", e.name),
                None => format!(
                    "no environment is available, so '{{{{{name}}}}}' would be sent literally",
                ),
            });
        }
    }

    out
}

/// base URL 那一格为什么空着。两种原因给两种说法，agent 才知道该做什么。
fn base_url_gap(pool: &DbPool, env: Option<&Environment>, module_id: Option<&str>) -> String {
    let Some(env) = env else {
        return "no environment: the endpoint has no environment bound and the project has no active one, so there is nowhere to read a base URL from".to_string();
    };
    let Some(module_id) = module_id else {
        return "the endpoint is not under any module, and a base URL lives in the (environment x module) cell".to_string();
    };
    let module_name = simple_post_core::db::with_conn(pool, |c| {
        Ok::<String, AppError>(
            simple_post_core::db::nodes::get(c, module_id)
                .map(|n| n.name)
                .unwrap_or_else(|_| module_id.to_string()),
        )
    })
    .unwrap_or_else(|_| module_id.to_string());
    format!(
        "environment '{}' has no base URL for module '{}' — that cell never inherits",
        env.name, module_name
    )
}

/// 这个名字能不能被替换掉。
///
/// 环境变量被停用时不参与替换，故它不算可用；动态变量的定义挂在项目里，被停用的
/// 定义同样不参与替换。
fn variable_available(env: Option<&Environment>, dynamic_names: &[String], name: &str) -> bool {
    if dynamic_names.iter().any(|d| d == name) {
        return true;
    }
    match env {
        Some(e) => {
            !e.disabled_variables.iter().any(|d| d == name) && e.variables.contains_key(name)
        }
        None => false,
    }
}

/// 某个项目里全部**启用中**的动态变量名。停用的定义不参与替换，故不算可用。
fn enabled_dynamic_names(pool: &DbPool, project_id: &str) -> Vec<String> {
    simple_post_core::db::with_conn(pool, |c| {
        Ok(
            simple_post_core::db::dynamic_variables::list_by_project(c, project_id)?
                .into_iter()
                .filter(|d| d.enabled)
                .map(|d| d.name)
                .collect(),
        )
    })
    .unwrap_or_default()
}

/// 这个 `missing_pieces` 需要 `Project`，才能知道端点没点名环境时该用哪个默认环境。
pub(crate) fn project_of(pool: &DbPool, project_id: &str) -> Result<Project, AppError> {
    simple_post_core::db::with_conn(pool, |c| simple_post_core::db::projects::get(c, project_id))
}
