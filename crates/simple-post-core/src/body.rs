//! 请求体的解析与装配 —— 渲染进程与 MCP 端共用的那套规则。
//!
//! ## 一个请求体有七种形态
//!
//! [`BodyMode`] 的每个变体在 [`RequestBody::content`] 里装的东西都不一样：
//!
//! | 模式 | `content` 装的是什么 | 产物 |
//! |---|---|---|
//! | `none` | 空 | 没有 body |
//! | `json` / `xml` / `raw` | 原文 | 文本 |
//! | `form` | 行数组的 JSON 文本 | 文本（urlencoded） |
//! | `multipart` | 分片数组的 JSON 文本 | 字节 |
//! | `binary` | 文件描述的 JSON 文本 | 字节（文件本身） |
//!
//! `form` 与 `multipart` 是**结构化**的：它们的 `content` 是一段 JSON 文本，描述的是
//! 一行一行 / 一个一个分片，而不是拼好的正文。结构化带来两样东西：界面上的表格有地方
//! 放说明，文件分片有地方放字节。
//!
//! ## 模板化按字段做，不按整段做
//!
//! `json` / `xml` / `raw` 整段替换；`form` 与 `multipart` **逐个字段**替换。整段替换在
//! 结构化形态上是错的：替换出来的值里带一个引号就会把那段 JSON 文本弄坏，于是整份内容
//! 解析不出来，请求发出去的是一个空 body。
//!
//! ## 产物的两种形态
//!
//! 产物是**文本**还是**二进制**，由字节决定：字节是合法 UTF-8 就走文本形态，否则走
//! base64 形态。`binary` 模式一律走二进制形态——用户选的是文件，发的是它的字节。
//!
//! 渲染进程侧有同一套规则（`src/lib/body/`），两边由 `fixtures/body-parity.json` 钉住。

use std::collections::HashMap;

use crate::domain::{BodyFile, BodyMode, FormField, MultipartPart, RequestBody};
use crate::template::resolve_template;
use crate::url::percent_encode;

// -------------------------------------------------------------------------
// 产物
// -------------------------------------------------------------------------

/// 解析后的请求体。
///
/// 文本形态与二进制形态**互斥**：有文本形态就没有二进制形态，反之亦然。两者都缺席 = 没有
/// 请求体。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedBody {
    /// 文本形态。
    pub text: Option<String>,
    /// 二进制形态（base64）。
    pub base64: Option<String>,
    /// 没解析出来的变量名（去重，按首次出现排序）。
    pub missing: Vec<String>,
}

impl ResolvedBody {
    /// 没有请求体。
    fn empty() -> Self {
        Self::default()
    }
}

// -------------------------------------------------------------------------
// 按模式解析
// -------------------------------------------------------------------------

/// 按 [`RequestBody::mode`] 解析请求体。
///
/// `vars` 是这一次的变量池。`json` / `xml` / `raw` 整段替换，`form` 与 `multipart`
/// 逐个字段替换，`binary` 不替换（文件字节不是模板）。
pub fn resolve_body(body: &RequestBody, vars: &HashMap<String, String>) -> ResolvedBody {
    if body.mode == BodyMode::None || body.content.is_empty() {
        return ResolvedBody::empty();
    }

    match body.mode {
        BodyMode::None => ResolvedBody::empty(),
        BodyMode::Json | BodyMode::Xml | BodyMode::Raw => {
            let r = resolve_template(&body.content, vars);
            ResolvedBody {
                text: Some(r.resolved),
                base64: None,
                missing: r.missing,
            }
        }
        BodyMode::Form => resolve_form_body(&body.content, vars),
        BodyMode::Multipart => resolve_multipart_body(&body.content, vars),
        BodyMode::Binary => resolve_binary_body(&body.content),
    }
}

/// `form` 模式：逐行模板化，然后拼成 urlencoded 正文。
///
/// **空键的行不进正文**，其中的变量也不报缺失——与查询参数同一条规则：没有名字的字段
/// 没有地方可去。
fn resolve_form_body(content: &str, vars: &HashMap<String, String>) -> ResolvedBody {
    let fields = parse_form_fields(content);
    let mut missing: Vec<String> = Vec::new();
    let mut pairs: Vec<String> = Vec::with_capacity(fields.len());

    for field in &fields {
        if !field.enabled || field.key.is_empty() {
            continue;
        }
        let key = resolve_template(&field.key, vars);
        let value = resolve_template(&field.value, vars);
        missing.extend(key.missing);
        missing.extend(value.missing);
        pairs.push(format!(
            "{}={}",
            percent_encode(&key.resolved),
            percent_encode(&value.resolved)
        ));
    }

    ResolvedBody {
        text: Some(pairs.join("&")),
        base64: None,
        missing: dedup(missing),
    }
}

/// `multipart` 模式：逐个分片模板化，然后重新序列化成分片数组。
///
/// 装配（boundary、分片头、字节）在发送侧做：boundary 每次现生成，装配与 Content-Type
/// 必须用同一个值。
fn resolve_multipart_body(content: &str, vars: &HashMap<String, String>) -> ResolvedBody {
    let mut parts = parse_multipart_parts(content);
    let mut missing: Vec<String> = Vec::new();

    for part in parts.iter_mut() {
        // 文件分片的字节不模板化：base64 里没有 `{{`，模板化它只会平白多扫一遍。
        // 分片名也不模板化——它与文件名一起描述「哪个文件」，不是用户填的值。
        if part.base64.is_some() {
            continue;
        }
        let name = resolve_template(&part.name, vars);
        missing.extend(name.missing);
        part.name = name.resolved;
        let r = resolve_template(&part.value, vars);
        missing.extend(r.missing);
        part.value = r.resolved;
    }

    ResolvedBody {
        text: Some(serde_json::to_string(&parts).unwrap_or_default()),
        base64: None,
        missing: dedup(missing),
    }
}

/// `binary` 模式：文件描述里的字节原样带出去。
///
/// 文件描述读不出来（内容不是那份 JSON）时没有 body：发一个空 body 好过发一段被改写
/// 过的文本。
fn resolve_binary_body(content: &str) -> ResolvedBody {
    match parse_body_file(content) {
        Some(file) => ResolvedBody {
            text: None,
            base64: Some(file.base64),
            missing: Vec::new(),
        },
        None => ResolvedBody::empty(),
    }
}

// -------------------------------------------------------------------------
// 解析：form 的行
// -------------------------------------------------------------------------

/// `form` 模式的内容 → 行。
///
/// 内容是行数组的 JSON 文本。**解析不出来时给空数组**，不报错：这是一个纯函数，两条发送
/// 路径在这一点上必须同形。
///
/// 任一行的字段类型不对就整份作废，与 multipart 分片的规则一致：逐行各自取值会让同一个
/// 畸形输入在两侧得到不同的正文。
///
/// **不是行数组时按旧格式读**：早期版本的 `form` 模式存的是一个拼好的 urlencoded 串
/// （Postman 导入器至今还那么写）。那份内容不该被当成空表单——那等于把用户已经写好的
/// 字段悄悄丢掉。
///
/// 「是数组但有一行不对」与「不是数组」是两种不同的情况，故分成两步判：前者整份作废，
/// 后者按旧格式读。
pub fn parse_form_fields(content: &str) -> Vec<FormField> {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(serde_json::Value::Array(_)) => {
            serde_json::from_str::<Vec<FormField>>(content).unwrap_or_default()
        }
        _ => parse_legacy_form_fields(content),
    }
}

/// 旧格式的 `form` 内容（urlencoded 串）→ 行。
///
/// 按 `&` 分行，每行按**第一个** `=` 分成键与值。值不做百分号解码：那份内容里存的是
/// 用户写下的原文，解码会把值里的 `%` 变成别的字符。
pub fn parse_legacy_form_fields(content: &str) -> Vec<FormField> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => FormField {
                key: key.to_string(),
                value: value.to_string(),
                enabled: true,
                description: None,
            },
            None => FormField {
                key: pair.to_string(),
                value: String::new(),
                enabled: true,
                description: None,
            },
        })
        .collect()
}

/// 行 → 行数组的 JSON 文本。
pub fn serialize_form_fields(fields: &[FormField]) -> String {
    serde_json::to_string(fields).unwrap_or_default()
}

// -------------------------------------------------------------------------
// 解析：multipart 的分片
// -------------------------------------------------------------------------

/// `multipart` 模式的内容 → 分片。
///
/// **解析不出来时给空数组**，不报错——与渲染进程侧 `src/lib/multipart.ts` 的
/// `parseMultipartParts` 同一条规则。
///
/// 任一分片的字段类型不对就整份作废，与 serde 反序列化失败后丢弃整份一致。
pub fn parse_multipart_parts(content: &str) -> Vec<MultipartPart> {
    serde_json::from_str::<Vec<MultipartPart>>(content).unwrap_or_default()
}

// -------------------------------------------------------------------------
// 解析：binary 的文件
// -------------------------------------------------------------------------

/// `binary` 模式的内容 → 文件描述。
///
/// 读不出来时给 `None`：那个内容不是文件描述，就没有文件。
pub fn parse_body_file(content: &str) -> Option<BodyFile> {
    serde_json::from_str::<BodyFile>(content).ok()
}

/// `binary` 模式请求体的 Content-Type：文件自己的 MIME 类型，缺省
/// `application/octet-stream`。
///
/// 读不出文件描述时给缺省值：没有文件也就没有类型，而 `application/octet-stream`
/// 是任何服务端都能收下的形态。
pub fn binary_content_type(content: &str) -> String {
    parse_body_file(content)
        .map(|f| f.content_type)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

// -------------------------------------------------------------------------
// 内部
// -------------------------------------------------------------------------

/// 去重并保持首次出现的次序。
fn dedup(mut names: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    names.retain(|name| seen.insert(name.clone(), ()).is_none());
    names
}

// 测试——规则逐条钉住。

#[cfg(test)]
#[path = "body_test.rs"]
mod tests;

/// 跨语言样本——与渲染进程侧 `src/lib/__tests__/body-parity.test.ts` 读同一份文件。
#[cfg(test)]
#[path = "body_parity_test.rs"]
mod parity_tests;
