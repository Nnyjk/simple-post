//! 动态变量的公共操作 —— MCP 工具与两侧 transport 共用。
//!
//! 放在 mcp crate 而不是 server crate：MCP 工具在本 crate 内，它不能依赖上层；而 REST 与
//! Tauri 两侧都能依赖本 crate。故公共操作住在这一层，server crate 的 `dynamic_api` 只是
//! 它的一层薄壳。
//!
//! 定义是**全局**的：不属于任何项目，任何项目的端点都能引用它。取数用的环境与服务由
//! 定义自己决定（点名的那个，或数据源端点所属项目的首个环境与默认服务），故同一个
//! `{{name}}` 在任何项目里取到的值都一样。

use serde::Serialize;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    now_ms, uid, DynamicCacheEntry, DynamicError, DynamicResolution, DynamicValueSource,
    DynamicValueStatus, DynamicVariable, DynamicVariablePatch, Environment, ValueMode,
};
use simple_post_core::dynamic::{parse_raw, serialize_value, ResolveContext, SourceContext};
use simple_post_core::error::AppError;

/// 新建一条定义时要填的字段。缺席的字段取默认值。
#[derive(Debug, Clone, Default)]
pub struct CreateOpts {
    /// 渲染层自己铸的 id。`None` = 由服务端铸。
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub source_endpoint_id: Option<String>,
    pub source_inline_json: Option<String>,
    /// 取数用的环境。`None` = 数据源端点所属项目的第一个可用环境。
    pub source_environment_id: Option<String>,
    /// 取数用的服务。`None` = 数据源接口自己绑的服务，它没绑就用项目的默认服务。
    pub source_base_url_definition_id: Option<String>,
    pub extract_path: Option<String>,
    pub value_mode: Option<ValueMode>,
    pub ttl_seconds: Option<i64>,
    pub stale_fallback: Option<bool>,
    pub enabled: Option<bool>,
    pub sort_order: Option<i64>,
}

/// 哪些端点引用了这个变量名。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceDto {
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub project_id: String,
    pub project_name: String,
}

/// 一条定义 + 它的当前值与缓存状态。
///
/// 列表与详情都用它：面板要显示「当前值」与「缓存状态」，故读的时候一并算出来。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariableView {
    #[serde(flatten)]
    pub definition: DynamicVariable,
    /// 当前生效的值（已按 `value_mode` 序列化）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// 这个值从哪来。
    pub source: DynamicValueSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub is_override: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DynamicError>,
    /// 数据源端点所属的项目。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_project_id: Option<String>,
    /// 数据源端点所属项目的名字。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_project_name: Option<String>,
    /// 实际取数用的环境。定义点名的那个，或数据源项目里的第一个可用环境。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_environment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_environment_name: Option<String>,
    /// 实际取数用的服务：定义点名的那个，或数据源接口自己绑的，或数据源项目的默认服务。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_base_url_definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_base_url_definition_name: Option<String>,
    /// 哪些端点的请求里写了这个变量名。跨全部项目——定义是全局的，引用它的端点可能
    /// 散在基线项目与各个分发项目里。
    pub referenced_by: Vec<ReferenceDto>,
}

/// 列出全部定义，带当前值与缓存状态。
///
/// **不取数**：读的是缓存行。取数可能发出站请求，而这是一次纯读。
///
/// `include_references` 决定要不要带上「哪些端点引用了它」。那一列是**别的项目的
/// 接口名与 id**，故 MCP 那条路（无按项目的门）不带走它——否则一个只读档位的客户端
/// 就能借这条工具枚举所有项目的接口名，而 `endpoint_list` / `endpoint_search` 恰恰
/// 拒绝无项目地扫全部项目。桌面端与 REST 那条路是用户自己的工作区，带上它。
pub fn list(pool: &DbPool, include_references: bool) -> Result<Vec<DynamicVariableView>, AppError> {
    let definitions = db::with_conn(pool, db::dynamic_variables::list)?;
    let now = now_ms();
    let mut out = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let source = simple_post_core::dynamic::source_context(pool, &definition)?;
        let env_key = source.environment_id.clone().unwrap_or_default();
        let entry = db::with_conn(pool, |c| {
            db::dynamic_variable_cache::get(c, &definition.id, &env_key)
        })?;
        let referenced_by = if include_references {
            db::with_conn(pool, |c| {
                db::dynamic_variables::referencing_endpoints(c, &definition.name)
            })?
            .into_iter()
            .map(|r| ReferenceDto {
                endpoint_id: r.endpoint_id,
                endpoint_name: r.endpoint_name,
                project_id: r.project_id,
                project_name: r.project_name,
            })
            .collect()
        } else {
            Vec::new()
        };
        out.push(view_of(
            definition,
            &source,
            entry.as_ref(),
            now,
            referenced_by,
        ));
    }
    Ok(out)
}

/// 读一条定义，带当前值与缓存状态。
pub fn get(pool: &DbPool, definition_id: &str) -> Result<DynamicVariableView, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let source = simple_post_core::dynamic::source_context(pool, &definition)?;
    let env_key = source.environment_id.clone().unwrap_or_default();
    let entry = db::with_conn(pool, |c| {
        db::dynamic_variable_cache::get(c, definition_id, &env_key)
    })?;
    Ok(view_of(
        definition,
        &source,
        entry.as_ref(),
        now_ms(),
        Vec::new(),
    ))
}

/// 新建一条定义。
pub fn create(pool: &DbPool, opts: CreateOpts) -> Result<DynamicVariable, AppError> {
    let now = now_ms();
    let definition = DynamicVariable {
        id: opts.id.unwrap_or_else(uid),
        name: opts.name,
        description: opts.description,
        source_endpoint_id: opts.source_endpoint_id,
        source_endpoint_name: None,
        source_inline_json: opts.source_inline_json,
        source_environment_id: opts.source_environment_id,
        source_base_url_definition_id: opts.source_base_url_definition_id,
        extract_path: opts.extract_path.unwrap_or_else(|| "$".to_string()),
        value_mode: opts.value_mode.unwrap_or_default(),
        ttl_seconds: opts.ttl_seconds.unwrap_or(60),
        stale_fallback: opts.stale_fallback.unwrap_or(false),
        enabled: opts.enabled.unwrap_or(true),
        sort_order: opts.sort_order.unwrap_or(0),
        created_at: now,
        updated_at: now,
    };
    db::with_tx(pool, |c| db::dynamic_variables::create(c, &definition))?;
    db::with_conn(pool, |c| db::dynamic_variables::get(c, &definition.id))
}

/// 改一条定义。
pub fn update(
    pool: &DbPool,
    definition_id: &str,
    patch: DynamicVariablePatch,
) -> Result<DynamicVariable, AppError> {
    db::with_tx(pool, |c| {
        db::dynamic_variables::update(c, definition_id, patch.clone())
    })
}

/// 删一条定义。缓存行由外键带走。
pub fn delete(pool: &DbPool, definition_id: &str) -> Result<(), AppError> {
    db::with_tx(pool, |c| db::dynamic_variables::delete(c, definition_id))
}

/// 解析一批名字，返回它们的值、状态与错误。
///
/// 名字是全局的：同一个名字在任何项目里都指向同一条定义。
pub async fn resolve(
    pool: &DbPool,
    names: &[String],
    force: bool,
) -> Result<DynamicResolution, AppError> {
    let ctx = ResolveContext { pool: pool.clone() };
    simple_post_core::dynamic::resolve_names(&ctx, names, force, &crate::dynamic::fetcher()).await
}

/// 强制重新取一条定义。
pub async fn refresh(pool: &DbPool, definition_id: &str) -> Result<DynamicValueStatus, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let names = vec![definition.name.clone()];
    let resolution = resolve(pool, &names, true).await?;
    Ok(resolution
        .statuses
        .into_iter()
        .next()
        .unwrap_or_else(|| DynamicValueStatus {
            name: definition.name.clone(),
            value: None,
            source: DynamicValueSource::Missing,
            fetched_at: None,
            expires_at: None,
            is_override: false,
            error: None,
        }))
}

/// 写覆盖值。
///
/// `value` 的两态：`None` = 清除覆盖值（下次解析重新取数），`Some(text)` = 写入。
/// 覆盖值按（定义 × 取数环境）存放，故换环境改了值不影响另一个环境。
///
/// 过期时间有三种可能，故线上多带一个 `forever` 开关——一个可空的时间戳表达不了
/// 「沿用定义的 TTL」与「一直有效」这两者：
///
/// - `forever = true` → 一直有效，直到手动清除；
/// - `expires_at = Some(ms)` → 到期时间就是它；
/// - 两者都没有 → 沿用定义的 `ttl_seconds`。
pub fn set_override(
    pool: &DbPool,
    definition_id: &str,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: bool,
) -> Result<(), AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    // 缓存键用定义自己的取数环境，与解析时用的那一个同源：否则覆盖值会落到另一个键上，
    // 手动指定的值看不见自己。
    let source = simple_post_core::dynamic::source_context(pool, &definition)?;
    let env_key = source.environment_id.clone().unwrap_or_default();
    let Some(text) = value else {
        // 清除覆盖值：整行删掉，下一次解析重新取数。
        db::with_tx(pool, |c| {
            db::dynamic_variable_cache::delete(c, definition_id, &env_key)
        })?;
        return Ok(());
    };
    let now = now_ms();
    let deadline = if forever {
        None
    } else {
        Some(expires_at.unwrap_or(now + definition.ttl_seconds * 1000))
    };
    let entry = DynamicCacheEntry {
        definition_id: definition_id.to_string(),
        environment_id: env_key.clone(),
        value_json: Some(text),
        fetched_at: now,
        expires_at: deadline.unwrap_or(now + definition.ttl_seconds * 1000),
        is_override: true,
        override_expires_at: deadline,
    };
    db::with_tx(pool, |c| db::dynamic_variable_cache::upsert(c, &entry))?;
    Ok(())
}

/// 一条定义 + 它的缓存行 → 视图。
fn view_of(
    definition: DynamicVariable,
    source: &SourceContext,
    entry: Option<&DynamicCacheEntry>,
    now: i64,
    referenced_by: Vec<ReferenceDto>,
) -> DynamicVariableView {
    let (value, source_kind, fetched_at, expires_at, is_override) = match entry {
        None => (None, DynamicValueSource::Missing, None, None, false),
        Some(e) if e.is_override => {
            let alive = e.override_expires_at.map(|t| t > now).unwrap_or(true);
            if alive {
                (
                    e.value_json
                        .as_ref()
                        .map(|raw| serialize_value(&parse_raw(raw), definition.value_mode)),
                    DynamicValueSource::Override,
                    Some(e.fetched_at),
                    e.override_expires_at,
                    true,
                )
            } else {
                // 覆盖值过期了：值不再生效。
                (
                    None,
                    DynamicValueSource::Missing,
                    Some(e.fetched_at),
                    None,
                    true,
                )
            }
        }
        Some(e) => {
            let fresh = e.expires_at > now;
            (
                e.value_json
                    .as_ref()
                    .map(|raw| serialize_value(&parse_raw(raw), definition.value_mode)),
                if fresh {
                    DynamicValueSource::Auto
                } else {
                    DynamicValueSource::Stale
                },
                Some(e.fetched_at),
                Some(e.expires_at),
                false,
            )
        }
    };
    let environment: Option<&Environment> = source.env.as_ref();
    let service_name = source.base_url_definition_id.as_ref().and_then(|id| {
        source
            .project
            .base_url_definitions
            .iter()
            .find(|d| &d.id == id)
            .map(|d| d.name.clone())
    });
    DynamicVariableView {
        definition,
        value,
        source: source_kind,
        fetched_at,
        expires_at,
        is_override,
        error: None,
        source_project_id: source.project_id.clone(),
        source_project_name: if source.project_id.is_some() {
            Some(source.project.name.clone())
        } else {
            None
        },
        effective_environment_id: source.environment_id.clone(),
        effective_environment_name: environment.map(|e| e.name.clone()),
        effective_base_url_definition_id: source.base_url_definition_id.clone(),
        effective_base_url_definition_name: service_name,
        referenced_by,
    }
}
