//! 落一份接口规格，并算出端点还缺什么。
//!
//! 「还缺什么」这一步与落库放在一起，因为它是**同一份规格**的另一半答案：
//! 规格落进库里说明「接口建好了」，而缺的那几样（base URL 那一格、环境变量）说明
//! 「它还发不出去」。分开放，调用方就要再发一次请求、看报错、再回头补一轮。

use serde::Serialize;

use simple_post_core::db::DbPool;
use simple_post_core::error::AppError;
use simple_post_core::spec::upsert::{upsert_endpoint_spec, EndpointSpec, SpecOutcome};

/// 一次落库的结果：落了什么 + 端点还缺什么。
///
/// `outcome` 被摊平，故线上的键就是它自己的键（`projectId` / `module` / `endpoint` …），
/// `missing` 与它们平级。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecUpsertResult {
    /// 落库的结果：模块、集合路径、环境、服务与端点各自的 id。
    #[serde(flatten)]
    pub outcome: SpecOutcome,
    /// 这个端点还缺什么。空列表表示它能发出去了。
    pub missing: Vec<String>,
}

/// 落一份规格。
///
/// 整个落库过程在**一个事务**里：规格写到一半失败时，库里不留半截状态。
///
/// ## 权限门在这一层的外面
///
/// 本函数不查权限档位。MCP 工具那一条路在调它**之前**过门（逐项目 `permissions`
/// 表管的是「AI 客户端在这个项目里能做什么」）；REST 路由与桌面 command 那两条路
/// 是用户自己的会话，走的是与 `nodes_create` 同一套规矩 —— 那一族写路由都不查
/// 逐项目档位，故这里也不查。
///
/// 「还缺什么」在落库之后算，且**算不出来不算缺**：查库失败时返回空列表，
/// 而不是把「查不到」说成「不缺」—— 前者让调用方多问一句，后者让它以为一切正常。
pub fn upsert(pool: &DbPool, spec: &EndpointSpec) -> Result<SpecUpsertResult, AppError> {
    let outcome = simple_post_core::db::with_tx(pool, |c| upsert_endpoint_spec(c, spec))?;
    let missing = missing_pieces(pool, &outcome);
    Ok(SpecUpsertResult { outcome, missing })
}

/// 这个端点还缺什么。空列表表示它能发出去了。
fn missing_pieces(pool: &DbPool, outcome: &SpecOutcome) -> Vec<String> {
    let endpoint_id = &outcome.endpoint.resource.node.id;
    // 取项目的那一步与写工具共用同一个判据（`project_ops::endpoint::project_of`），
    // 故「项目 ★」与「激活环境」在两条路上是同一个答案。
    let Ok(project) = crate::mcp::project_ops::endpoint::project_of(pool, &outcome.project_id)
    else {
        return Vec::new();
    };
    crate::mcp::project_ops::endpoint::missing_pieces(
        pool,
        endpoint_id,
        &outcome.endpoint.resource.detail,
        &project,
    )
}
