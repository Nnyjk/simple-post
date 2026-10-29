//! 编排：取数、抽取、落缓存。
//!
//! 取数失败不算整次发送失败：变量按缺失处理，`{{name}}` 原样留在请求里，错误另记一条。
//! 退化顺序：覆盖值 → 过期缓存值（仅当 `stale_fallback` 为真） → 按缺失处理。

use crate::db;
use crate::domain::{
    DynamicCacheEntry, DynamicError, DynamicErrorKind, DynamicResolution, DynamicValueSource,
    DynamicValueStatus, DynamicVariable,
};
use crate::error::AppError;

use super::cache::{decide, Decision};
use super::extract::extract_value;
use super::fetcher::{ResolveContext, SourceFetcher};
use super::flight::flight_lock;
use super::serialize::{parse_raw, serialize_value};
use super::source::{source_context, SourceContext};

/// 一个值的体积上限。超过它记一条错误，而不是把超大文本塞进缓存表。
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

/// 解析一批名字，返回它们的值、状态与错误。
///
/// `project_id` 决定这些名字在哪个范围里找：
///
/// - `Some(项目)` = **项目内取值**：只认那个项目里的定义。找不到就是缺失，`{{name}}`
///   原样留在请求里。请求发送走的就是这条——消费方项目里没有这个名字，它不会去别的
///   项目拿同名的那条。
/// - `None` = **跨项目取值**（调用接口那条路）：同一个名字在多个项目里都有定义时，
///   不知道该用哪一条，故记一条「歧义」错误并按缺失处理，不猜。
///
/// 名字里没有对应定义的（那是环境变量，或者是拼错的名字）直接跳过——它们由变量替换
/// 那一步按缺失处理。停用的定义也跳过：`{{name}}` 原样留着并进未解析变量列表。
pub async fn resolve_names(
    ctx: &ResolveContext,
    project_id: Option<&str>,
    names: &[String],
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<DynamicResolution, AppError> {
    let mut resolution = DynamicResolution::default();

    for name in names {
        let (def, lookup_error) = lookup(ctx, project_id, name)?;
        if let Some(error) = lookup_error {
            resolution.errors.push(error);
        }
        let Some(def) = def else {
            continue;
        };
        if !def.enabled {
            continue;
        }
        let status = resolve_definition(ctx, &def, force, fetcher).await?;
        if let Some(value) = &status.value {
            resolution.values.insert(name.clone(), value.clone());
        }
        if let Some(error) = &status.error {
            resolution.errors.push(error.clone());
        }
        resolution.statuses.push(status);
    }

    Ok(resolution)
}

/// 解析一条**已知**的定义，返回它的状态。
///
/// 强制重取走的是这条：它手里已经有那条定义了，不必再按名字找一遍——按名字找会撞上
/// 同名定义在别的项目里的那种歧义。
pub async fn resolve_definition(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<DynamicValueStatus, AppError> {
    let now = crate::domain::now_ms();
    let source = source_context(&ctx.pool, def)?;
    let outcome = resolve_one(ctx, def, &source, now, force, fetcher).await?;
    Ok(DynamicValueStatus {
        name: def.name.clone(),
        value: outcome.value,
        source: outcome.source,
        fetched_at: outcome.fetched_at,
        expires_at: outcome.expires_at,
        is_override: outcome.is_override,
        error: outcome.error,
    })
}

/// 找到一个名字对应的定义。
///
/// 项目内取值：只查那个项目。跨项目取值：查全部项目，同名多条即歧义——记一条错误
/// 并按缺失处理。
fn lookup(
    ctx: &ResolveContext,
    project_id: Option<&str>,
    name: &str,
) -> Result<(Option<DynamicVariable>, Option<DynamicError>), AppError> {
    match project_id {
        Some(project_id) => {
            let found = db::with_conn(&ctx.pool, |c| {
                db::dynamic_variables::get_by_name_in_project(c, project_id, name)
            })?;
            Ok((found, None))
        }
        None => {
            let candidates = db::with_conn(&ctx.pool, |c| {
                db::dynamic_variables::get_by_name_across(c, name)
            })?;
            match candidates.len() {
                0 => Ok((None, None)),
                1 => Ok((candidates.into_iter().next(), None)),
                _ => {
                    // 歧义：跨项目取值时同一个名字有多条定义，不猜。
                    let projects: Vec<String> = candidates
                        .iter()
                        .map(|d| d.project_id.clone().unwrap_or_else(|| "未归属".to_string()))
                        .collect();
                    let error = DynamicError {
                        name: name.to_string(),
                        kind: DynamicErrorKind::Ambiguous,
                        message: format!(
                            "变量 '{}' 在多个项目里都有定义（{}），跨项目取值时不知道该用哪一条",
                            name,
                            projects.join("、")
                        ),
                    };
                    Ok((None, Some(error)))
                }
            }
        }
    }
}

/// 一条定义的解析结果。
struct OneOutcome {
    value: Option<String>,
    source: DynamicValueSource,
    fetched_at: Option<i64>,
    expires_at: Option<i64>,
    is_override: bool,
    error: Option<DynamicError>,
}

impl OneOutcome {
    fn missing(error: Option<DynamicError>) -> OneOutcome {
        OneOutcome {
            value: None,
            source: DynamicValueSource::Missing,
            fetched_at: None,
            expires_at: None,
            is_override: false,
            error,
        }
    }
}

/// 解析一条定义。
async fn resolve_one(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    source: &SourceContext,
    now: i64,
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    let env_key = source.environment_id.clone().unwrap_or_default();
    let entry = db::with_conn(&ctx.pool, |c| {
        db::dynamic_variable_cache::get(c, &def.id, &env_key)
    })?;

    match decide(def, entry.as_ref(), now, force) {
        Decision::UseOverride { value_json } => {
            let value = serialize_value(&parse_raw(&value_json), def.value_mode);
            Ok(OneOutcome {
                value: Some(value),
                source: DynamicValueSource::Override,
                fetched_at: entry.as_ref().map(|e| e.fetched_at),
                expires_at: entry.as_ref().and_then(|e| e.override_expires_at),
                is_override: true,
                error: None,
            })
        }
        Decision::UseCache { value_json } => {
            let value = serialize_value(&parse_raw(&value_json), def.value_mode);
            Ok(OneOutcome {
                value: Some(value),
                source: DynamicValueSource::Auto,
                fetched_at: entry.as_ref().map(|e| e.fetched_at),
                expires_at: entry.as_ref().map(|e| e.expires_at),
                is_override: false,
                error: None,
            })
        }
        Decision::Fetch { stale_fallback } => {
            fetch_one(ctx, def, source, now, force, stale_fallback, fetcher).await
        }
    }
}

/// 取数、抽取、落缓存。
///
/// single-flight 在这里生效：同一个（定义 × 环境）同时只发一次取数请求，其余等待同一个结果。
/// 拿到锁之后**重新查一次缓存**——前一个等待者可能刚把值填好。
async fn fetch_one(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    source: &SourceContext,
    now: i64,
    force: bool,
    stale_fallback: Option<String>,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    // 缓存键的环境段取自取数上下文（定义点名的环境，或它所属项目的第一个可用环境），
    // 不是消费方的环境。
    let env_key = source.environment_id.clone().unwrap_or_default();
    let lock = flight_lock(&(def.id.clone(), env_key.clone()));
    let _guard = lock.lock().await;

    // 本次解析重新查一次缓存：等锁期间前一个等待者可能已经取好并落库了。
    let entry = db::with_conn(&ctx.pool, |c| {
        db::dynamic_variable_cache::get(c, &def.id, &env_key)
    })?;
    if !force {
        if let Some(e) = &entry {
            if !e.is_override && e.expires_at > now {
                if let Some(value_json) = &e.value_json {
                    let value = serialize_value(&parse_raw(value_json), def.value_mode);
                    return Ok(OneOutcome {
                        value: Some(value),
                        source: DynamicValueSource::Auto,
                        fetched_at: Some(e.fetched_at),
                        expires_at: Some(e.expires_at),
                        is_override: false,
                        error: None,
                    });
                }
            }
        }
    }

    let raw = match fetcher.fetch(def, ctx, source).await {
        Ok(raw) => raw,
        Err(error) => {
            // 取数失败：本次解析改用过期缓存值（定义允许时）。
            if let Some(stale) = stale_fallback {
                let value = serialize_value(&parse_raw(&stale), def.value_mode);
                return Ok(OneOutcome {
                    value: Some(value),
                    source: DynamicValueSource::Stale,
                    fetched_at: None,
                    expires_at: None,
                    is_override: false,
                    error: Some(error),
                });
            }
            return Ok(OneOutcome::missing(Some(error)));
        }
    };

    if !(200..300).contains(&raw.status) {
        let error = DynamicError {
            name: def.name.clone(),
            kind: DynamicErrorKind::Status,
            message: format!("数据源返回 {}", raw.status),
        };
        return Ok(fallback_or_missing(def, stale_fallback, error));
    }

    let extracted = match extract_value(&raw.body, &def.extract_path) {
        Ok(value) => value,
        Err(kind) => {
            let error = DynamicError {
                name: def.name.clone(),
                kind,
                message: match kind {
                    DynamicErrorKind::NotJson => "数据源的响应不是 JSON".to_string(),
                    DynamicErrorKind::NoMatch => {
                        format!("抽取路径 '{}' 没有命中", def.extract_path)
                    }
                    _ => format!("抽取路径 '{}' 不是合法的 JSONPath", def.extract_path),
                },
            };
            return Ok(fallback_or_missing(def, stale_fallback, error));
        }
    };

    // 处理脚本：抽取之后跑。JSONPath 负责「取哪一段」，脚本负责「这一段怎么变成值」。
    let extracted = match super::transform::apply(def, extracted, &raw, source).await {
        Ok(value) => value,
        Err(error) => return Ok(fallback_or_missing(def, stale_fallback, error)),
    };

    let raw_json = serde_json::to_string(&extracted).unwrap_or_default();
    let serialized = serialize_value(&extracted, def.value_mode);
    if serialized.len() > MAX_VALUE_BYTES {
        let error = DynamicError {
            name: def.name.clone(),
            kind: DynamicErrorKind::TooLarge,
            message: format!(
                "取回的值有 {} 字节，超过上限 {} 字节",
                serialized.len(),
                MAX_VALUE_BYTES
            ),
        };
        // 本次解析不把超大的值写进缓存表，但值已经取回来了——回退候选仍然可用。
        return Ok(fallback_or_missing(def, stale_fallback, error));
    }

    let expires_at = now + def.ttl_seconds * 1000;
    let entry = DynamicCacheEntry {
        definition_id: def.id.clone(),
        environment_id: env_key.clone(),
        value_json: Some(raw_json),
        fetched_at: now,
        expires_at,
        is_override: false,
        override_expires_at: None,
    };
    let stored = db::with_tx(&ctx.pool, |c| db::dynamic_variable_cache::upsert(c, &entry));
    if let Err(e) = stored {
        // 落库失败不影响这次取值：值已经在手，故记一条错误但仍然用它。
        let error = DynamicError {
            name: def.name.clone(),
            kind: DynamicErrorKind::SourceError,
            message: format!("缓存写入失败：{e}"),
        };
        return Ok(OneOutcome {
            value: Some(serialized),
            source: DynamicValueSource::Auto,
            fetched_at: Some(now),
            expires_at: Some(expires_at),
            is_override: false,
            error: Some(error),
        });
    }

    Ok(OneOutcome {
        value: Some(serialized),
        source: DynamicValueSource::Auto,
        fetched_at: Some(now),
        expires_at: Some(expires_at),
        is_override: false,
        error: None,
    })
}

/// 取数失败（或值不可用）时的退化：有回退候选就用它并标「过期值」，否则按缺失处理。
fn fallback_or_missing(
    def: &DynamicVariable,
    stale_fallback: Option<String>,
    error: DynamicError,
) -> OneOutcome {
    match stale_fallback {
        Some(stale) => {
            let value = serialize_value(&parse_raw(&stale), def.value_mode);
            OneOutcome {
                value: Some(value),
                source: DynamicValueSource::Stale,
                fetched_at: None,
                expires_at: None,
                is_override: false,
                error: Some(error),
            }
        }
        None => OneOutcome::missing(Some(error)),
    }
}
