//! 动态变量的取数器：把一条定义的数据源变成一次真实的 HTTP 响应。
//!
//! 实现住在 mcp crate 而不是 core，因为「引用端点」形态要用
//! `variable_fill::resolve_endpoint` 组装请求，而那个函数住在 mcp crate（core 不能依赖
//! mcp）。core 那边只留 [`simple_post_core::dynamic::SourceFetcher`] 这个抽象。
//!
//! 两种数据源形态：
//!
//! - **引用端点**：复用那个端点自己的 base URL、鉴权与前置脚本。程序按「发送这个端点」的
//!   同一套规则处理它，故它可以用环境变量，也可以带自己的脚本。
//! - **内联请求**：一份 method / url / headers / body / auth。URL 必须写完整——内联请求没有
//!   base URL 概念。
//!
//! 数据源端点**不能**引用动态变量（只用环境变量），故结构上不可能出现环。

use std::future::Future;
use std::pin::Pin;

use serde::Deserialize;

use simple_post_core::db::DbPool;
use simple_post_core::domain::{
    parse_http_method, AuthConfig, BodyMode, DynamicError, DynamicErrorKind, DynamicVariable,
    EndpointDetail, KeyValue, RequestBody,
};
use simple_post_core::dynamic::{RawSource, ResolveContext, SourceFetcher};

use crate::variable_fill::EndpointOverrides;

/// 取数器。两种形态都走它，故「取数」这件事只有一份实现。
pub struct EndpointSourceFetcher;

/// 内联请求的形状。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineSource {
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    params: Vec<KeyValue>,
    #[serde(default)]
    headers: Vec<KeyValue>,
    #[serde(default)]
    body: RequestBody,
    #[serde(default)]
    auth: AuthConfig,
}

fn default_method() -> String {
    "GET".to_string()
}

impl SourceFetcher for EndpointSourceFetcher {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        ctx: &'a ResolveContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(endpoint_id) = def.source_endpoint_id.as_deref() {
                return fetch_from_endpoint(def, endpoint_id, ctx).await;
            }
            if let Some(inline) = def.source_inline_json.as_deref() {
                return fetch_from_inline(def, inline, ctx).await;
            }
            Err(DynamicError {
                name: def.name.clone(),
                kind: DynamicErrorKind::SourceError,
                message: "这条定义没有数据源：既没有引用端点，也没有内联请求".into(),
            })
        })
    }
}

/// 引用端点形态。
async fn fetch_from_endpoint(
    def: &DynamicVariable,
    endpoint_id: &str,
    ctx: &ResolveContext,
) -> Result<RawSource, DynamicError> {
    let detail: EndpointDetail = simple_post_core::db::with_conn(&ctx.pool, |c| {
        simple_post_core::db::endpoint_details::get(c, endpoint_id)
    })
    .map_err(|_| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceMissing,
        message: format!("数据源端点 {endpoint_id} 已被删除"),
    })?;

    // 模块 id：base URL 的取值住在 (环境 × 服务 × 模块) 的格子里，故要知道它落在哪个模块。
    let module_id = simple_post_core::db::with_conn(&ctx.pool, |c| {
        simple_post_core::db::nodes::module_id_of(c, endpoint_id)
    })
    .ok()
    .flatten();

    let overrides = EndpointOverrides {
        module_id,
        ..Default::default()
    };

    let resolved = crate::variable_fill::resolve_endpoint(
        &detail,
        ctx.env.as_ref(),
        &ctx.project,
        Some(&overrides),
    )
    .map_err(|e| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message: format!("解析数据源端点失败：{e}"),
    })?;

    let mut headers = resolved.headers;
    let mut url = resolved.url;
    let mut body = resolved.body;

    // OAuth2 的 token 是发请求之前才取得到的，故注入放在这里而不是解析器里。
    if let simple_post_core::domain::AuthConfig {
        kind: simple_post_core::domain::AuthType::Oauth2,
        oauth2: Some(cfg),
        ..
    } = &resolved.auth
    {
        let token = simple_post_core::oauth2::ensure_token(&ctx.pool, cfg)
            .await
            .map_err(|e| DynamicError {
                name: def.name.clone(),
                kind: DynamicErrorKind::SourceError,
                message: format!("取 OAuth2 token 失败：{e}"),
            })?;
        simple_post_core::oauth2::inject(cfg, &token, &mut headers, &mut url);
    }

    // multipart 的 body 在这里才成形：boundary 必须现生成。
    if resolved.body_mode == BodyMode::Multipart {
        body = body
            .take()
            .map(|content| crate::mcp::http_ops::multipart::apply(&mut headers, &content));
    }

    send(
        def,
        ctx,
        Some(endpoint_id.to_string()),
        resolved.method,
        url,
        headers,
        body,
    )
    .await
}

/// 内联请求形态。
async fn fetch_from_inline(
    def: &DynamicVariable,
    inline: &str,
    ctx: &ResolveContext,
) -> Result<RawSource, DynamicError> {
    let spec: InlineSource = serde_json::from_str(inline).map_err(|e| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message: format!("内联请求不是合法的 JSON：{e}"),
    })?;

    let method = parse_http_method(&spec.method).map_err(|_| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message: format!("内联请求的 method '{}' 不是合法的 HTTP 动词", spec.method),
    })?;

    // 内联请求走**同一套**组装规则：把它拼成一份内存里的端点行，于是变量替换、header
    // 排序、鉴权派生、body 判空全都复用 `resolve_endpoint`，不另写一遍。
    let detail = EndpointDetail {
        node_id: String::new(),
        method,
        url: spec.url,
        environment_id: None,
        base_url_definition_id: None,
        // 空串 = 自定义模式但不加前缀：URL 字段本身就是完整地址。
        base_url_override: Some(String::new()),
        params: spec.params,
        headers: spec.headers,
        body: spec.body,
        auth: spec.auth,
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
    };

    let overrides = EndpointOverrides::default();
    let resolved = crate::variable_fill::resolve_endpoint(
        &detail,
        ctx.env.as_ref(),
        &ctx.project,
        Some(&overrides),
    )
    .map_err(|e| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message: format!("解析内联请求失败：{e}"),
    })?;

    let mut headers = resolved.headers;
    let url = resolved.url;
    let mut body = resolved.body;

    if resolved.body_mode == BodyMode::Multipart {
        body = body
            .take()
            .map(|content| crate::mcp::http_ops::multipart::apply(&mut headers, &content));
    }

    send(def, ctx, None, resolved.method, url, headers, body).await
}

/// 把组装好的请求发出去，并把传输层的结果折成 [`RawSource`]。
///
/// 脚本链在 `endpoint_id` 存在时装配，故数据源端点自己的前置脚本与集合链都跑。
async fn send(
    def: &DynamicVariable,
    ctx: &ResolveContext,
    endpoint_id: Option<String>,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
) -> Result<RawSource, DynamicError> {
    let settings = crate::mcp::http_ops::resolve_settings(&ctx.pool, (None, None, None), None);
    // 脚本仍能读到环境变量，但写不进去：取数是一次「读」，不该顺手改写项目的环境。
    // 快照里没有环境 id，`apply_env_writes` 便没有落点，脚本的写入只活在这一趟里
    // （前置脚本写的值，测试脚本仍读得到）。
    let env = simple_post_core::script::EnvSnapshot {
        id: None,
        ..crate::mcp::http_ops::env_snapshot(ctx.env.as_ref())
    };

    let outcome = simple_post_core::script::send_with_scripts(
        &ctx.pool,
        simple_post_core::script::ScriptedSendInput {
            endpoint_id,
            request: simple_post_core::script::ScriptRequest {
                method,
                url,
                headers,
                body,
            },
            env,
            settings,
            script_override: None,
        },
    )
    .await
    .map_err(|e| DynamicError {
        name: def.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message: format!("发送数据源请求失败：{e}"),
    })?;

    match outcome.http.response {
        Some(response) => Ok(RawSource {
            status: response.status,
            content_type: response.content_type,
            body: response.body,
        }),
        None => {
            let error = outcome.http.error;
            let kind = match error.as_ref().map(|e| e.kind.as_str()) {
                Some("timeout") => DynamicErrorKind::Timeout,
                _ => DynamicErrorKind::Network,
            };
            Err(DynamicError {
                name: def.name.clone(),
                kind,
                message: error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "请求没有抵达服务端".into()),
            })
        }
    }
}

/// 取数器需要的连接池类型别名，便于调用方少写一层路径。
pub type Pool = DbPool;
