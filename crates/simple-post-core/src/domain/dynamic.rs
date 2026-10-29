//! 动态变量：一条「变量名 → 数据源 → 抽取规则 → 缓存时长」的声明，以及它的当前值。
//!
//! 消费端点的请求体里写 `{{sns}}`，发送时程序按 [`DynamicVariable`] 取数、抽取、缓存并替换。
//! 人工调试与 Agent 通过 MCP 调试都不用手工搬运 JSON。
//!
//! 作用域是**项目**：一条定义挂在一个项目里，变量名在项目内唯一。不同项目可以有同名变量
//! 而互不覆盖——基线项目与各个客户分发项目各自取自己的那份数据，正因为它们的名字一样、
//! 取值不同。
//!
//! **取值只认消费方所在的项目。** 请求里的 `{{name}}` 在项目 A 里找不到定义时，它就是
//! 缺失的，不会退到项目 B 去拿同名的那条。跨项目取值是**调用接口**的事（那条接口可以
//! 点名项目），不是替换规则的事。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::util::serialises_to_empty_object;

/// 值的序列化形态。
///
/// - `auto`：对象与数组给裸 JSON 片段，标量给纯文本。默认形态。
/// - `json`：始终是 JSON 序列化，故标量带引号（`"A001"`）。
/// - `text`：与 `auto` 的输出**完全一致**。它不是另一种形态，而是用户的声明：值会被放进
///   请求体的字符串位置，转义由用户自己承担。替换环节不做任何转义。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValueMode {
    #[default]
    Auto,
    Json,
    Text,
}

impl ValueMode {
    /// 线字符串。跨版本稳定——`dynamic_variables.value_mode` 存的就是这个形态。
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueMode::Auto => "auto",
            ValueMode::Json => "json",
            ValueMode::Text => "text",
        }
    }

    /// [`Self::as_str`] 的逆。未知值返回 `None`；调用方应当把它当作 `auto`。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(ValueMode::Auto),
            "json" => Some(ValueMode::Json),
            "text" => Some(ValueMode::Text),
            _ => None,
        }
    }
}

/// 一条动态变量的定义。
///
/// 数据源二选一：[`Self::source_endpoint_id`] 引用树里的一个端点，
/// [`Self::source_inline_json`] 是一份内联请求。两个都为空即「没有数据源」。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariable {
    pub id: String,
    /// 归属项目。`None` = 未归属：迁移遗留的定义在归位之前没有项目。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// 变量名，请求体里写 `{{name}}` 用的那个。必须匹配 `^[A-Za-z0-9_]+$`。
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 数据源端点（`nodes.id`）。端点被删后这一列被置为 NULL，定义本身仍在。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_endpoint_id: Option<String>,
    /// 数据源端点的名字。联查 `nodes.name` 得到，好让界面直接显示「数据源：站点清单」
    /// 而不必自己再查一次树。**不是表里的列。**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_endpoint_name: Option<String>,
    /// 内联请求（method / url / headers / body / auth 的 JSON 文本）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_inline_json: Option<String>,
    /// 取数用的环境。`None` = 数据源端点所属项目的第一个可用环境。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_environment_id: Option<String>,
    /// 取数用的服务（`base_url_definitions.id`）。`None` = 数据源接口所属项目的默认服务。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_base_url_definition_id: Option<String>,
    /// JSONPath 表达式。`$` 表示整段响应。
    pub extract_path: String,
    /// 处理脚本。抽取之后跑，用来做 JSONPath 表达不了的加工。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform_script: Option<String>,
    pub value_mode: ValueMode,
    /// 缓存有效期（秒）。
    pub ttl_seconds: i64,
    /// 取数失败时是否允许用已过期的缓存值回退。
    pub stale_fallback: bool,
    /// 是否参与解析。停用的定义不参与替换，`{{name}}` 原样留着。
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 动态变量定义的可改字段。
///
/// 三态语义：缺席 = 不动，`null` = 清除，有值 = 写入。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicVariablePatch {
    /// 归属项目。有值 = 移到那个项目（也用来认领未归属的定义）。
    pub project_id: Option<String>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_endpoint_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_inline_json: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_environment_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_base_url_definition_id: Option<Option<String>>,
    pub extract_path: Option<String>,
    #[serde(default, deserialize_with = "super::double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform_script: Option<Option<String>>,
    pub value_mode: Option<ValueMode>,
    pub ttl_seconds: Option<i64>,
    pub stale_fallback: Option<bool>,
    pub enabled: Option<bool>,
    pub sort_order: Option<i64>,
}

impl DynamicVariablePatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// 缓存里的一行：一个（定义 × 环境）的当前值。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicCacheEntry {
    pub definition_id: String,
    /// 环境 id；空串表示「没有环境」。
    pub environment_id: String,
    /// 抽到的原始值（JSON 文本）。序列化形态由定义的 `value_mode` 决定，与缓存无关。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_json: Option<String>,
    pub fetched_at: i64,
    pub expires_at: i64,
    /// 真 = 这个值是用户手动改的，不是自动取回来的。
    pub is_override: bool,
    /// 覆盖值的过期时间。`None` = 一直有效，直到手动清除。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_expires_at: Option<i64>,
}

/// 取数或解析失败的原因。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DynamicErrorKind {
    /// 取数请求没有到达服务端。
    Network,
    /// 取数请求超时。
    Timeout,
    /// 服务端返回了非 2xx。
    Status,
    /// 响应不是 JSON，无法抽取。
    NotJson,
    /// 响应是 JSON，但抽取路径没命中。
    NoMatch,
    /// 引用的数据源端点已被删除。
    SourceMissing,
    /// 解析数据源端点本身失败（缺 base URL、鉴权失败、抽取表达式非法等）。
    SourceError,
    /// 值超过大小上限。
    TooLarge,
    /// 同一个名字在多个项目里都有定义，跨项目取值时不知道该用哪一条。
    ///
    /// 只在**跨项目**取值时出现：点名了项目就不会有歧义。
    Ambiguous,
    /// 处理脚本跑失败（语法错、超时、抛异常等）。
    ScriptError,
}

/// 一条动态变量的错误。`name` 是出错的变量名，故调用方不必自己猜是谁坏了。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicError {
    pub name: String,
    pub kind: DynamicErrorKind,
    pub message: String,
}

/// 当前值是从哪来的。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DynamicValueSource {
    /// 自动取回来的，且缓存还在有效期内。
    Auto,
    /// 用户手动改的。
    Override,
    /// 取数失败，用的是已过期的缓存值。
    Stale,
    /// 没有值。
    Missing,
}

/// 一条动态变量的当前状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicValueStatus {
    pub name: String,
    /// 当前生效的值（已按 `value_mode` 序列化）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub source: DynamicValueSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub is_override: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DynamicError>,
}

/// 一次解析的结果：值、状态与错误。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicResolution {
    /// 变量名 → 生效的值。缺席的名字按缺失处理。
    pub values: HashMap<String, String>,
    pub statuses: Vec<DynamicValueStatus>,
    pub errors: Vec<DynamicError>,
}
