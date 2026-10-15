//! `resolve_endpoint` —— 主 API，把端点 + 环境 + 项目 + 覆盖值合成线上形状。
//!
//! 对应渲染进程的 `buildFetchInit`；顺序（先拼 baseUrl 再替换变量、最后收斜杠）
//! 是承重的，改动顺序会改出别的 URL。

use std::collections::HashMap;

use simple_post_core::domain::{EndpointDetail, Environment, Project};
use simple_post_core::error::AppError;

use super::auth::resolve_auth;
use super::base_url::{collapse_url_slashes, join_base_url, resolve_base_url_with};
use super::request::{append_apikey_query, append_query_string, build_headers};
use super::template::resolve_template;
use super::types::{EndpointOverrides, ResolvedEndpoint};
// -------------------------------------------------------------------------
// resolve_endpoint —— 主 API
// -------------------------------------------------------------------------

/// 把端点 + 环境 + 项目 + 覆盖值合成为完全解析好的线上形状。
///
/// 解析顺序（对应 `buildFetchInit`）：
///   1. 套用覆盖值（url / method / headers / params / body）
///   2. 经 [`resolve_base_url_with`] 解析 baseUrl
///   3. 到处替换 `{{var}}`（URL 路径、headers、params、body 内容、鉴权字段），然后
///      收掉路径里的重复斜杠（见 [`collapse_url_slashes`] —— 只有替换才会把拼接插入的
///      分隔符变成 `//`）
///   4. 追加启用的 query 参数（编码后）
///   5. 追加 apikey query（`auth.kind == Apikey && inLocation == Query` 时）
///   6. 构建有序请求头列表（用户头 + 自动 Content-Type + 自动 Accept + 鉴权推出的
///      `Authorization`）
///   7. 决定是否带上 body（仅当 `body.mode != None && content` 非空）
///
/// `missing_vars` 汇总所有没解析出来的 `{{x}}`，失败还是告警由调用方决定。
///
/// 模块走 `overrides.module_id` 而不是单独一个参数：它属于调用方上下文，与调用方自己
/// 选的 base URL 同类；放在覆盖值里，传入 `None` 的现有调用方都不用改，解析结果是
/// 「没有模块，也就没有格子可读」—— 这正是一个没有树上下文的端点该得的答案。
///
/// `AppError::Validation` 只用于明显畸形的输入。当前发送路径没有别的校验规则，真正
/// 用不了的时候由 HTTP 发送器自己报错。
pub fn resolve_endpoint(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
    overrides: Option<&EndpointOverrides>,
) -> Result<ResolvedEndpoint, AppError> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record_missing =
        |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
            if seen.insert(var.clone(), ()).is_none() {
                missing.push(var);
            }
        };

    // ---- 套用覆盖值（相关字段的浅拷贝）---------
    let mut eff_url = detail.url.clone();
    let mut eff_method = detail.method.clone();
    let mut eff_headers = detail.headers.clone();
    let mut eff_params = detail.params.clone();
    let mut eff_body = detail.body.clone();
    let eff_auth = detail.auth.clone();
    // 调用方的选择压过端点自带的定义 —— 渲染进程未保存的 base URL 选择正是靠这条生效。
    let eff_def_id = overrides.and_then(|o| o.base_url_definition_id.clone());
    if let Some(o) = overrides {
        if let Some(u) = o.url.clone() {
            eff_url = u;
        }
        if let Some(m) = o.method.clone() {
            eff_method = m;
        }
        if let Some(h) = o.headers.clone() {
            eff_headers = h;
        }
        if let Some(p) = o.params.clone() {
            eff_params = p;
        }
        if let Some(b) = o.body.clone() {
            eff_body = b;
        }
    }

    // ---- 变量池 ------------------------------------------------
    let vars: HashMap<String, String> = env.map(|e| e.variables.clone()).unwrap_or_default();

    // ---- params：先替换变量，再拼 URL。
    // 分两步做，missing 追踪与回显用的 params_out 列表才共用同一份真相。
    let mut params_subst: Vec<(String, String)> = Vec::with_capacity(eff_params.len());
    for p in &eff_params {
        if !p.enabled || p.key.is_empty() || p.value.is_empty() {
            continue;
        }
        let r = resolve_template(&p.value, &vars);
        for m in r.missing {
            record_missing(&mut missing, &mut seen, m);
        }
        params_subst.push((p.key.clone(), r.resolved));
    }

    // ---- baseUrl 解析 + URL 替换 ------------------------
    let (base_url, _src, _def_id) = resolve_base_url_with(
        detail,
        env,
        project,
        eff_def_id.as_deref(),
        overrides.and_then(|o| o.module_id.as_deref()),
    );
    let joined = join_base_url(&base_url, &eff_url);
    let url_r = resolve_template(&joined, &vars);
    for m in url_r.missing {
        record_missing(&mut missing, &mut seen, m);
    }
    // 替换之后、绝不之前：`{{var}}` → `/api` 才会把 `join_base_url` 插的分隔符变成
    // `//`，到这里真实路径才第一次已知。`collapse_url_slashes` 不碰 query/fragment
    // 数据，所以 params 仍能在它之后再追加。
    let url_norm = collapse_url_slashes(&url_r.resolved);

    // ---- 追加 query string（用已替换的 params）-------
    let url_with_params = append_query_string(url_norm, &params_subst);

    // ---- 鉴权解析 ---------------------------------------------
    let (resolved_auth, auth_miss) = resolve_auth(&eff_auth, &vars);
    for m in auth_miss {
        record_missing(&mut missing, &mut seen, m);
    }

    // ---- query 形式的 apikey ---------------------------------------------
    let final_url = append_apikey_query(url_with_params, &resolved_auth);

    // ---- body 是否带上 + 替换 --------------------------------
    let body = match &eff_body.mode {
        simple_post_core::domain::BodyMode::None => None,
        _ if eff_body.content.is_empty() => None,
        _ => {
            let b = resolve_template(&eff_body.content, &vars);
            for m in b.missing {
                record_missing(&mut missing, &mut seen, m);
            }
            Some(b.resolved)
        }
    };

    // ---- 请求头排序 ---------------------------------------------
    let headers = build_headers(
        &eff_headers,
        &eff_body.mode,
        &resolved_auth,
        &vars,
        &mut missing,
    );

    Ok(ResolvedEndpoint {
        url: final_url,
        method: eff_method.as_str().to_string(),
        headers,
        params: params_subst,
        body,
        auth: resolved_auth,
        missing_vars: missing,
    })
}
