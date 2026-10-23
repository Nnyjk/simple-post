//! 动态变量的公共操作 —— MCP 工具与两侧 transport 共用。
//!
//! 放在 mcp crate 而不是 server crate：MCP 工具在本 crate 内，它不能依赖上层；而 REST 与
//! Tauri 两侧都能依赖本 crate。故公共操作住在这一层，server crate 的 `dynamic_api` 只是
//! 它的一层薄壳。

use serde::Serialize;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    now_ms, uid, DynamicCacheEntry, DynamicError, DynamicResolution, DynamicValueSource,
    DynamicValueStatus, DynamicVariable, DynamicVariablePatch, ValueMode,
};
use simple_post_core::dynamic::{parse_raw, serialize_value, ResolveContext};
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
    pub extract_path: Option<String>,
    pub value_mode: Option<ValueMode>,
    pub ttl_seconds: Option<i64>,
    pub stale_fallback: Option<bool>,
    pub enabled: Option<bool>,
    pub sort_order: Option<i64>,
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
}

/// 一次解析/读写用的环境键。
///
/// 环境 id 的取法在**所有**路径上必须一致：`None` = 用项目当前激活的那个环境。这与变量替换
/// 用的那一份环境是同一条规则（`mcp::resolve_env`）。
///
/// 两条路径若各取各的，缓存的键就会在「写」与「读」之间漂移：覆盖值落到空键上，而解析去
/// 读激活环境那一行，于是手动指定的值看不见自己。
fn environment_key(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<String, AppError> {
    Ok(crate::mcp::resolve_env(pool, environment_id, project_id)?
        .map(|e| e.id)
        .unwrap_or_default())
}

/// 列出本项目的全部定义，带当前值与缓存状态。
///
/// **不取数**：读的是缓存行。取数可能发出站请求，而这是一次纯读。
pub fn list(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<Vec<DynamicVariableView>, AppError> {
    let definitions = db::with_conn(pool, |c| {
        db::dynamic_variables::list_by_project(c, project_id)
    })?;
    let env_key = environment_key(pool, project_id, environment_id)?;
    let now = now_ms();
    let mut out = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let entry = db::with_conn(pool, |c| {
            db::dynamic_variable_cache::get(c, &definition.id, &env_key)
        })?;
        out.push(view_of(definition, entry.as_ref(), now));
    }
    Ok(out)
}

/// 读一条定义，带当前值与缓存状态。
pub fn get(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicVariableView, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let env_key = environment_key(pool, &definition.project_id, environment_id)?;
    let entry = db::with_conn(pool, |c| {
        db::dynamic_variable_cache::get(c, definition_id, &env_key)
    })?;
    Ok(view_of(definition, entry.as_ref(), now_ms()))
}

/// 新建一条定义。
pub fn create(
    pool: &DbPool,
    project_id: &str,
    opts: CreateOpts,
) -> Result<DynamicVariable, AppError> {
    let now = now_ms();
    let definition = DynamicVariable {
        id: opts.id.unwrap_or_else(uid),
        project_id: project_id.to_string(),
        name: opts.name,
        description: opts.description,
        source_endpoint_id: opts.source_endpoint_id,
        source_endpoint_name: None,
        source_inline_json: opts.source_inline_json,
        extract_path: opts.extract_path.unwrap_or_else(|| "$".to_string()),
        value_mode: opts.value_mode.unwrap_or_default(),
        ttl_seconds: opts.ttl_seconds.unwrap_or(60),
        stale_fallback: opts.stale_fallback.unwrap_or(false),
        enabled: opts.enabled.unwrap_or(true),
        sort_order: opts.sort_order.unwrap_or(0),
        created_at: now,
        updated_at: now,
    };
    db::with_tx(pool, |c| {
        db::dynamic_variables::create(c, &definition, project_id)
    })?;
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
/// `environment_id` 缺席时取项目当前激活的环境 —— 与界面那侧同一套规则。
pub async fn resolve(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
    names: &[String],
    force: bool,
) -> Result<DynamicResolution, AppError> {
    let ctx = context(pool, project_id, environment_id)?;
    simple_post_core::dynamic::resolve_names(&ctx, names, force, &crate::dynamic::fetcher()).await
}

/// 强制重新取一条定义。
pub async fn refresh(
    pool: &DbPool,
    project_id: &str,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicValueStatus, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let names = vec![definition.name.clone()];
    let resolution = resolve(pool, project_id, environment_id, &names, true).await?;
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
/// 覆盖值按（定义 × 环境）存放，故测试环境改了值不影响生产环境。
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
    environment_id: Option<&str>,
    value: Option<String>,
    expires_at: Option<i64>,
    forever: bool,
) -> Result<(), AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    // 环境键与解析用的那一份一致（见 `environment_key`）：否则覆盖值会落到另一个键上，
    // 手动指定的值看不见自己。
    let env_key = environment_key(pool, &definition.project_id, environment_id)?;
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

/// 一次解析的上下文。
fn context(
    pool: &DbPool,
    project_id: &str,
    environment_id: Option<&str>,
) -> Result<ResolveContext, AppError> {
    let env = crate::mcp::resolve_env(pool, environment_id, project_id)?;
    let project = db::with_conn(pool, |c| db::projects::get(c, project_id))
        .unwrap_or_else(|_| crate::mcp::http_ops::EMPTY_PROJECT.clone());
    Ok(ResolveContext {
        pool: pool.clone(),
        project_id: project_id.to_string(),
        environment_id: env
            .as_ref()
            .map(|e| e.id.clone())
            .or_else(|| environment_id.map(str::to_string)),
        env,
        project,
    })
}

/// 一条定义 + 它的缓存行 → 视图。
fn view_of(
    definition: DynamicVariable,
    entry: Option<&DynamicCacheEntry>,
    now: i64,
) -> DynamicVariableView {
    let (value, source, fetched_at, expires_at, is_override) = match entry {
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
    DynamicVariableView {
        definition,
        value,
        source,
        fetched_at,
        expires_at,
        is_override,
        error: None,
    }
}
