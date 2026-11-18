//! 缓存决定：这一次解析走哪条路。

use crate::domain::{DynamicCacheEntry, DynamicVariable};

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
/// | 没有数据源 | 重新取数；自动值既不读也不当回退候选，覆盖值照旧生效 |
/// | `force` 为真 | 重新取数，跳过缓存里的一切（包括手动指定的值） |
/// | 有覆盖值且没过期 | 用覆盖值 |
/// | 有覆盖值但已过期 | 重新取数；过期值留作取数失败时的回退候选（定义允许时） |
/// | 无覆盖值，缓存没过期 | 用缓存值 |
/// | 无覆盖值，缓存过期了 | 重新取数；过期值留作取数失败时的回退候选（定义允许时） |
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
    // 没有数据源的定义取不到值：数据源接口被删掉之后，它过去写下的自动值已经没有数据源
    // 支撑，故既不读它，也不拿它当取数失败的回退候选——按缺失处理。覆盖值是人手写的，
    // 与数据源无关，仍按下面的覆盖值规则生效。
    let has_source = def.has_source();
    if force {
        // `force` 为真时：跳过缓存里的**一切**——包括用户手动指定的值——重新取数。
        // 一个叫「刷新」却什么都不做的按钮比它丢掉的那份手动值更糟；丢掉的动作是可见的
        // （来源徽标从「手动改的」变回「自动取回」），且用户可以再写一次。
        return Decision::Fetch {
            // 回退候选只认**已经过期**的那一行：`force` 不看有效期，一份刚由别的解析写下的
            // 新鲜值也会走到这里，它不是过期值，取数失败时按缺失处理才对。
            // 覆盖值过期后留下的候选仍是人手写的值；自动值只在有数据源时才算数。
            stale_fallback: if (has_source || entry.is_override) && is_expired(entry, now) {
                stale_candidate(def, entry)
            } else {
                None
            },
        };
    }
    if entry.is_override {
        let alive = !is_expired(entry, now);
        if alive {
            if let Some(value_json) = &entry.value_json {
                return Decision::UseOverride {
                    value_json: value_json.clone(),
                };
            }
        }
        return Decision::Fetch {
            stale_fallback: stale_candidate(def, entry),
        };
    }
    if has_source && entry.expires_at > now {
        if let Some(value_json) = &entry.value_json {
            return Decision::UseCache {
                value_json: value_json.clone(),
            };
        }
    }
    Decision::Fetch {
        stale_fallback: if has_source {
            stale_candidate(def, entry)
        } else {
            None
        },
    }
}

/// 这一行过期了吗：覆盖值看到期时刻（没写到期时刻的永久覆盖值永不过期），自动值看 TTL。
///
/// 判活的时刻由调用方给，故调用方要在读到那一行**之后**再取时刻。
pub(super) fn is_expired(entry: &DynamicCacheEntry, now: i64) -> bool {
    if entry.is_override {
        entry
            .override_expires_at
            .map(|deadline| deadline <= now)
            .unwrap_or(false)
    } else {
        entry.expires_at <= now
    }
}

/// 过期值的回退候选：定义允许时才给。
fn stale_candidate(def: &DynamicVariable, entry: &DynamicCacheEntry) -> Option<String> {
    if !def.stale_fallback {
        return None;
    }
    entry.value_json.clone()
}
