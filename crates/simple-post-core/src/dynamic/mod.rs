//! 动态变量的解析：取数、抽取、缓存命中、single-flight。
//!
//! 一条动态变量的声明是「变量名 → 数据源 → 抽取规则 → 缓存时长」。消费端点的请求体里写
//! `{{sns}}`，发送时程序按本模块解析出值，并进变量池。
//!
//! ## 为什么解析在「替换」之前
//!
//! 变量池的合并规则有两条子规则：动态变量的值优先于同名环境变量；被停用的环境变量不影响
//! 动态变量。这两条只有在替换之前并池才成立——替换一旦发生，池里已经没有同名变量的概念了。
//! 故本模块的产出是**一个变量名到值的映射**，由调用方在替换之前并进池。
//!
//! ## 分层
//!
//! 本模块住在 core，因为它只需要 core 的 db / http_executor / template。但「引用端点」形态的
//! 数据源要用 `variable_fill::resolve_endpoint` 组装请求，那个函数住在 mcp crate，而 core
//! 不能依赖 mcp。故本模块定义 [`SourceFetcher`] 抽象，由 mcp crate 提供实现——
//! 两侧 transport 共用同一份取数逻辑。
//!
//! ## 取数失败不让整次发送失败
//!
//! 这与既有的取舍一致：脚本失败不影响请求的成败（见 `crate::script::send_with_scripts`）。
//! 请求发出去，服务端多半会返回一个明确的业务错误，那比客户端提前放弃更有信息量。
//! 处理顺序：覆盖值 → 过期缓存值（仅当 `stale_fallback` 为真） → 按缺失处理。

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde_json::Value;

use crate::db;
use crate::domain::{
    AuthConfig, AuthType, DynamicCacheEntry, DynamicError, DynamicErrorKind, DynamicResolution,
    DynamicValueSource, DynamicValueStatus, DynamicVariable, Environment, KeyValue, Project,
    RequestBody, ValueMode,
};
use crate::error::AppError;
use crate::template::list_missing_vars;

/// 一个值的体积上限。超过它记一条错误，而不是把超大文本塞进缓存表。
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

/// 解析的上下文。一次解析里的所有动态变量共享它。
pub struct ResolveContext {
    pub pool: db::DbPool,
    pub project_id: String,
    /// 当前环境。`None` = 没有环境（缓存键用空串占位）。
    pub environment_id: Option<String>,
    /// 当前环境的内容。数据源端点用它解析 base URL、鉴权与变量替换。
    pub env: Option<Environment>,
    /// 当前项目。数据源端点用它解析 ★ 服务与模块的服务列表。
    pub project: Project,
}

/// 数据源取回来的原始响应。
pub struct RawSource {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

/// 取数器：把一条定义的数据源变成一次真实的 HTTP 响应。
///
/// 实现住在 mcp crate（那里有 `variable_fill::resolve_endpoint`）。core 定义这个抽象，是为了让
/// 引擎不必依赖上层 crate。
pub trait SourceFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        ctx: &'a ResolveContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>>;
}

// -------------------------------------------------------------------------
// 变量池的合并规则
// -------------------------------------------------------------------------

/// 合成变量池：环境变量（去掉停用的那些）+ 动态变量的值。
///
/// 两条子规则都由「动态变量放在停用过滤之后并入」这一条实现方式成立：
///
/// 1. 动态变量的值覆盖同名环境变量——它是后面并进去的。
/// 2. 被停用的环境变量不影响动态变量——停用过滤只作用于环境变量，停用名单里的同名动态
///    变量照常生效。
pub fn merge_pool(
    env_vars: &HashMap<String, String>,
    disabled: &[String],
    dynamic: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut pool: HashMap<String, String> = env_vars
        .iter()
        .filter(|(k, _)| !disabled.iter().any(|d| d == *k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in dynamic {
        pool.insert(k.clone(), v.clone());
    }
    pool
}

// -------------------------------------------------------------------------
// 值的序列化
// -------------------------------------------------------------------------

/// 把抽到的值变成替换用的文本。
///
/// - `json`：始终是 JSON 序列化，故标量带引号（`"A001"`）。
/// - `auto` 与 `text`：对象与数组给裸 JSON 片段，标量给纯文本。两者输出**完全一致**——
///   `text` 是用户的声明（值会被放进请求体的字符串位置，转义由用户承担），不是另一种形态。
///
/// 替换环节不做任何转义。
pub fn serialize_value(value: &Value, mode: ValueMode) -> String {
    match mode {
        ValueMode::Json => serde_json::to_string(value).unwrap_or_default(),
        ValueMode::Auto | ValueMode::Text => match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            other => serde_json::to_string(other).unwrap_or_default(),
        },
    }
}

/// 覆盖值是用户手打的文本：能解析成 JSON 就用它，解析不了就当成一个字符串。
///
/// 这样用户写 `A001` 得到 `A001`，写 `[{"code":"A001"}]` 得到数组。
pub fn parse_raw(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

// -------------------------------------------------------------------------
// 引用的名字
// -------------------------------------------------------------------------

/// 请求模板的各段。扫描「哪些 `{{name}}` 被引用了」只需要这些。
pub struct TemplateParts<'a> {
    /// base URL。它可能自身带变量，故与 path 一起扫。
    pub base_url: &'a str,
    /// 端点的 path。
    pub url: &'a str,
    pub params: &'a [KeyValue],
    pub headers: &'a [KeyValue],
    pub body: &'a RequestBody,
    pub auth: &'a AuthConfig,
}

/// 模板里出现的全部 `{{name}}` 名字，按首次出现排序。
///
/// 只扫**会被替换**的字段：启用的且 key 非空的参数值、启用的且 key 非空的 header 值、body
/// 内容、以及鉴权里那三种方案（bearer / basic / apikey）。OAuth2 的配置字段不被变量替换，
/// 故不在扫描范围内——扫了会让一条根本没被用到的定义去取数。
///
/// 未启用的参数与 header 里的变量不算被引用：它们不会发出，其中的变量不是这次请求的问题。
pub fn list_names(parts: &TemplateParts<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |text: &str| {
        for name in list_missing_vars(text) {
            if seen.insert(name.clone()) {
                out.push(name);
            }
        }
    };

    add(parts.base_url);
    add(parts.url);
    for p in parts.params {
        if p.enabled && !p.key.is_empty() {
            add(&p.value);
        }
    }
    for h in parts.headers {
        if h.enabled && !h.key.is_empty() {
            add(&h.value);
        }
    }
    if !parts.body.content.is_empty() {
        add(&parts.body.content);
    }
    match parts.auth.kind {
        AuthType::Bearer => {
            if let Some(bearer) = &parts.auth.bearer {
                add(bearer);
            }
        }
        AuthType::Basic => {
            if let Some(basic) = &parts.auth.basic {
                add(&basic.username);
                add(&basic.password);
            }
        }
        AuthType::Apikey => {
            if let Some(apikey) = &parts.auth.apikey {
                add(&apikey.key);
                add(&apikey.value);
            }
        }
        // OAuth2 的配置字段不参与变量替换，故不扫；`None` 也不带任何值。
        AuthType::None | AuthType::Oauth2 => {}
    }
    out
}

// -------------------------------------------------------------------------
// 缓存决策
// -------------------------------------------------------------------------

/// 一次解析该走哪条路。
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// 用覆盖值（用户手动改的）。
    UseOverride { value_json: String },
    /// 用缓存值（自动取回来的，还在有效期内）。
    UseCache { value_json: String },
    /// 重新取数。`stale_fallback` 是取数失败时可以用的过期值。
    Fetch { stale_fallback: Option<String> },
}

/// 决定这一次解析走哪条路。
///
/// | 情形 | 决定 |
/// |---|---|
/// | 有覆盖值且没过期 | 用覆盖值 |
/// | 有覆盖值但已过期 | 重新取数（覆盖值过期不等于取数失败，不走 `stale_fallback`） |
/// | 无覆盖值，`force` 为真 | 重新取数 |
/// | 无覆盖值，缓存没过期 | 用缓存值 |
/// | 无覆盖值，缓存过期了 | 重新取数；过期值留作取数失败时的回退候选 |
/// | 没有缓存行 | 重新取数 |
pub fn decide(
    def: &DynamicVariable,
    entry: Option<&DynamicCacheEntry>,
    now: i64,
    force: bool,
) -> Decision {
    let Some(entry) = entry else {
        return Decision::Fetch {
            stale_fallback: None,
        };
    };
    if entry.is_override {
        let alive = entry.override_expires_at.map(|t| t > now).unwrap_or(true);
        if alive {
            if let Some(value_json) = &entry.value_json {
                return Decision::UseOverride {
                    value_json: value_json.clone(),
                };
            }
        }
        return Decision::Fetch {
            stale_fallback: None,
        };
    }
    if !force && entry.expires_at > now {
        if let Some(value_json) = &entry.value_json {
            return Decision::UseCache {
                value_json: value_json.clone(),
            };
        }
    }
    let stale_fallback = if def.stale_fallback {
        entry.value_json.clone()
    } else {
        None
    };
    Decision::Fetch { stale_fallback }
}

// -------------------------------------------------------------------------
// 抽取
// -------------------------------------------------------------------------

/// 按 JSONPath 从响应体里抽一个值。
///
/// 响应体必须能解析成 JSON（否则是 `not_json`）；路径没命中是 `no_match`；路径表达式本身
/// 非法是 `source_error`——那是定义坏了，不是数据的问题。
pub fn extract_value(body: &str, extract_path: &str) -> Result<Value, DynamicErrorKind> {
    let value: Value = serde_json::from_str(body).map_err(|_| DynamicErrorKind::NotJson)?;
    let path = if extract_path.trim().is_empty() {
        "$"
    } else {
        extract_path.trim()
    };
    let query =
        serde_json_path::JsonPath::parse(path).map_err(|_| DynamicErrorKind::SourceError)?;
    match query.query(&value).first() {
        Some(node) => Ok((*node).clone()),
        None => Err(DynamicErrorKind::NoMatch),
    }
}

// -------------------------------------------------------------------------
// 编排
// -------------------------------------------------------------------------

/// 解析一批名字，返回它们的值、状态与错误。
///
/// 名字里没有对应定义的（那是环境变量，或者是拼错的名字）直接跳过——它们由变量替换那一步
/// 按缺失处理。停用的定义也跳过：`{{name}}` 原样留着并进未解析变量列表。
pub async fn resolve_names(
    ctx: &ResolveContext,
    names: &[String],
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<DynamicResolution, AppError> {
    let now = crate::domain::now_ms();
    let mut resolution = DynamicResolution::default();

    for name in names {
        let Some(def) = db::with_conn(&ctx.pool, |c| {
            db::dynamic_variables::get_by_name(c, &ctx.project_id, name)
        })?
        else {
            continue;
        };
        if !def.enabled {
            continue;
        }
        let outcome = resolve_one(ctx, &def, now, force, fetcher).await?;
        if let Some(value) = &outcome.value {
            resolution.values.insert(name.clone(), value.clone());
        }
        if let Some(error) = &outcome.error {
            resolution.errors.push(error.clone());
        }
        resolution.statuses.push(DynamicValueStatus {
            name: name.clone(),
            value: outcome.value,
            source: outcome.source,
            fetched_at: outcome.fetched_at,
            expires_at: outcome.expires_at,
            is_override: outcome.is_override,
            error: outcome.error,
        });
    }

    Ok(resolution)
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
    now: i64,
    force: bool,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    let env_key = ctx.environment_id.clone().unwrap_or_default();
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
            fetch_one(ctx, def, now, force, env_key, stale_fallback, fetcher).await
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
    now: i64,
    force: bool,
    env_key: String,
    stale_fallback: Option<String>,
    fetcher: &dyn SourceFetcher,
) -> Result<OneOutcome, AppError> {
    let lock = flight_lock(&(def.id.clone(), env_key.clone()));
    let _guard = lock.lock().await;

    // 重新查一次缓存：等锁期间别人可能已经取好并落库了。
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

    let raw = match fetcher.fetch(def, ctx).await {
        Ok(raw) => raw,
        Err(error) => {
            // 取数失败：先看能不能用过期值兜底。
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
        // 超大的值不写库，但已经取回来了——回退候选仍然可用。
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

/// 取数失败（或值不可用）时的降级：有回退候选就用它并标「过期值」，否则按缺失处理。
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

// -------------------------------------------------------------------------
// single-flight
// -------------------------------------------------------------------------

/// single-flight 的互斥量表：键是（定义 × 环境），值是那个键的互斥量。
///
/// 条目用 `Weak` 持有：没人等它时它自然消失，map 不会无上限增长。
type FlightMap = HashMap<(String, String), Weak<tokio::sync::Mutex<()>>>;

/// 同一个（定义 × 环境）的互斥量。
fn flight_lock(key: &(String, String)) -> Arc<tokio::sync::Mutex<()>> {
    static MAP: OnceLock<Mutex<FlightMap>> = OnceLock::new();
    let map = MAP.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|_, weak| weak.strong_count() > 0);
    if let Some(existing) = guard.get(key).and_then(|weak| weak.upgrade()) {
        return existing;
    }
    let fresh = Arc::new(tokio::sync::Mutex::new(()));
    guard.insert(key.clone(), Arc::downgrade(&fresh));
    fresh
}

// 测试。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "resolve_parity_test.rs"]
mod parity_tests;
