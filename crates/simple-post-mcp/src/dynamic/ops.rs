//! 动态变量的公共操作 —— MCP 工具与两侧 transport 共用。
//!
//! 放在 mcp crate 而不是 server crate：MCP 工具在本 crate 内，它不能依赖上层；而 REST 与
//! Tauri 两侧都能依赖本 crate。故公共操作住在这一层，server crate 的 `dynamic_api` 只是
//! 它的一层薄壳。
//!
//! 定义挂在**项目**里：变量名在项目内唯一，不同项目可以有同名变量而互不覆盖。取数用的
//! 环境有两种形态：定义点名的那一个（**指定**），或调用方此刻选中的那一个（**继承**，
//! 它不属于数据源端点所属项目时回落到那个项目的默认环境）。故同一个 `{{name}}` 在不同
//! 项目、不同环境里取到的值可以不同——那正是分项目与分环境的意义。

use serde::Serialize;

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{
    now_ms, uid, DynamicCacheEntry, DynamicError, DynamicErrorKind, DynamicResolution,
    DynamicValueSource, DynamicValueStatus, DynamicVariable, DynamicVariablePatch, Environment,
    ValueMode,
};
use simple_post_core::dynamic::{parse_raw, serialize_value, ResolveContext, SourceContext};
use simple_post_core::error::AppError;

/// 新建一条定义时要填的字段。缺席的字段取默认值。
#[derive(Debug, Clone, Default)]
pub struct CreateOpts {
    /// 渲染层自己铸的 id。`None` = 由服务端铸。
    pub id: Option<String>,
    /// 归属项目。定义挂在项目里，故不同项目可以有同名变量而互不覆盖。
    pub project_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub source_endpoint_id: Option<String>,
    pub source_inline_json: Option<String>,
    /// 取数用的环境。`None` = 继承调用方此刻选中的环境（不属于数据源端点所属项目时，
    /// 回落到那个项目的默认环境）。
    pub source_environment_id: Option<String>,
    pub extract_path: Option<String>,
    /// 处理脚本。抽取之后跑。
    pub transform_script: Option<String>,
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
    /// 数据源端点所属的项目。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_project_id: Option<String>,
    /// 数据源端点所属项目的名字。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_project_name: Option<String>,
    /// 实际取数用的环境：定义点名的那个（指定），或继承来的那个（含回落后的默认环境）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_environment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_environment_name: Option<String>,
}

/// 列出定义，带当前值与缓存状态。
///
/// `project_id` 决定列哪个范围：给了就只列那个项目里的，不给就列全部项目——跨项目
/// 调用接口要靠后者。
///
/// `environment_id` 是**调用方此刻选中的环境**：它只对「继承取数环境」的定义起作用
/// （见 `core::dynamic::source_context`）。缓存行按实际生效的环境取，故这里给出的值与
/// 随后解析时看到的是同一行。
///
/// **不取数**：读的是缓存行。取数可能发出站请求，而这是一次纯读。
///
/// **一条坏行不拖垮整张列表**：取数上下文构造不出来的定义（点名的环境属于别的项目、
/// 环境行已删）只让自己那一行没有值，错误记在那一行的 `error` 上。面板一次列全部定义，
/// 整批失败等于那个 tab 打不开，比展示一条带错误的行更难收拾。
///
/// **不带「哪些端点引用了它」**：那条信息只在接口那侧存在——接口知道自己配了哪些变量，
/// 变量这一侧不维护反向索引。一个被几乎每个接口用到的通用变量会把这条列表撑爆，
/// 而那条信息对维护变量本身没有用。
pub fn list(
    pool: &DbPool,
    project_id: Option<&str>,
    environment_id: Option<&str>,
) -> Result<Vec<DynamicVariableView>, AppError> {
    let definitions = match project_id {
        Some(project_id) => db::with_conn(pool, |c| {
            db::dynamic_variables::list_by_project(c, project_id)
        })?,
        None => db::with_conn(pool, db::dynamic_variables::list)?,
    };
    let now = now_ms();
    let mut out = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let source =
            match simple_post_core::dynamic::source_context(pool, &definition, environment_id) {
                Ok(source) => source,
                Err(err) => {
                    let error = context_error(&definition, err);
                    out.push(view_of_failed_context(definition, error));
                    continue;
                }
            };
        let env_key = source.environment_id.clone().unwrap_or_default();
        let entry = db::with_conn(pool, |c| {
            db::dynamic_variable_cache::get(c, &definition.id, &env_key)
        })?;
        out.push(view_of(definition, &source, entry.as_ref(), now));
    }
    Ok(out)
}

/// 读一条定义，带当前值与缓存状态。
///
/// 取数上下文构造失败时同样降级：返回这一行、`error` 里带原因，而不是整次读取失败——
/// 详情页要能显示「这条定义的环境是坏的」，那比一个 400 有用。
pub fn get(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicVariableView, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let source = match simple_post_core::dynamic::source_context(pool, &definition, environment_id)
    {
        Ok(source) => source,
        Err(err) => {
            let error = context_error(&definition, err);
            return Ok(view_of_failed_context(definition, error));
        }
    };
    let env_key = source.environment_id.clone().unwrap_or_default();
    let entry = db::with_conn(pool, |c| {
        db::dynamic_variable_cache::get(c, definition_id, &env_key)
    })?;
    Ok(view_of(definition, &source, entry.as_ref(), now_ms()))
}

/// 新建一条定义。
pub fn create(pool: &DbPool, opts: CreateOpts) -> Result<DynamicVariable, AppError> {
    let now = now_ms();
    let definition = DynamicVariable {
        id: opts.id.unwrap_or_else(uid),
        project_id: opts.project_id,
        name: opts.name,
        description: opts.description,
        source_endpoint_id: opts.source_endpoint_id,
        source_endpoint_name: None,
        source_inline_json: opts.source_inline_json,
        source_environment_id: opts.source_environment_id,
        extract_path: opts.extract_path.unwrap_or_else(|| "$".to_string()),
        transform_script: opts.transform_script,
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
/// `project_id` 决定这些名字在哪个范围里找：给了就只认那个项目里的定义（请求发送走
/// 的就是这条）；不给就是跨项目取值，同一个名字在多个项目里都有定义时记一条「歧义」
/// 错误并按缺失处理。
///
/// `environment_id` 是调用方此刻选中的环境，只对继承取数环境的定义起作用。
pub async fn resolve(
    pool: &DbPool,
    project_id: Option<&str>,
    names: &[String],
    force: bool,
    environment_id: Option<&str>,
) -> Result<DynamicResolution, AppError> {
    let ctx = ResolveContext {
        pool: pool.clone(),
        environment_id: environment_id.map(str::to_string),
    };
    simple_post_core::dynamic::resolve_names(
        &ctx,
        project_id,
        names,
        force,
        &crate::dynamic::fetcher(),
    )
    .await
}

/// 强制重新取一条定义。
///
/// 按 id 解析，不按名字：按名字找会撞上同名定义在别的项目里的那种歧义。
pub async fn refresh(
    pool: &DbPool,
    definition_id: &str,
    environment_id: Option<&str>,
) -> Result<DynamicValueStatus, AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    let ctx = ResolveContext {
        pool: pool.clone(),
        environment_id: environment_id.map(str::to_string),
    };
    simple_post_core::dynamic::resolve_definition(
        &ctx,
        &definition,
        true,
        &crate::dynamic::fetcher(),
    )
    .await
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
    environment_id: Option<&str>,
) -> Result<(), AppError> {
    let definition = db::with_conn(pool, |c| db::dynamic_variables::get(c, definition_id))?;
    // 缓存键用定义**实际生效**的取数环境，与解析时用的那一个同源：否则覆盖值会落到另一个
    // 键上，手动指定的值看不见自己。继承模式下这个环境随调用方的选择变，故调用方要给。
    let source = simple_post_core::dynamic::source_context(pool, &definition, environment_id)?;
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

/// 取数上下文构造失败的那一行：没有值、没有缓存时间，原因放在 `error` 上。
///
/// 项目与环境都是空的——上下文没算出来，就没有「值从哪个环境来」可报。行本身的身份
/// （id、名字、数据源、抽取规则）照旧给出，用户要靠它找到那条坏定义。
fn view_of_failed_context(definition: DynamicVariable, error: DynamicError) -> DynamicVariableView {
    DynamicVariableView {
        definition,
        value: None,
        source: DynamicValueSource::Missing,
        fetched_at: None,
        expires_at: None,
        is_override: false,
        error: Some(error),
        source_project_id: None,
        source_project_name: None,
        effective_environment_id: None,
        effective_environment_name: None,
    }
}

/// 取数上下文构造失败的原因 → 行上的那条错误。
///
/// 校验与「行不存在」两种错误带的就是给人看的那句话（与保存侧逐字相同）；其它错误
/// （数据库坏了）原样转写，别把它说成配置错误。
fn context_error(definition: &DynamicVariable, err: AppError) -> DynamicError {
    let message = match &err {
        AppError::Validation(message) | AppError::NotFound(message) => message.clone(),
        other => other.to_string(),
    };
    DynamicError {
        name: definition.name.clone(),
        kind: DynamicErrorKind::SourceError,
        message,
    }
}

/// 一条定义 + 它的缓存行 → 视图。
fn view_of(
    definition: DynamicVariable,
    source: &SourceContext,
    entry: Option<&DynamicCacheEntry>,
    now: i64,
) -> DynamicVariableView {
    // 没有数据源的定义不展示自动值：数据源接口被删掉之后，那份值已经没有数据源支撑，
    // 解析也按缺失处理（见 `core::dynamic::cache::decide`）。覆盖值是人手写的，照旧展示。
    let entry = match entry {
        Some(e) if !e.is_override && !definition.has_source() => None,
        other => other,
    };
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
    }
}
