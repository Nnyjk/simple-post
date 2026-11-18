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
    let outcome = resolve_one(ctx, def, &source, force, fetcher).await?;
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
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    let env_key = source.environment_id.clone().unwrap_or_default();
    // 没有数据源时算不出环境（缓存键退化成空键），而覆盖值当初是按它那时生效的环境键写下的
    // ——故这时横跨定义的所有环境键找仍然生效的那一份，人手写的值不因为数据源被删就消失。
    // 只找覆盖值：没有数据源支撑的自动值不该被这样翻出来。
    let entry = if def.has_source() {
        db::with_conn(&ctx.pool, |c| {
            db::dynamic_variable_cache::get(c, &def.id, &env_key)
        })?
    } else {
        let scanned_at = crate::domain::now_ms();
        db::with_conn(&ctx.pool, |c| {
            db::dynamic_variable_cache::live_overrides_for_definition(c, &def.id, scanned_at)
        })?
        .into_iter()
        .next()
    };
    // 判活用的时刻不早于读到的那一行：先读行、再取时刻，恰好在这两个动作之间到期的覆盖值
    // 才判得成过期，而不是被当成活值返回。
    let now = crate::domain::now_ms();

    // 进门这一刻那份仍然生效的覆盖值长什么样。写回侧要分辨「进门就有、这次刷新该换掉的」
    // 与「取数途中才写下的、必须留住的」——只记「有没有覆盖值」会把后者当成前者冲掉。
    let seen_override = entry
        .as_ref()
        .filter(|e| live_override(e, now))
        .map(OverrideRef::of);

    match decide(def, entry.as_ref(), now, force) {
        Decision::UseOverride { value_json } => {
            Ok(override_outcome(def, entry.as_ref(), &value_json))
        }
        Decision::UseCache { value_json } => Ok(cache_outcome(def, entry.as_ref(), &value_json)),
        // 这次结论只说「大概要走取数」：单飞行锁要等，等的那段时间缓存行还会变，故
        // `fetch_one` 拿到锁之后会重算一次。
        Decision::Fetch => fetch_one(ctx, def, source, force, seen_override, fetcher).await,
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
    seen_override: Option<OverrideRef>,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    // 缓存键的环境段取自取数上下文——**实际生效**的那个环境（指定模式是定义点名的那个，
    // 继承模式是调用方选中的那个）。它随调用方切环境而变，故切环境不会读到上一个环境的值。
    let env_key = source.environment_id.clone().unwrap_or_default();
    let lock = flight_lock(&(def.id.clone(), env_key.clone()));
    // 测试用它观察「进门那一次已经算完、正排在锁上」——等锁不可观测，而固定时长的等待
    // 证明不了对方已经走到这一步。
    #[cfg(test)]
    if let Some(hook) = &ctx.lock_wait {
        let _ = hook.send(());
    }
    let _guard = lock.lock().await;

    // 等锁那段时间缓存行会变：前一个等待者可能刚把值填好，用户在另一条连接上可能刚点
    // 「保存为覆盖值」。故拿到锁之后重读缓存行、重跑同一个 `decide`，用它这一刻的结论——
    // 结论变成「用手工值」或「用新鲜缓存」就直接返回，不发请求，也不进写回事务。
    // 时刻在这行之后取：判活用的时刻不早于读到的那一行。
    let entry = db::with_conn(&ctx.pool, |c| {
        db::dynamic_variable_cache::get(c, &def.id, &env_key)
    })?;
    let now = crate::domain::now_ms();
    match decide(def, entry.as_ref(), now, force) {
        Decision::UseOverride { value_json } => {
            return Ok(override_outcome(def, entry.as_ref(), &value_json));
        }
        Decision::UseCache { value_json } => {
            return Ok(cache_outcome(def, entry.as_ref(), &value_json));
        }
        Decision::Fetch => {}
    }

    let raw = match fetcher.fetch(def, ctx, source).await {
        Ok(raw) => raw,
        Err(error) => return failure_fallback(ctx, def, &env_key, error),
    };

    if !(200..300).contains(&raw.status) {
        let error = DynamicError {
            name: def.name.clone(),
            kind: DynamicErrorKind::Status,
            message: format!("数据源返回 {}", raw.status),
        };
        return failure_fallback(ctx, def, &env_key, error);
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
            return failure_fallback(ctx, def, &env_key, error);
        }
    };

    // 处理脚本：抽取之后跑。JSONPath 负责「取哪一段」，脚本负责「这一段怎么变成值」。
    let extracted = match super::transform::apply(def, extracted, &raw, source).await {
        Ok(value) => value,
        Err(error) => return failure_fallback(ctx, def, &env_key, error),
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
        // 本次解析不把超大的值写进缓存表；能不能用之前那份过期值顶上，按失败这一刻判。
        return failure_fallback(ctx, def, &env_key, error);
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
    // 进门那一刻就有的覆盖值另算：这次取数只可能是刷新走上来的（`cache::decide` 的 `force`
    // 特意跳过覆盖值），刷新要的就是把**它看见的那一份**手工值换掉，故只有当前行仍是那一份
    // 才允许写；换成了另一份（取数途中写的），同样留住。
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
        // 地址随模块变：端点的 base URL 挂在（环境 × 模块）那一格上，端点被移到别的模块，
        // 这次取数用的就是旧地址下的东西。故与取数开始那一刻的模块再比一次。
        let module_now = match current.source_endpoint_id.as_deref() {
            Some(endpoint_id) => db::nodes::module_id_of(c, endpoint_id)?,
            None => None,
        };
        if module_now != source.module_id {
            return Ok(false);
        }
        if let Some(existing) = db::dynamic_variable_cache::get(c, &def.id, &env_key)? {
            if existing.is_override {
                let replaceable = match &seen_override {
                    Some(seen) => OverrideRef::of(&existing) == *seen,
                    None => !live_override(&existing, crate::domain::now_ms()),
                };
                if !replaceable {
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

/// 进门那一刻那份覆盖值的身份：写回侧据此分辨「还是它看见的那一份吗」。
///
/// 只比对值本身不够：用户可以把同一份值再存一次，保存时刻与到期时刻都变了，那就是新的一份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct OverrideRef {
    value_json: Option<String>,
    fetched_at: i64,
    override_expires_at: Option<i64>,
}

impl OverrideRef {
    fn of(entry: &DynamicCacheEntry) -> OverrideRef {
        OverrideRef {
            value_json: entry.value_json.clone(),
            fetched_at: entry.fetched_at,
            override_expires_at: entry.override_expires_at,
        }
    }
}

/// 这一行是**仍然生效**的覆盖值吗：人手写的，且没有到期或还没到。
///
/// 「仍然生效」按调用方给的时刻算，故取数开始与写回两处可以用不同的时刻各判一次。
fn live_override(entry: &DynamicCacheEntry, now: i64) -> bool {
    entry.is_override && !super::cache::is_expired(entry, now)
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

/// 取数失败（或值不可用）时的退化：能拿一份过期值顶上就用它并标「过期值」，否则按缺失处理。
///
/// 能不能顶上按**失败这一刻**判：重读这一格缓存行再判，而不是沿用取数开始时的结论。一次
/// 取数可能跨过那一行的过期点——开始还新鲜、失败时已经过期，那正是它该顶上的时刻；反过来，
/// 开始就过期、中途被写成新鲜值的一行，此刻不该再当回退值。判活的时刻取在读到之后。
fn failure_fallback(
    ctx: &ResolveContext,
    def: &DynamicVariable,
    env_key: &str,
    error: DynamicError,
) -> Result<OneOutcome, AppError> {
    let entry = db::with_conn(&ctx.pool, |c| {
        db::dynamic_variable_cache::get(c, &def.id, env_key)
    })?;
    let now = crate::domain::now_ms();
    let Some(stale) = entry
        .as_ref()
        .and_then(|entry| super::cache::fallback_value(def, entry, now))
    else {
        return Ok(OneOutcome::missing(Some(error)));
    };
    let value = serialize_value(&parse_raw(&stale), def.value_mode);
    Ok(OneOutcome {
        value: Some(value),
        source: DynamicValueSource::Stale,
        fetched_at: None,
        expires_at: None,
        is_override: false,
        error: Some(error),
    })
}
