//! 严格的 patch 字段读取器。
//!
//! `endpoint_update` 的每个字段都经这里读，契约是四档：键缺失 → 不动、JSON `null` →
//! 清空、有值 → 写入、格式错 → `Validation`。第四档承重。

use serde_json::Value;

use super::input::{AUTH_TYPE_VALUES, BODY_MODE_VALUES};
use simple_post_core::error::AppError;
// =========================================================================
// 严格的 patch 字段读取器
// =========================================================================
//
// 每个读取器都保持的契约：
//
//   * 键缺失          → `None` → 该列不动
//   * 显式 JSON `null` → `Some(None)` → 该列清空（仅 `double_option` 字段）
//   * 有值且格式正确   → `Some(Some(v))` → 写入
//   * 有值但格式错误   → `Err(AppError::Validation)`，点名字段与期望形状
//
// 关键是第四行。`.ok()` 会把「字段缺失」与「字段格式错」压成同一个结果，于是一个
// 写坏的 patch 会返回 `is_error: false` 却什么都没写 —— agent 无法区分「改成功」与
// 「被丢掉」。
//
// 每一行的 `?` 都意味着格式错误会在开事务**之前**中止整个调用，于是 patch 绝不会
// 只应用一半：要么点名的字段全写进去，要么一个不写。

/// 附在「字段格式错」错误后的可读形状提示，逐字段写明 —— agent 下一步就是重发
/// patch，不该再去猜哪个嵌套键写错了。
fn patch_shape_hint(field: &str) -> String {
    match field {
        "method" => "a string HTTP verb, e.g. \"GET\", \"POST\", \"PUT\", \
                     \"DELETE\", \"PATCH\", \"HEAD\", \"OPTIONS\" (or any RFC \
                     9110 token such as \"PROPFIND\")"
            .to_string(),
        "url" => "a string, e.g. \"/api/users/{id}\"".to_string(),
        "params" | "headers" | "path_params" => "an array of objects shaped \
             {\"key\":\"<name>\",\"value\":\"<value>\",\"enabled\":true} \
             — note the field names are `key` and `value`, not `k`/`v`/`name`"
            .to_string(),
        "body" => format!(
            "an object shaped {{\"mode\":\"<{}>\",\"content\":\"<raw body>\",\
             \"schema\":\"<optional>\"}}",
            BODY_MODE_VALUES.join("|")
        ),
        "auth" => format!(
            "an object shaped {{\"kind\":\"<{}>\", ...}} where the rest \
             depends on kind: bearer → {{\"bearer\":\"<token>\"}}, basic → \
             {{\"basic\":{{\"username\":\"u\",\"password\":\"p\"}}}}, apikey → \
             {{\"apikey\":{{\"key\":\"<header name>\",\"value\":\"<key>\",\
             \"inLocation\":\"header\"|\"query\"}}}}, oauth2 → \
             {{\"oauth2\":{{\"grantType\":\"client_credentials\",\
             \"tokenUrl\":\"<url>\",\"clientId\":\"<id>\"}}}}",
            AUTH_TYPE_VALUES.join("|")
        ),
        "tags" => "an array of strings, e.g. [\"smoke\",\"user\"]".to_string(),
        "environment_id"
        | "base_url_definition_id"
        | "base_url_override"
        | "description"
        | "docs"
        | "notes" => "a string, or null to clear the field".to_string(),
        "name" => "a non-empty string".to_string(),
        "sort_order" => "an integer".to_string(),
        _ => String::new(),
    }
}

/// 把某个字段的反序列化失败包成 agent 看得懂的 `Validation` 错误。
fn malformed_field(field: &str, detail: impl std::fmt::Display) -> AppError {
    let hint = patch_shape_hint(field);
    if hint.is_empty() {
        AppError::Validation(format!("patch field '{field}' is malformed: {detail}"))
    } else {
        AppError::Validation(format!(
            "patch field '{field}' is malformed: {detail}. Expected {hint}"
        ))
    }
}

/// 读一个 patch 字段，保持 `double_option` 的三档语义：缺失 → `None`（不动）、
/// `null` → `Some(None)`（清空）、有值 → `Some(Some(v))`。
///
/// 反序列化失败是**错误**，不是静默的「别动」。
pub(super) fn patch_field<T: serde::de::DeserializeOwned>(
    patch_obj: &Value,
    field: &str,
) -> Result<Option<Option<T>>, AppError> {
    let Some(raw) = patch_obj.get(field) else {
        return Ok(None);
    };
    if raw.is_null() {
        // `double_option` 语义：显式 null 是「清空这一列」，一个真实的写入目标，
        // 不是「别动」。
        return Ok(Some(None));
    }
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(v) => Ok(Some(Some(v))),
        Err(e) => Err(malformed_field(field, e)),
    }
}

/// 读一个没有「用 null 清空」语义的 patch 字段 —— 值本身就是载荷（`params`、
/// `headers`、`tags`、`body`、`auth`）。
///
/// 这里的 `null` 是**笔误**而非清空请求：这些列要靠发空数组 / 空 body 才清得掉；
/// 把 `null` 当清空，会让「把缺失值序列化成 null」的客户端悄悄抹掉配好的请求头列表。
pub(super) fn patch_value_field<T: serde::de::DeserializeOwned>(
    patch_obj: &Value,
    field: &str,
) -> Result<Option<T>, AppError> {
    let Some(raw) = patch_obj.get(field) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Err(malformed_field(field, "got null"));
    }
    match serde_json::from_value::<T>(raw.clone()) {
        Ok(v) => Ok(Some(v)),
        Err(e) => Err(malformed_field(field, e)),
    }
}
