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
    if force {
        // `force` 为真时：跳过缓存里的**一切**——包括用户手动指定的值——重新取数。
        // 一个叫「刷新」却什么都不做的按钮比它丢掉的那份手动值更糟；丢掉的动作是可见的
        // （来源徽标从「手动改的」变回「自动取回」），且用户可以再写一次。
        return Decision::Fetch {
            stale_fallback: stale_candidate(def, entry),
        };
    }
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
            stale_fallback: stale_candidate(def, entry),
        };
    }
    if !force && entry.expires_at > now {
        if let Some(value_json) = &entry.value_json {
            return Decision::UseCache {
                value_json: value_json.clone(),
            };
        }
    }
    Decision::Fetch {
        stale_fallback: stale_candidate(def, entry),
    }
}

/// 过期值的回退候选：定义允许时才给。
fn stale_candidate(def: &DynamicVariable, entry: &DynamicCacheEntry) -> Option<String> {
    if !def.stale_fallback {
        return None;
    }
    entry.value_json.clone()
}
