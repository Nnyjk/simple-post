//! 请求体的两份内容：`content` 是载荷，`schema` 是定义。
//!
//! 被测的是「这份内容看起来是不是一份 JSON Schema」这个启发式判定 —— 它决定 MCP
//! 侧那两道门（`endpoint_create` / `endpoint_update`）会不会把一次写反拦下来。

use super::{BodyMode, RequestBody};

/// 一份典型的 JSON Schema：顶层有 `type` 与 `properties`。
const SCHEMA: &str = r#"{"type":"object","properties":{"id":{"type":"string"}}}"#;

fn json_body(content: &str) -> RequestBody {
    RequestBody {
        mode: BodyMode::Json,
        content: content.into(),
        schema: None,
    }
}

#[test]
fn a_schema_shaped_payload_is_recognised() {
    assert!(json_body(SCHEMA).content_looks_like_schema());
}

#[test]
fn an_ordinary_payload_is_not_a_schema() {
    let body = json_body(r#"{"id":"a1","name":"ada"}"#);
    assert!(!body.content_looks_like_schema());
    assert!(!body.payload_may_be_a_schema());
}

/// 只带 `type` 的载荷太常见，不能算 schema。
#[test]
fn a_payload_with_only_a_type_key_is_not_a_schema() {
    assert!(!json_body(r#"{"type":"order"}"#).content_looks_like_schema());
}

/// `$schema` 是 JSON Schema 自己的标记键，见到它就是一份 schema。
#[test]
fn a_draft_2020_12_document_is_recognised() {
    let body = json_body(r#"{"$schema":"https://json-schema.org/draft/2020-12/schema"}"#);
    assert!(body.content_looks_like_schema());
}

/// 非 JSON 模式的内容不参与这个判定：它本来就不会被当成 JSON 解析。
#[test]
fn a_non_json_mode_is_never_a_schema() {
    let body = RequestBody {
        mode: BodyMode::Raw,
        content: SCHEMA.into(),
        schema: None,
    };
    assert!(!body.content_looks_like_schema());
}

/// 解析不了的 JSON 不是 schema —— 它连 JSON 都不是。
#[test]
fn broken_json_is_not_a_schema() {
    assert!(!json_body("{not json").content_looks_like_schema());
}

/// 填了 `schema` 就不算写反：那份内容是有意选定的（真要把一份 schema 当载荷发）。
#[test]
fn a_declared_schema_means_the_payload_was_chosen_deliberately() {
    let body = RequestBody {
        mode: BodyMode::Json,
        content: SCHEMA.into(),
        schema: Some(SCHEMA.into()),
    };
    assert!(body.content_looks_like_schema(), "内容本身仍像 schema");
    assert!(!body.payload_may_be_a_schema(), "但填了 schema 就不算写反");
}
