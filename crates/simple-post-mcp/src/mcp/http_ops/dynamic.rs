//! 动态变量 —— 这次请求引用到的那些定义，取数后并进变量池。

use crate::mcp::AppState;
use simple_post_core::domain::{EndpointDetail, Environment, Project};
use simple_post_core::error::AppError;

/// 解析这个请求引用到的动态变量，返回它们的值（变量名 → 替换用的文本）。
///
/// 只挑「请求里出现过」的名字——未被引用的定义不参与这次发送，也不产生错误。
///
/// **取值只认消费方所在的项目。** 请求里的 `{{name}}` 在本项目里找不到定义，就是缺失的：
/// 不退到别的项目去拿同名的那条。故这里按 `project.id` 查定义。
///
/// **发送路径也要过数据源项目的那道门。** 一条定义可能指向别的项目的接口，而取数用的是
/// 那个项目的 base URL 与鉴权——那个项目的权限决定这次取数能不能做。少了这道门，一个
/// 只读项目的客户端就能借本项目的接口把别的项目的凭据用出去。
/// 内联数据源没有项目可落（它自带完整地址与凭据），故只由全局能力档位裁决。
///
/// 取数失败**不让整次调用失败**：该变量按缺失处理（`{{name}}` 原样留在请求里），错误
/// 由动态变量面板与 `dynamic_var_resolve` 呈现。这与脚本失败不影响请求成败是同一条取舍。
/// 但**权限不足不是取数失败**：它是一次授权决定，故直接让这次调用失败。
pub(super) async fn resolve_dynamic_values(
    state: &AppState,
    env: Option<&Environment>,
    project: &Project,
    detail: &EndpointDetail,
    overrides: &crate::variable_fill::EndpointOverrides,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let empty = std::collections::HashMap::new();
    // `resolve_base_url` 给出的是**未替换**的模板：base URL 也扫（它可能自身
    // 带变量），代理地址同样要扫（发送路径对代理地址单独做一次模板替换）。
    let resolved =
        crate::variable_fill::resolve_base_url(detail, env, overrides.module_id.as_deref());
    let base_url = resolved.base_url;
    let proxy = resolved.proxy;
    // 扫描用的是**生效后**的模板：调用方的覆盖值会替换端点里的那一份，而替换发生在
    // 变量池合并之后，故扫描必须看生效的那一份——否则覆盖值里写的 `{{name}}` 不会
    // 触发取数，原样发出去。
    let eff_url = overrides.url.clone().unwrap_or_else(|| detail.url.clone());
    let eff_params = overrides
        .params
        .clone()
        .unwrap_or_else(|| detail.params.clone());
    // 路径参数的值同样会被 `{{var}}` 替换，故它们里的标记同样要参与扫描。
    let eff_path_params = overrides
        .path_params
        .clone()
        .unwrap_or_else(|| detail.path_params.clone());
    let eff_headers = overrides
        .headers
        .clone()
        .unwrap_or_else(|| detail.headers.clone());
    let eff_body = overrides
        .body
        .clone()
        .unwrap_or_else(|| detail.body.clone());
    let parts = simple_post_core::dynamic::TemplateParts {
        base_url: &base_url,
        proxy: proxy.as_deref(),
        url: &eff_url,
        params: &eff_params,
        path_params: &eff_path_params,
        headers: &eff_headers,
        body: &eff_body,
        auth: &detail.auth,
    };
    let names = simple_post_core::dynamic::list_names(&parts);
    if names.is_empty() {
        return Ok(empty);
    }
    // 逐个名字找到它指向的定义，再按那个定义的数据源项目过门：定义挂在消费方项目里，
    // 但数据源可能指向别的项目，那道门要一个个查。
    for name in &names {
        let definition = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::dynamic_variables::get_by_name_in_project(c, &project.id, name)
        })?;
        let Some(definition) = definition else {
            continue;
        };
        let source_project = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::dynamic_variables::source_project_id_of(c, &definition.id)
        })?;
        let Some(source_project) = source_project else {
            // 内联数据源：地址与凭据都是调用方自己给的，没有别的项目的凭据可读。
            continue;
        };
        crate::mcp::check_permission(&state.pool, &source_project, "dynamic_var_resolve").await?;
    }
    let ctx = simple_post_core::dynamic::ResolveContext {
        pool: state.pool.clone(),
    };
    let resolution = simple_post_core::dynamic::resolve_names(
        &ctx,
        Some(&project.id),
        &names,
        false,
        &crate::dynamic::fetcher(),
    )
    .await?;
    Ok(resolution.values)
}
