//! 按内容嗅探外来格式，并把外来文档翻成 IR。
//!
//! 嗅探只看**顶层键名**：用户从文件选择器里点进来的东西可能是三种形状（Postman 集合、
//! OpenAPI 文档、或者一个随手存成 `.json` 的数组），而「猜错了」的错误信息比「不认识的
//! 格式」有用得多——所以宁可多走一步完整解析，也不要在这里下结论。
//!
//! 顶层键从**已解析的值**上取，不从文本上找：单行压缩过的 JSON（CDN 上的 spec、
//! `JSON.stringify` 的产物）没有行首可言，按行首匹配会把它整个认不出来。

use simple_post_core::domain::Environment;
use simple_post_core::error::AppError;

use crate::ir::Exchange;

pub mod openapi;
pub mod postman;

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

    /// 用户看得懂的格式名。线上名带版本号与连字符，是给程序用的。
    pub fn label(&self) -> &'static str {
        match self {
            Format::PostmanV2_1 => "Postman 集合 v2.1",
            Format::OpenApi3 => "OpenAPI 3.0",
        }
    }
}

/// 一次解析的全部产物。
///
/// 环境**不**进 [`Exchange`]：它不是资源树的一层，而是项目级的另一张表。把它挂在这里
/// 是为了让落库方能在**同一个事务**里把树与环境一起写进去——半个导入比不导入更难收拾。
#[derive(Debug, Clone, Default)]
pub struct ParsedExchange {
    pub exchange: Exchange,
    /// 仅 Postman 有值。一个源环境 → 一个新环境，变量进 `variables`。
    pub environments: Vec<Environment>,
}

/// 嗅探一份上传上来的内容。
///
/// 判据是**顶层键名**。JSON 走真解析后取顶层键——这是唯一能证明「顶层」的东西，
/// 也让压缩过的 JSON 认得出来。JSON 解析失败（多半是 YAML）才回落到按行首找键。
pub fn sniff(text: &str) -> Result<Format, AppError> {
    let body = strip_bom(text);

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        return format_of(&value);
    }

    if let Some(format) = format_of_yaml_text(body) {
        return Ok(format);
    }

    Err(unknown_format())
}

/// 嗅探并翻成 IR。**只解析一次**——大文档（URL 模式拉几 MB 的 spec）解析两遍是白花的 CPU。
///
/// 连格式一起返回：调用方要报「这是份什么格式」，再单独嗅探一次就是第二遍解析。
pub fn sniff_and_parse(text: &str) -> Result<(Format, ParsedExchange), AppError> {
    let body = strip_bom(text);

    // JSON：顶层键从解析结果上取，格式与内容共用同一次解析。
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        let format = format_of(&value)?;
        return Ok((format, parse(format, &value)?));
    }

    // YAML：交给 `parse_text` 走 YAML 那条路，它自己解析一次。
    let format = format_of_yaml_text(body).ok_or_else(unknown_format)?;
    Ok((format, parse_text(format, text)?))
}

/// 从文本进来，先定格式再翻成 IR。
///
/// JSON 走 `serde_json`、YAML 走 `serde_norway`，两者得到**同一种** `Value`。这不是
/// 取巧：外来格式的键本来就都是字符串，故 YAML 先落成 `Value` 之后，OpenAPI 的 `$ref`
/// 只是 JSON Pointer 走查，不需要任何额外的解析结构。
///
/// Postman 是 JSON-only，但它照样经这条路：它只有 JSON，多一条分支只是让
/// 「两个格式共用一条入口」这件事看起来是刻意的。
pub fn parse_text(format: Format, text: &str) -> Result<ParsedExchange, AppError> {
    let body = strip_bom(text);
    let value: serde_json::Value = if body.trim_start().starts_with('{') {
        serde_json::from_str(body)
            .map_err(|e| AppError::Validation(format!("JSON 解析失败：{e}")))?
    } else {
        serde_norway::from_str(body)
            .map_err(|e| AppError::Validation(format!("YAML 解析失败：{e}")))?
    };
    parse(format, &value)
}

/// 把已经解析成 [`serde_json::Value`] 的外来文档翻成 IR。
pub fn parse(format: Format, value: &serde_json::Value) -> Result<ParsedExchange, AppError> {
    match format {
        Format::PostmanV2_1 => postman::parse(value),
        Format::OpenApi3 => openapi::parse(value),
    }
}

/// 去掉 BOM：Windows 记事本存 UTF-8 会带上它，它会让第一个键名匹配不上。
fn strip_bom(text: &str) -> &str {
    text.trim_start_matches('\u{feff}')
}

/// 已解析的值 → 格式。键必须**在顶层**。
fn format_of(value: &serde_json::Value) -> Result<Format, AppError> {
    let Some(map) = value.as_object() else {
        return Err(unknown_format());
    };
    if map.contains_key("item") && map.contains_key("info") {
        return Ok(Format::PostmanV2_1);
    }
    if map.contains_key("openapi") && map.contains_key("paths") {
        return Ok(Format::OpenApi3);
    }
    Err(unknown_format())
}

/// YAML 文本 → 格式。**只看行首**：YAML 的顶层键在自己的行首，缩进的键属于嵌套对象。
///
/// 只服务 YAML：JSON 走真解析后取顶层键，不靠文本匹配。
fn format_of_yaml_text(text: &str) -> Option<Format> {
    if has_top_level_key(text, "item") && has_top_level_key(text, "info") {
        return Some(Format::PostmanV2_1);
    }
    if has_top_level_key(text, "openapi") && has_top_level_key(text, "paths") {
        return Some(Format::OpenApi3);
    }
    None
}

/// 认不出时的错误。两种格式各说各的特征键，用户能自己看出缺了什么。
fn unknown_format() -> AppError {
    AppError::Validation(
        "认不出这是什么格式：既没有 Postman 集合的 item/info，也没有 OpenAPI 文档的 openapi/paths"
            .into(),
    )
}

/// 顶层有没有这个键。**只看行首**：缩进的键属于嵌套对象。
///
/// 只服务 YAML —— JSON 走真解析后取顶层键，不靠文本匹配。
fn has_top_level_key(text: &str, key: &str) -> bool {
    let quoted = format!("\"{key}\"");
    text.lines().any(|line| {
        let l = line.trim_start();
        // YAML: `key:` 或 `"key":`
        l.starts_with(&quoted) || l.starts_with(&format!("{key}:"))
    })
}

// 解析器目前只有两个实现，故这不是 trait。等真出现第三种格式（HAR？Insomnia？）再抽。

// 测试——两种格式的形状与误判防护。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
