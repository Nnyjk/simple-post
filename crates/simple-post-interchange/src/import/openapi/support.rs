//! 导入：`$ref` 展开、`servers` 拆分、多 media type、cookie/trace 与两种文本输入。

use serde_json::Value;

use crate::diagnostics::{Diagnostic, Severity};
use crate::ir::Exchange;

use super::parse;

pub(super) fn ir_of(src: &str) -> Exchange {
    let value: Value =
        serde_json::from_str(src).unwrap_or_else(|e| panic!("测试文档不是 JSON：{e}"));
    parse(&value)
        .unwrap_or_else(|e| panic!("解析失败：{e}"))
        .exchange
}

pub(super) fn warns(ex: &Exchange) -> Vec<&Diagnostic> {
    ex.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .collect()
}

pub(super) fn dropped(ex: &Exchange) -> Vec<&str> {
    ex.diagnostics
        .iter()
        .filter_map(|d| d.dropped.as_deref())
        .collect()
}

pub(super) fn first_endpoint(ex: &Exchange) -> &crate::ir::IrEndpoint {
    &ex.modules[0].endpoints[0]
}
