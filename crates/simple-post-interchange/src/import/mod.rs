//! 按内容嗅探外来格式。
//!
//! 嗅探只看**顶层键名**，不做完整解析：用户从文件选择器里点进来的东西可能是三种形状
//! （Postman 集合、OpenAPI 文档、或者一个随手存成 `.json` 的数组），而「猜错了」的错误信息
//! 比「不认识的格式」有用得多——所以宁可多走一步完整解析，也不要在这里下结论。
//!
//! 真正的解析在 `postman.rs` / `openapi.rs`（#3 / #4）。本模块只回答「这份内容更像哪一种」。

use simple_post_core::error::AppError;

/// 嗅探到的外来格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Postman Collection v2.1。有 `info` + `item`。
    PostmanV2_1,
    /// OpenAPI 3.x。有 `openapi` + `info` + `paths`。
    OpenApi3,
}

impl Format {
    /// 线上名。用于诊断文本与 UI 的下拉项。
    pub fn as_str(&self) -> &'static str {
        match self {
            Format::PostmanV2_1 => "postman-collection-v2.1",
            Format::OpenApi3 => "openapi-3",
        }
    }
}

/// 嗅探一份上传上来的内容。
///
/// 只认 JSON 与 YAML 的**文本形状**——本 crate 此刻没有 YAML 依赖（那是 #4 的事），
/// 故 YAML 走的是「行首键名」这条正则路径，不经过真正的 YAML 解析器。
pub fn sniff(text: &str) -> Result<Format, AppError> {
    // BOM：Windows 记事本存 UTF-8 会带上，它会让第一个键名匹配不上。
    let body = text.trim_start_matches('\u{feff}');

    if has_top_level_key(body, "item") && has_top_level_key(body, "info") {
        return Ok(Format::PostmanV2_1);
    }
    if has_top_level_key(body, "openapi") && has_top_level_key(body, "paths") {
        return Ok(Format::OpenApi3);
    }

    Err(AppError::Validation(
        "认不出这是什么格式：既没有 Postman 集合的 item/info，也没有 OpenAPI 文档的 openapi/paths"
            .into(),
    ))
}

/// 顶层有没有这个键。
///
/// **只看行首**是有意的：JSON 的键在自己的行首，YAML 的顶层键也在行首（缩进的键属于
/// 嵌套对象）。代价是 `{"a": {"item": 1}}` 这种内层同名键不会误判——而这正是我们要的。
fn has_top_level_key(text: &str, key: &str) -> bool {
    let quoted = format!("\"{key}\"");
    text.lines().any(|line| {
        let l = line.trim_start();
        // YAML: `key:` 或 `"key":`
        l.starts_with(&quoted) || l.starts_with(&format!("{key}:"))
    })
}

// 测试——两种格式的形状与误判防护。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
