//! body.mode 的四种落法：multipart 分片数组、form 串、raw 文本，以及出声丢弃的那些。

use serde_json::json;
use simple_post_core::domain::BodyMode;

use crate::diagnostics::Severity;

use super::support::{endpoints_in_order, parse_ok};

/// 测试侧的 `MultipartPart` 镜像：与 `src/lib/multipart.ts` 的线格式一致，
/// 故这里能反序列化成功，正说明导入写出的 JSON 发送侧读得懂。
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}

#[test]
fn text_only_formdata_becomes_a_multipart_part_array() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "上传",
            "request": {
                "method": "POST",
                "body": {
                    "mode": "formdata",
                    "formdata": [
                        { "key": "field", "value": "1", "type": "text" },
                        { "key": "note", "value": "hello" }
                    ]
                },
                "url": "https://api.example.com/up"
            }
        }]
    }));

    let body = &parsed.exchange.modules[0].endpoints[0].body;
    assert_eq!(body.mode, BodyMode::Multipart);
    let parts: Vec<Part> = serde_json::from_str(&body.content).expect("content 必须是分片数组");
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "field");
    assert_eq!(parts[0].value, "1");
    assert_eq!(parts[1].name, "note");
    assert_eq!(parts[1].value, "hello");
    // 文本分片不带这两个位；写出的 null 会让 TS / Rust 的分片解析判成非法输入并整个丢掉。
    assert!(parts[0].filename.is_none());
    assert!(parts[0].content_type.is_none());
    assert!(!body.content.contains("null"));
}

#[test]
fn a_formdata_file_field_is_reported_and_dropped() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "上传",
            "request": {
                "method": "POST",
                "body": {
                    "mode": "formdata",
                    "formdata": [
                        { "key": "field", "value": "1", "type": "text" },
                        { "key": "avatar", "type": "file", "src": "C:/tmp/a.png" }
                    ]
                },
                "url": "https://api.example.com/up"
            }
        }]
    }));

    let ex = &parsed.exchange;
    assert_eq!(ex.diagnostics.len(), 1);
    let d = &ex.diagnostics[0];
    assert_eq!(d.severity, Severity::Warn);
    assert_eq!(d.pointer, "/item/0/request/body/formdata/1");
    assert_eq!(d.dropped.as_deref(), Some("multipart file 字段 avatar"));

    let body = &ex.modules[0].endpoints[0].body;
    let parts: Vec<Part> = serde_json::from_str(&body.content).expect("content 必须是分片数组");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "field");
}

#[test]
fn other_body_modes_land_where_the_sender_expects_them() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [
            {
                "name": "urlencoded",
                "request": {
                    "method": "POST",
                    "body": {
                        "mode": "urlencoded",
                        "urlencoded": [
                            { "key": "a", "value": "1" },
                            { "key": "b", "value": "2" },
                            { "key": "off", "value": "3", "disabled": true }
                        ]
                    },
                    "url": "https://api.example.com/a"
                }
            },
            {
                "name": "raw text",
                "request": {
                    "method": "POST",
                    "body": { "mode": "raw", "raw": "hello" },
                    "url": "https://api.example.com/b"
                }
            },
            {
                "name": "graphql",
                "request": {
                    "method": "POST",
                    "body": { "mode": "graphql", "graphql": { "query": "{ a }" } },
                    "url": "https://api.example.com/c"
                }
            },
            {
                "name": "空 body",
                "request": { "method": "POST", "url": "https://api.example.com/d" }
            }
        ]
    }));

    let ex = &parsed.exchange;
    let eps = endpoints_in_order(ex);
    assert_eq!(eps.len(), 4, "四条顶层散落的请求都进了同一个模块");
    assert_eq!(eps[0].body.mode, BodyMode::Form);
    // `form` 模式的内容是**行数组**，不是拼好的 urlencoded 串：界面上的表格才有
    // 地方放，而被 Postman 禁用的字段也留得下来（标成不参与发送）。
    assert_eq!(
        eps[0].body.content,
        r#"[{"key":"a","value":"1","enabled":true},{"key":"b","value":"2","enabled":true},{"key":"off","value":"3","enabled":false}]"#
    );
    assert_eq!(eps[1].body.mode, BodyMode::Raw);
    assert_eq!(eps[1].body.content, "hello");
    assert_eq!(eps[2].body.mode, BodyMode::None);
    assert_eq!(eps[3].body.mode, BodyMode::None);

    // graphql 没有对应形态，必须出声。
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(ex.diagnostics[0].dropped.as_deref(), Some("graphql 请求体"));
}

/// 畸形条目的指针要指到它真正住的那一层。
///
/// `urlencoded[]` 的每一项住在 `body/urlencoded/` 下面。指 body 本身的话，用户
/// 看到的 `/item/0/request/body/1` 在源文件里指向一个**数组**，点不出那一行。
/// 同族的 `formdata` 与 `header` 本来就指对了，故这是 `urlencoded` 一处单独的错。
#[test]
fn a_malformed_urlencoded_entry_points_at_the_urlencoded_level() {
    let parsed = parse_ok(&json!({
        "info": { "name": "n" },
        "item": [{
            "name": "表单",
            "request": {
                "method": "POST",
                "body": {
                    "mode": "urlencoded",
                    "urlencoded": [
                        { "key": "a", "value": "1" },
                        "不是对象"
                    ]
                },
                "url": "https://api.example.com/a"
            }
        }]
    }));

    let ex = &parsed.exchange;
    assert_eq!(ex.diagnostics.len(), 1);
    assert_eq!(ex.diagnostics[0].severity, Severity::Warn);
    assert_eq!(
        ex.diagnostics[0].pointer,
        "/item/0/request/body/urlencoded/1"
    );
}
