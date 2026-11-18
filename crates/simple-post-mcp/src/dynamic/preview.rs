//! 取数预览：用**表单里当前这份草稿**真发一次取数请求，把响应交给人点选。
//!
//! 面板的「取数预览」走这里。它与真实解析共用同一条取数器（[`super::source::EndpointSourceFetcher`]）
//! 与同一份全局请求设置（`resolve_settings` 取的就是发送时那一份），故「预览能取到、发送取不到」
//! 这类漂移不会发生——上限、超时、环境解析全都是同一个实现。
//!
//! ## 不碰持久状态
//!
//! 预览是**只读**的一次往返：不读也不写 `dynamic_variable_cache`，不过 single-flight，
//! 不产生覆盖值，不写项目的环境变量（脚本的写入只活在这一趟里），不写响应历史。
//! 故同一份草稿重复预览幂等，也不会把用户手写的值冲掉。
//!
//! ## 先取数，后抽取
//!
//! 抽取失败（响应不是 JSON、路径没命中、路径表达式非法）**不让预览失败**：正文与
//! `bodyJson` 照样返回，否则用户看不到树，也就没法把 JSONPath 点出来。处理脚本失败同理
//! ——真实链路里它就是「按缺失处理」，预览把错误记在 `transformError` 上，`value` 仍是
//! 抽取结果。
//!
//! ## 编排住在这里
//!
//! 两个薄壳（Tauri 命令与 REST handler）都只做参数拼装。逻辑若落进任一侧，另一侧就会
//! 静默少一半——同样的理由，`dynamic::ops` 也住在本 crate。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use simple_post_core::db::DbPool;
use simple_post_core::domain::{DynamicError, DynamicErrorKind, DynamicVariable, ValueMode};
use simple_post_core::dynamic::{
    apply_transform, extract_value, source_context, RawSource, ResolveContext, SourceContext,
    SourceFetcher,
};
use simple_post_core::error::AppError;

use super::source::EndpointSourceFetcher;

/// 面板送来的草稿。**全是可选项**：用户可能在填一个还没有名字的新定义。
///
/// 它刻意不是 `DynamicVariable`：定义里的 id / 缓存 / 排序那些字段对预览没有意义，而
/// 「读一条已存的定义」也不是预览该做的事——预览看的是表单里这一刻那份草稿。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDraft {
    /// 变量名。只用来给错误消息一个主语，预览本身不校验它。
    #[serde(default)]
    pub name: Option<String>,
    /// 数据源：引用树里的一个端点。与 `source_inline_json` 同时给时**以端点为先**
    /// （与真实取数一致，见 `dynamic::source::EndpointSourceFetcher`）。
    #[serde(default)]
    pub source_endpoint_id: Option<String>,
    /// 数据源：内联请求的 JSON 文本。
    #[serde(default)]
    pub source_inline_json: Option<String>,
    /// 取数用的环境。`None` = 继承调用方此刻选中的环境。
    #[serde(default)]
    pub source_environment_id: Option<String>,
    /// JSONPath。空串与缺席都表示整段响应（`$`）。
    #[serde(default)]
    pub extract_path: Option<String>,
    /// 处理脚本。预览会真的跑它一次。
    #[serde(default)]
    pub transform_script: Option<String>,
}

impl PreviewDraft {
    /// 把草稿摊成取数器认识的那条定义。
    ///
    /// 空串按「没填」处理：界面把没选的字段发成 `""`，若原样交给取数器，「引用端点」形态
    /// 会去查一个 id 为空串的端点，报的是「端点已被删除」而不是「没有数据源」。
    pub fn into_definition(self) -> DynamicVariable {
        DynamicVariable {
            id: String::new(),
            project_id: None,
            name: self.name.unwrap_or_default(),
            description: None,
            source_endpoint_id: non_empty(self.source_endpoint_id),
            source_endpoint_name: None,
            source_inline_json: non_empty(self.source_inline_json),
            source_environment_id: non_empty(self.source_environment_id),
            extract_path: self.extract_path.unwrap_or_else(|| "$".into()),
            transform_script: self.transform_script,
            value_mode: ValueMode::Auto,
            ttl_seconds: 0,
            stale_fallback: false,
            enabled: true,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }
}

/// `Some("")` 与 `Some("   ")` 都是「没填」。
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

/// 预览里的一条问题。`kind` 与 `DynamicErrorKind` 的线字符串同名，前端据此选措辞。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewIssue {
    /// `not_json` / `no_match` / `source_error` / `script_error` / `binary`。
    pub kind: String,
    pub message: String,
}

impl PreviewIssue {
    fn new(kind: DynamicErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind: kind_label(&kind).to_string(),
            message: message.into(),
        }
    }
}

/// 一次预览的结果。
///
/// 正文有**三种**形态，预览都要说清楚是哪一种，不能都显示成空树：
///
/// - JSON：`bodyJson` 有值，树可以点；
/// - 不是 JSON 的文本：`bodyJson` 为空，`bodyText` 是原文；
/// - 二进制：`binary` 为真，`bodyText` 是宽松解码的结果（不可逆），`bodyJson` 必然为空。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub status: u16,
    pub content_type: String,
    pub duration_ms: u64,
    /// 正文被全局上限裁过。树是不完整的，界面要明说。
    pub truncated: bool,
    /// 正文是二进制（图片、压缩包……）。此时不能做 JSONPath。
    pub binary: bool,
    /// 文本正文。二进制响应时它是宽松解码的结果（非法字节已变成 U+FFFD）。
    pub body_text: String,
    /// **严格**解析出来的 JSON。不是 JSON 就是 `None`——刻意不用 `parse_raw`，那个会把 HTML
    /// 错误页降级成一个字符串节点，让用户以为接口返回了 JSON。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_json: Option<Value>,
    /// 实际生效的抽取规则（空串已被换成 `$`），界面拿它拼脚本片段。
    pub extract_path: String,
    /// 实际生效的取数环境。`None` = 没有环境。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// JSONPath 抽出来的值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted: Option<Value>,
    /// 抽取失败的原因。有它时 `bodyJson` 仍然在，用户能照着树重新点一条路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_error: Option<PreviewIssue>,
    /// 最终会进变量池的那个值：脚本成功用脚本的返回值，否则用抽取结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// 处理脚本失败的原因。有它时 `value` 仍是抽取结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform_error: Option<PreviewIssue>,
}

/// 用一份草稿取一次数，抽出它的值，并给出足够改 JSONPath 的上下文。
///
/// `caller_environment_id` 是**调用方此刻选中的环境**，只对「继承取数环境」的草稿起作用
/// （与真实解析同一个判定，见 `core::dynamic::source_context`）。
///
/// 取数失败是**唯一**会让整条预览失败的路径：请求没发出去，就没有正文可看。抽取与处理
/// 脚本的失败都落进结果里的对应字段。
pub async fn preview(
    pool: &DbPool,
    draft: PreviewDraft,
    caller_environment_id: Option<&str>,
) -> Result<PreviewResult, AppError> {
    let definition = draft.into_definition();
    let source = source_context(pool, &definition, caller_environment_id)?;
    let ctx = ResolveContext {
        pool: pool.clone(),
        environment_id: caller_environment_id.map(str::to_string),
    };
    // 与真实解析同一个取数器：两种数据源形态、环境解析、超时与体积上限都在它里面。
    let raw = EndpointSourceFetcher
        .fetch(&definition, &ctx, &source)
        .await
        .map_err(as_app_error)?;
    Ok(evaluate(&definition, &raw, &source).await)
}

/// 取回来的响应 → 预览结果。纯计算（除了跑处理脚本）。
///
/// 单独拆出来是为了能不起网络地钉住三件事：抽取失败仍带正文、二进制被点名、截断被透传。
async fn evaluate(
    definition: &DynamicVariable,
    raw: &RawSource,
    source: &SourceContext,
) -> PreviewResult {
    let extract_path = effective_path(&definition.extract_path);
    // `body_base64` 有值说明原始字节不是合法 UTF-8：`body` 只是宽松解码的残影，抽不出东西。
    let binary = raw.body_base64.is_some();
    let body_json = if binary {
        None
    } else {
        // 正文解析两遍（这里一次、`extract_value` 里一次）：抽取必须走真实链路那一份实现，
        // 否则预览与运行时对「是不是 JSON」会各说各话。一次预览多解析一遍不值得为它分叉。
        serde_json::from_str::<Value>(&raw.body).ok()
    };

    let mut extracted = None;
    let mut extract_error = None;
    if binary {
        extract_error = Some(PreviewIssue {
            kind: "binary".to_string(),
            message: "这是二进制正文（图片、压缩包等），不能做 JSONPath".to_string(),
        });
    } else {
        match extract_value(&raw.body, &definition.extract_path) {
            Ok(value) => extracted = Some(value),
            Err(kind) => extract_error = Some(extract_issue(&kind, &extract_path)),
        }
    }

    let extracted_snapshot = extracted.clone();
    let mut value = extracted.clone();
    let mut transform_error = None;
    if let Some(input) = extracted {
        match apply_transform(definition, input, raw, source).await {
            Ok(transformed) => value = Some(transformed),
            // 真实链路里脚本失败就是「按缺失处理」。预览不吞掉抽取结果：用户要的是
            // 「这段 JSONPath 抽到了什么」，脚本坏没坏是另一条信息。
            Err(error) => {
                transform_error = Some(PreviewIssue::new(
                    DynamicErrorKind::ScriptError,
                    error.message,
                ))
            }
        }
    }

    PreviewResult {
        status: raw.status,
        content_type: raw.content_type.clone(),
        duration_ms: raw.duration_ms,
        truncated: raw.truncated,
        binary,
        body_text: raw.body.clone(),
        body_json,
        extract_path,
        environment_id: source.environment_id.clone(),
        extracted: extracted_snapshot,
        extract_error,
        value,
        transform_error,
    }
}

/// 生效的抽取规则。空白即整段响应——与 `core::dynamic::extract_value` 同一套回落规则。
fn effective_path(extract_path: &str) -> String {
    let trimmed = extract_path.trim();
    if trimmed.is_empty() {
        "$".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 抽取失败 → 给人看的一句话。措辞与真实解析（`core::dynamic::resolve`）一致，
/// 否则同一份定义在预览与运行时说两套话。
fn extract_issue(kind: &DynamicErrorKind, path: &str) -> PreviewIssue {
    let message = match kind {
        DynamicErrorKind::NotJson => "数据源的响应不是 JSON".to_string(),
        DynamicErrorKind::NoMatch => format!("抽取路径 '{path}' 没有命中"),
        _ => format!("抽取路径 '{path}' 不是合法的 JSONPath"),
    };
    PreviewIssue::new(*kind, message)
}

/// `DynamicErrorKind` 的线字符串。渲染层按它决定措辞，故必须稳定。
fn kind_label(kind: &DynamicErrorKind) -> &'static str {
    match kind {
        DynamicErrorKind::Network => "network",
        DynamicErrorKind::Timeout => "timeout",
        DynamicErrorKind::Status => "status",
        DynamicErrorKind::NotJson => "not_json",
        DynamicErrorKind::NoMatch => "no_match",
        DynamicErrorKind::SourceMissing => "source_missing",
        DynamicErrorKind::SourceError => "source_error",
        DynamicErrorKind::TooLarge => "too_large",
        DynamicErrorKind::Ambiguous => "ambiguous",
        DynamicErrorKind::ScriptError => "script_error",
    }
}

/// 取数失败 → 应用错误。网络 / 超时是传输层的错，其余是配置的错——后者以行内提示呈现。
fn as_app_error(error: DynamicError) -> AppError {
    match error.kind {
        DynamicErrorKind::Network | DynamicErrorKind::Timeout => AppError::Http(error.message),
        _ => AppError::Validation(error.message),
    }
}

#[cfg(test)]
#[path = "preview_test.rs"]
mod tests;
