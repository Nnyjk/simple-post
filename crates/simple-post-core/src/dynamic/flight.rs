//! single-flight：同一个（定义 × 环境）同时只发一次取数请求。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// single-flight 的互斥量表：键是（定义 × 环境），值是那个键的互斥量。
///
/// 条目用 `Weak` 持有：没人等它时它自然消失，map 不会无上限增长。
type FlightMap = HashMap<(String, String), Weak<tokio::sync::Mutex<()>>>;

/// 同一个（定义 × 环境）的互斥量。
pub(super) fn flight_lock(key: &(String, String)) -> Arc<tokio::sync::Mutex<()>> {
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
