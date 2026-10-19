//! 按内容嗅探外来格式，并把外来文档翻成 IR。
//!
//! 嗅探只看**顶层键名**，不做完整解析：用户从文件选择器里点进来的东西可能是三种形状
//! （Postman 集合、OpenAPI 文档、或者一个随手存成 `.json` 的数组），而「猜错了」的错误信息
//! 比「不认识的格式」有用得多——所以宁可多走一步完整解析，也不要在这里下结论。

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

/// 从文本进来，先定格式再翻成 IR。
///
/// JSON 走 `serde_json`、YAML 走 `serde_norway`，两者得到**同一种** `Value`。这不是
/// 取巧：外来格式的键本来就都是字符串，故 YAML 先落成 `Value` 之后，OpenAPI 的 `$ref`
/// 只是 JSON Pointer 走查，不需要任何额外的解析结构。
///
/// Postman 是 JSON-only，但它照样经这条路：它只有 JSON，多一条分支只是让
/// 「两个格式共用一条入口」这件事看起来是刻意的。
pub fn parse_text(format: Format, text: &str) -> Result<ParsedExchange, AppError> {
    let body = text.trim_start_matches('\u{feff}');
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

/// 从 host 推服务名。端口与 scheme 不算名字的一部分；带变量的 host 回落成 `default`。
///
/// 两种外来格式的 host 形状不同——Postman 的 `host` 字段不带 scheme，OpenAPI 的
/// `servers[].url` 带——所以先剥 scheme 再判，两侧共用这一份。
pub(crate) fn service_name(host: &str) -> String {
    let without_scheme = match host.find("://") {
        Some(at) => &host[at + 3..],
        None => host,
    };
    let bare = without_scheme.split(':').next().unwrap_or(without_scheme);
    // `{` 一次收口两种写法：OpenAPI 的 `{var}` 与 Postman 的 `{{var}}`。
    if bare.is_empty() || bare.contains('{') || bare.contains('/') {
        "default".to_string()
    } else {
        bare.to_string()
    }
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

// 解析器目前只有两个实现，故这不是 trait。等真出现第三种格式（HAR？Insomnia？）再抽。

// 测试——两种格式的形状与误判防护。

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
