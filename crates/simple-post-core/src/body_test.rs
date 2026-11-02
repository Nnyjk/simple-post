//! 请求体解析规则的行为用例。

use std::collections::HashMap;

use crate::body::{
    parse_body_file, parse_form_fields, parse_multipart_parts, resolve_body, serialize_form_fields,
};
use crate::domain::{BodyFile, BodyMode, FormField, RequestBody};

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn body(mode: BodyMode, content: &str) -> RequestBody {
    RequestBody {
        mode,
        content: content.to_string(),
        schema: None,
    }
}

// -------------------------------------------------------------------------
// form 的行
// -------------------------------------------------------------------------

#[test]
fn form_rows_are_read_from_the_json_array() {
    let fields = parse_form_fields(
        r#"[{"key":"name","value":"ada","enabled":true},{"key":"age","value":"42"}]"#,
    );
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].key, "name");
    assert_eq!(fields[0].value, "ada");
    assert!(fields[0].enabled);
    // `enabled` 缺省为真：一份没写它的行就是一行要发的表单字段。
    assert!(fields[1].enabled);
    assert_eq!(fields[1].value, "42");
}

#[test]
fn a_legacy_urlencoded_body_is_read_as_rows() {
    let fields = parse_form_fields("name=ada&age=42");
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].key, "name");
    assert_eq!(fields[0].value, "ada");
    assert_eq!(fields[1].key, "age");
    assert_eq!(fields[1].value, "42");
    assert!(fields.iter().all(|f| f.enabled));
}

#[test]
fn a_legacy_body_keeps_the_percent_signs_verbatim() {
    // 值不做百分号解码：那份内容存的是用户写下的原文，解码会把 `%` 变成别的字符。
    let fields = parse_form_fields("rate=100%25&note=50%");
    assert_eq!(fields[0].value, "100%25");
    assert_eq!(fields[1].value, "50%");
}

#[test]
fn rows_survive_the_round_trip_through_the_json_text() {
    let fields = vec![
        FormField {
            key: "name".into(),
            value: "ada".into(),
            enabled: true,
            description: Some("姓名".into()),
        },
        FormField {
            key: "age".into(),
            value: "42".into(),
            enabled: false,
            description: None,
        },
    ];
    let text = serialize_form_fields(&fields);
    let back = parse_form_fields(&text);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].description.as_deref(), Some("姓名"));
    assert!(!back[1].enabled);
}

// -------------------------------------------------------------------------
// form 的正文
// -------------------------------------------------------------------------

#[test]
fn a_form_body_is_urlencoded_with_percent_twenty_for_spaces() {
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"q","value":"a b","enabled":true}]"#,
        ),
        &vars(&[]),
    );
    // 空格是 `%20`，不是 form 编码的 `+`——与查询参数同一条规则。
    assert_eq!(out.text.as_deref(), Some("q=a%20b"));
}

#[test]
fn a_disabled_row_and_a_row_without_a_key_stay_out_of_the_body() {
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"a","value":"1","enabled":true},{"key":"b","value":"2","enabled":false},{"key":"","value":"3","enabled":true}]"#,
        ),
        &vars(&[]),
    );
    assert_eq!(out.text.as_deref(), Some("a=1"));
}

#[test]
fn the_variables_of_a_skipped_row_are_not_reported_missing() {
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"","value":"{{nope}}","enabled":true}]"#,
        ),
        &vars(&[]),
    );
    assert!(out.missing.is_empty(), "got: {:?}", out.missing);
}

#[test]
fn a_form_value_is_templated_per_field() {
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"token","value":"{{t}}","enabled":true}]"#,
        ),
        &vars(&[("t", "abc")]),
    );
    assert_eq!(out.text.as_deref(), Some("token=abc"));
}

#[test]
fn a_templated_value_does_not_break_the_content() {
    // 整段模板化会把替换出来的引号塞进那段 JSON 文本里，于是整份内容解析不出来、
    // 请求发出去的是一个空 body。逐字段模板化没有这个问题。
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"q","value":"{{t}}","enabled":true}]"#,
        ),
        &vars(&[("t", r#"a"b"#)]),
    );
    assert_eq!(out.text.as_deref(), Some(r#"q=a%22b"#));
}

#[test]
fn a_missing_variable_in_a_form_value_is_reported() {
    let out = resolve_body(
        &body(
            BodyMode::Form,
            r#"[{"key":"q","value":"{{nope}}","enabled":true}]"#,
        ),
        &vars(&[]),
    );
    assert_eq!(out.missing, vec!["nope".to_string()]);
}

#[test]
fn a_legacy_form_body_is_encoded_the_same_way() {
    let out = resolve_body(&body(BodyMode::Form, "name=ada"), &vars(&[]));
    assert_eq!(out.text.as_deref(), Some("name=ada"));
}

// -------------------------------------------------------------------------
// multipart 的分片
// -------------------------------------------------------------------------

#[test]
fn multipart_parts_are_read_from_the_json_array() {
    let parts = parse_multipart_parts(r#"[{"name":"field","value":"v1"}]"#);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "field");
    assert_eq!(parts[0].value, "v1");
}

#[test]
fn a_malformed_parts_array_yields_no_parts() {
    assert!(parse_multipart_parts("[{\"name\":1}]").is_empty());
    assert!(parse_multipart_parts("not json").is_empty());
}

#[test]
fn a_multipart_text_part_is_templated_and_a_file_part_is_not() {
    let out = resolve_body(
        &body(
            BodyMode::Multipart,
            r#"[{"name":"field","value":"{{t}}"},{"name":"file","filename":"a.png","base64":"iVBORw0KGgo="}]"#,
        ),
        &vars(&[("t", "v1")]),
    );
    let parts = parse_multipart_parts(out.text.as_deref().unwrap_or_default());
    assert_eq!(parts[0].value, "v1");
    // 文件分片的字节不模板化。
    assert_eq!(parts[1].base64.as_deref(), Some("iVBORw0KGgo="));
    assert_eq!(parts[1].filename.as_deref(), Some("a.png"));
}

// -------------------------------------------------------------------------
// binary 的文件
// -------------------------------------------------------------------------

#[test]
fn a_binary_body_carries_the_file_bytes() {
    let out = resolve_body(
        &body(
            BodyMode::Binary,
            r#"{"name":"a.png","size":3,"contentType":"image/png","base64":"iVBORw0KGgo="}"#,
        ),
        &vars(&[]),
    );
    assert_eq!(out.base64.as_deref(), Some("iVBORw0KGgo="));
    assert!(out.text.is_none());
}

#[test]
fn a_binary_body_whose_content_is_not_a_file_description_has_no_body() {
    let out = resolve_body(&body(BodyMode::Binary, "not json"), &vars(&[]));
    assert!(out.text.is_none());
    assert!(out.base64.is_none());
}

#[test]
fn the_file_description_is_read_back() {
    let file = parse_body_file(
        r#"{"name":"a.png","size":3,"contentType":"image/png","base64":"iVBORw0KGgo="}"#,
    )
    .expect("the file description");
    assert_eq!(file.name, "a.png");
    assert_eq!(file.size, 3);
    assert_eq!(file.content_type, "image/png");
    assert_eq!(file.base64, "iVBORw0KGgo=");

    let empty: Option<BodyFile> = parse_body_file("{}");
    assert!(
        empty.is_some(),
        "缺字段的文件描述读得出来，缺的字段取缺省值"
    );
}

// -------------------------------------------------------------------------
// 其它模式
// -------------------------------------------------------------------------

#[test]
fn a_text_body_is_templated_as_a_whole() {
    let out = resolve_body(&body(BodyMode::Xml, "<a>{{t}}</a>"), &vars(&[("t", "1")]));
    assert_eq!(out.text.as_deref(), Some("<a>1</a>"));
    assert!(out.base64.is_none());
}

#[test]
fn no_mode_has_no_body() {
    let out = resolve_body(&body(BodyMode::None, "ignored"), &vars(&[]));
    assert!(out.text.is_none());
    assert!(out.base64.is_none());
}

#[test]
fn an_empty_content_has_no_body() {
    let out = resolve_body(&body(BodyMode::Json, ""), &vars(&[]));
    assert!(out.text.is_none());
}
