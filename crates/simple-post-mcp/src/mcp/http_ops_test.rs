//! `http_ops` 的单元断言：dispatch 表、入参压平与鉴权 / body 模式的解析。

use std::collections::HashMap;

use serde_json::json;

use crate::mcp::{arg_bool_opt, arg_str, arg_str_opt, arg_u64_opt};
use simple_post_core::domain::{BodyMode, Environment, HttpMethod};

use super::envelope::header_pairs_to_tuples;
use super::input::{extract_settings, flatten_key_values, parse_auth, parse_body_mode};
use super::{flatten_string_map, table, RunRequestArgs};
#[test]
fn table_has_two_rows() {
    assert_eq!(table().len(), 2);
}

#[test]
fn flatten_object_to_pairs() {
    let v = json!({ "X-Foo": "bar", "X-Num": 42 });
    let pairs = flatten_string_map(Some(&v), "h").unwrap();
    assert_eq!(
        pairs,
        vec![
            ("X-Foo".into(), "bar".into()),
            ("X-Num".into(), "42".into())
        ]
    );
}

#[test]
fn flatten_array_of_pairs() {
    let v = json!([["X-A", "1"], ["X-B", "2"]]);
    let pairs = flatten_string_map(Some(&v), "h").unwrap();
    assert_eq!(pairs.len(), 2);
    // 键不能带引号：`Value::String("X-A").to_string()` 会得到 `"X-A"`，
    // 那个头名到了服务端就是一个不存在的头。
    assert_eq!(pairs[0], ("X-A".into(), "1".into()));
}

#[test]
fn flatten_rejects_bad_shape() {
    let v = json!("not a map");
    assert!(flatten_string_map(Some(&v), "h").is_err());
}

/// 三种键值对形状（对象映射 / 键值对数组 / 端点那套对象）都要收下。
#[test]
fn key_values_accept_the_three_shapes() {
    // 对象映射。
    let from_object = flatten_key_values(Some(&json!({ "a": "1" })), "params").unwrap();
    assert_eq!(from_object.len(), 1);
    assert_eq!(from_object[0].key, "a");
    assert_eq!(from_object[0].value, "1");
    assert!(from_object[0].enabled, "没写 enabled 时启用");

    // 键值对数组。
    let from_pairs = flatten_key_values(Some(&json!([["a", "1"]])), "params").unwrap();
    assert_eq!(from_pairs[0].key, "a");
    assert_eq!(from_pairs[0].value, "1");

    // 端点那套形状，`enabled: false` 要被尊重。
    let from_objects = flatten_key_values(
        Some(&json!([{ "key": "a", "value": "1", "enabled": false }])),
        "params",
    )
    .unwrap();
    assert_eq!(from_objects[0].key, "a");
    assert!(!from_objects[0].enabled, "enabled: false 必须被尊重");
}

/// 既不是对象也不是数组的形状要报错，不能凭空造一个空列表。
#[test]
fn key_values_reject_a_shape_that_is_neither() {
    assert!(flatten_key_values(Some(&json!("nope")), "params").is_err());
    assert!(flatten_key_values(Some(&json!([1, 2])), "params").is_err());
}

/// 鉴权载荷用端点那套形状；方案选错要报错，载荷缺了则按端点同一套宽松度处理。
#[test]
fn auth_uses_the_endpoint_shape() {
    let bearer = parse_auth(Some(&json!({ "kind": "bearer", "bearer": "tok" }))).unwrap();
    assert_eq!(bearer.kind, simple_post_core::domain::AuthType::Bearer);
    assert_eq!(bearer.bearer.as_deref(), Some("tok"));

    // 缺席 = 不用鉴权。
    assert_eq!(
        parse_auth(None).unwrap().kind,
        simple_post_core::domain::AuthType::None
    );

    // 形状不对就报错：静默当成「不用鉴权」会发出一个缺了凭据的请求。
    assert!(parse_auth(Some(&json!({ "kind": "不是一种方案" }))).is_err());

    // 方案选了却没给载荷时**不**报错，与端点存的那份同一套宽松度
    // （`AuthConfig` 的载荷字段都是 `Option`）：bearer 没有 token 就是不发
    // `Authorization`，与界面那侧一致。
    let bare = parse_auth(Some(&json!({ "kind": "bearer" }))).unwrap();
    assert_eq!(bare.kind, simple_post_core::domain::AuthType::Bearer);
    assert_eq!(bare.bearer, None);
}

/// 有 body 而没给模式时算 `raw`，没给 body 时算 `none`。
#[test]
fn body_mode_defaults_to_raw_when_a_body_is_given() {
    assert_eq!(parse_body_mode(None, true).unwrap(), BodyMode::Raw);
    assert_eq!(parse_body_mode(None, false).unwrap(), BodyMode::None);
    assert_eq!(parse_body_mode(Some("json"), true).unwrap(), BodyMode::Json);
    assert_eq!(
        parse_body_mode(Some("multipart"), true).unwrap(),
        BodyMode::Multipart
    );
    assert!(parse_body_mode(Some("yaml"), true).is_err());
}

#[test]
fn settings_override_extracts_values() {
    let so = json!({
        "timeout_ms": 5000u64,
        "follow_redirects": false,
        "max_response_size_kb": 1024u64,
    });
    let args = RunRequestArgs {
        method: "GET".into(),
        url: "/x".into(),
        headers: None,
        params: None,
        body: None,
        body_mode: None,
        auth: None,
        env_id: None,
        base_url_definition_id: None,
        module_id: None,
        settings_override: Some(so),
        timeout_ms: None,
        follow_redirects: None,
        max_response_size_kb: None,
    };
    let (t, r, m) = extract_settings(&args.settings_override, &args);
    assert_eq!(t, Some(5000));
    assert_eq!(r, Some(false));
    assert_eq!(m, Some(1024));
}

#[test]
fn arg_helpers_cover_top_level() {
    let v = json!({ "method": "GET", "url": "/x", "n": 7 });
    assert_eq!(arg_str(&v, "method").unwrap(), "GET");
    assert_eq!(arg_str(&v, "url").unwrap(), "/x");
    assert_eq!(arg_u64_opt(&v, "n"), Some(7));
    assert!(!arg_bool_opt(&v, "missing"));
    assert_eq!(arg_str_opt(&v, "missing"), None);
}

#[test]
fn parse_method_known_and_custom() {
    assert_eq!(
        simple_post_core::domain::parse_http_method("get").unwrap(),
        HttpMethod::Get
    );
    assert_eq!(
        simple_post_core::domain::parse_http_method("POST").unwrap(),
        HttpMethod::Post
    );
    assert!(matches!(
        simple_post_core::domain::parse_http_method("FROBNICATE").unwrap(),
        HttpMethod::Custom(_)
    ));
}

#[test]
fn header_pairs_round_trip() {
    let pairs = [[String::from("a"), String::from("b")]];
    let tuples = header_pairs_to_tuples(&pairs);
    assert_eq!(tuples, vec![("a".into(), "b".into())]);
}

/// 环境行里那个 (服务 × 模块) 的格子：base URL 唯一的住址。
/// base URL 唯一的住址：(环境 × 服务 × 模块) 的那个格子。
#[test]
fn a_service_url_lives_in_the_env_cell() {
    let mut module_base_urls = HashMap::new();
    module_base_urls.insert(
        "auth".into(),
        HashMap::from([("mod_a".into(), "https://auth.example.com".into())]),
    );
    let env = Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        module_base_urls,
        variables: HashMap::new(),
        disabled_variables: Vec::new(),
        is_active: true,
    };
    let cell = env
        .module_base_urls
        .get("auth")
        .and_then(|m| m.get("mod_a"));
    assert_eq!(cell.map(String::as_str), Some("https://auth.example.com"));
}
