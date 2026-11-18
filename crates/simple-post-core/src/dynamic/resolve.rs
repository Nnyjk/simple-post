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

/// 解析一批名字，返回它们的值、状态与错误。`project_id` 决定这些名字在哪个范围里找：
///
/// - `Some(项目)` = **项目内取值**：只认那个项目里的定义，找不到就是缺失，`{{name}}` 原样
///   留在请求里。
/// - `None` = **跨项目取值**（调用接口那条路）：同一个名字在多个项目里都有定义时，记一条
///   「歧义」错误并按缺失处理，不猜。
///
/// 名字里没有对应定义的直接跳过——它们由变量替换那一步按缺失处理。停用的定义也跳过。
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

/// 解析一条**已知**的定义，返回它的状态。强制重取走的是这条：它手里已经有那条定义了，
/// 不必再按名字找一遍。
pub async fn resolve_definition(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<DynamicValueStatus, AppError> {
    let now = crate::domain::now_ms();
    // 取数上下文造不出来（数据源端点已被删掉、那一行读不出来……）时，这一次解析按缺失
    // 处理并记一条错误，而不是让整次发送在请求发出前中止。`dynamic/mod.rs` 的规范
    // 如此要求：取数失败不让整次发送失败。
    let source = match source_context(&ctx.pool, def, ctx.environment_id.as_deref()) {
        Ok(source) => source,
        Err(e) => {
            let error = DynamicError {
                name: def.name.clone(),
                kind: match &e {
                    AppError::NotFound(_) => DynamicErrorKind::SourceMissing,
                    _ => DynamicErrorKind::SourceError,
                },
                message: format!("取数上下文构造失败：{e}"),
            };
            return Ok(DynamicValueStatus {
                name: def.name.clone(),
                value: None,
                source: DynamicValueSource::Missing,
                fetched_at: None,
                expires_at: None,
                is_override: false,
                error: Some(error),
            });
        }
    };
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

/// 找到一个名字对应的定义。项目内取值：只查那个项目。跨项目取值：查全部项目，同名多条即
/// 歧义——记一条错误并按缺失处理。
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

    // 进门这一刻那一行是不是仍然生效的覆盖值。写回侧要保护的只是**这之后**才写下的手工值，
    // 故这个判断取的是此刻——不是锁后那一次（见 `fetch_one`）。
    let had_live_override = entry
        .as_ref()
        .map(|e| live_override(e, now))
        .unwrap_or(false);

    match decide(def, entry.as_ref(), now, force) {
        Decision::UseOverride { value_json } => {
            Ok(override_outcome(def, entry.as_ref(), &value_json))
        }
        Decision::UseCache { value_json } => Ok(cache_outcome(def, entry.as_ref(), &value_json)),
        // 这次结论只说「大概要走取数」：单飞行锁要等，等的那段时间缓存行还会变，故
        // `fetch_one` 拿到锁之后会重算一次。
        Decision::Fetch { .. } => {
            fetch_one(ctx, def, source, force, had_live_override, fetcher).await
        }
    }
}

/// 用覆盖值这条路的出口。
fn override_outcome(
    def: &DynamicVariable,
    entry: Option<&DynamicCacheEntry>,
    value_json: &str,
) -> OneOutcome {
    let value = serialize_value(&parse_raw(value_json), def.value_mode);
    OneOutcome {
        value: Some(value),
        source: DynamicValueSource::Override,
        fetched_at: entry.map(|e| e.fetched_at),
        expires_at: entry.and_then(|e| e.override_expires_at),
        is_override: true,
        error: None,
    }
}

/// 用新鲜缓存这条路的出口。
fn cache_outcome(
    def: &DynamicVariable,
    entry: Option<&DynamicCacheEntry>,
    value_json: &str,
) -> OneOutcome {
    let value = serialize_value(&parse_raw(value_json), def.value_mode);
    OneOutcome {
        value: Some(value),
        source: DynamicValueSource::Auto,
        fetched_at: entry.map(|e| e.fetched_at),
        expires_at: entry.map(|e| e.expires_at),
        is_override: false,
        error: None,
    }
}

/// 取数、抽取、落缓存。
///
/// single-flight 在这里生效：同一个（定义 × 环境）同时只发一次取数请求，其余等待同一个结果。
/// 拿到锁之后**重读缓存行并重跑 `decide`**——等锁那段时间别的连接可以填好缓存、也可以写
/// 覆盖值，故进门时那份结论只当「大概要走取数」，真正的结论在锁后取。
async fn fetch_one(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    source: &SourceContext,
    force: bool,
    had_live_override: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    // 缓存键的环境段取自取数上下文——**实际生效**的那个环境（指定模式是定义点名的那个，
    // 继承模式是调用方选中的那个）。它随调用方切环境而变，故切环境不会读到上一个环境的值。
    let env_key = source.environment_id.clone().unwrap_or_default();
    let lock = flight_lock(&(def.id.clone(), env_key.clone()));
    let _guard = lock.lock().await;

    // 等锁那段时间缓存行会变：前一个等待者可能刚把值填好，用户在另一条连接上可能刚点
    // 「保存为覆盖值」。故拿到锁之后按锁后这一刻的时刻重读缓存行、重跑同一个 `decide`，
    // 用它这一刻的结论——结论变成「用手工值」或「用新鲜缓存」就直接返回，不发请求，也不进
    // 写回事务。写回侧仍用进门时那份 `had_live_override`：刷新在等锁期间被写下的手工值
    // 也算「途中写的」，不该由这次刷新顶掉。
    let now = crate::domain::now_ms();
    let entry = db::with_conn(&ctx.pool, |c| {
        db::dynamic_variable_cache::get(c, &def.id, &env_key)
    })?;
    let stale_fallback = match decide(def, entry.as_ref(), now, force) {
        Decision::UseOverride { value_json } => {
            return Ok(override_outcome(def, entry.as_ref(), &value_json));
        }
        Decision::UseCache { value_json } => {
            return Ok(cache_outcome(def, entry.as_ref(), &value_json));
        }
        Decision::Fetch { stale_fallback } => stale_fallback,
    };

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
    // 取数期间用户可能改了定义：数据源 / 抽取规则 / 处理脚本 / 取数环境一变，值该怎么算就
    // 变了，写路径那时已经把这条定义的缓存行删掉。这次 fetch 用的是开始那份定义，故比对与
    // 写入放在**同一个事务**里——中间插不进另一次修改；定义不再是开始那一份就跳过写入。
    // 值照常返回给这次调用方：它只是不进缓存，下一次解析会按新定义重新取数。
    //
    // 同一个事务里还要看一眼当前的缓存行：用户在取数途中（锁前、锁后都算）点了「保存为
    // 覆盖值」，那一行是他在这个环境上亲手钉住的，而这次取数只是「本该这样算」，不该盖过
    // 人手写的值。
    // 只在**进门那一刻还没有**有效覆盖值时才保护——那一刻就有的话，这次取数只可能是刷新
    // 走上来的（`cache::decide` 的 `force` 特意跳过覆盖值），刷新要的就是把那份手工值换掉。
    // 反过来，刷新在等锁期间被别人写下的手工值也算「途中写的」：`had_live_override` 取的是
    // 进门那一刻，不是锁后那一次。
    let stored = db::with_tx(&ctx.pool, |c| {
        let current = match db::dynamic_variables::get(c, &def.id) {
            Ok(current) => current,
            // 定义已经不在了：值仍可交给这次调用方，但没有缓存可写。
            Err(AppError::NotFound(_)) => return Ok(false),
            Err(e) => return Err(e),
        };
        if !same_fetch_inputs(&current, def) {
            return Ok(false);
        }
        if !had_live_override {
            if let Some(existing) = db::dynamic_variable_cache::get(c, &def.id, &env_key)? {
                if live_override(&existing, crate::domain::now_ms()) {
                    return Ok(false);
                }
            }
        }
        db::dynamic_variable_cache::upsert(c, &entry)?;
        Ok(true)
    });
    match stored {
        Ok(true) => {}
        Ok(false) => {
            return Ok(OneOutcome {
                value: Some(serialized),
                source: DynamicValueSource::Auto,
                fetched_at: Some(now),
                // 没有落库，故没有过期时间可报：下一次解析会重新取数。
                expires_at: None,
                is_override: false,
                error: None,
            });
        }
        Err(e) => {
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

/// 这一行是**仍然生效**的覆盖值吗：人手写的，且没有到期或还没到。
///
/// 「仍然生效」按调用方给的时刻算，故取数开始与写回两处可以用不同的时刻各判一次。
fn live_override(entry: &DynamicCacheEntry, now: i64) -> bool {
    entry.is_override
        && entry
            .override_expires_at
            .map(|deadline| deadline > now)
            .unwrap_or(true)
}

/// 这两份定义算出来的值会一样吗：数据源、取数环境、抽取规则、处理脚本。
///
/// 只有决定「取哪一段、怎么加工」的字段在这里；名字、描述、TTL、`value_mode` 改了不影响
/// 这份值的语义（`value_mode` 在读取时现算），故不必让这次取数作废。
fn same_fetch_inputs(current: &DynamicVariable, fetched: &DynamicVariable) -> bool {
    current.source_endpoint_id == fetched.source_endpoint_id
        && current.source_inline_json == fetched.source_inline_json
        && current.source_environment_id == fetched.source_environment_id
        && current.extract_path == fetched.extract_path
        && current.transform_script == fetched.transform_script
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
