//! 跨模块共用的辅助函数：时间戳、主键、颜色默认值与 patch 的三态解码。

use serde::{Deserialize, Serialize};

/// 当前 unix 毫秒。写路径上所有 `*_at` 列的唯一真相，使单个事务内的值自洽。
/// `SystemTime::now()` 只在时钟早于 unix epoch 时 panic；把这当程序员错误，而非用户可恢复的故障。
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

/// 调用方未给项目颜色时的默认值。住在这里而非 `commands/`，是为了让 REST 与 Tauri 两条写路径
/// 无法分叉：`commands` 已依赖 `domain`，在那条边另一侧再放一份只会自由地各说各话。
pub fn default_project_color() -> String {
    "#3b82f6".to_string()
}

/// 新主键：`sp_` + 墙钟十六进制 + 每进程计数器，拼接而成。
///
/// 它**不是**不透明、也不可猜测——它编码了生成时刻，这对单用户桌面应用可以接受（其数据库
/// 本就被该用户可读），此处点明以免有人把它用在需要更多保证的场景。计数器才是唯一性所在：
/// 否则同一纳秒内生成的两个键会相撞，而在一个时钟节拍内重启的进程会重复前缀。随机后缀可
/// 去掉这两条隐患，但库仍是本地的，不值得多一个依赖。
pub fn uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sp_{:x}{:x}", nanos, n)
}

/// 在 `Option<T>` 上把 JSON 缺席与 `null` 解析成**不同**状态：
///   - 缺席     → `None` 外层（`patch.field` 未设 → 不碰该列）
///   - `null`   → `Some(None)`       → 向该列写 NULL
///   - `"foo"`  → `Some(Some("foo"))` → 写 "foo"
///
/// serde 内建的 `Option<T>` 把缺席与 `null` 折叠成一回事，这会毁掉 patch 结构体里
/// 「清空该列」与「保持不动」的区分。本包装器把它还原。
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

/// `value` 是否不含任何会上线的字段。
///
/// 「有没有东西要写」由「能否序列化成空对象」回答，而不是自己再列一遍字段：每个 patch 字段
/// 都带 `skip_serializing_if = "Option::is_none"`，故该属性**就是**「缺席即不碰此列」的编码，
/// 判定从它推导出来就只有一处真相，新增字段不会与手写字段表失配。三态情形随之自动成立：
/// `Some(None)`（清空该列）序列化为显式 `null`，键存在，patch 正确地*不*算空。
///
/// 这些类型不会序列化失败（字段只有标量、`Option`、`Vec`，或本身 `Serialize` 的嵌套结构），
/// 但序列化失败时返回 `false` 而非 `true` 是刻意的：偏向「非空」意味着意外失败会让写入照常发生、
/// 真实错误在 DB 层暴露，而不是被吞成一次静默的空操作。
pub(crate) fn serialises_to_empty_object<T: Serialize>(value: &T) -> bool {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_object().map(|m| m.is_empty()))
        .unwrap_or(false)
}
