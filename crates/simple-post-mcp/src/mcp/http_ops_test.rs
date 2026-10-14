use super::*;
use crate::mcp::{arg_bool_opt, arg_str, arg_str_opt, arg_u64_opt};
use simple_post_core::domain::Environment;
use std::collections::HashMap;

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
}

#[test]
fn flatten_rejects_bad_shape() {
    let v = json!("not a map");
    assert!(flatten_string_map(Some(&v), "h").is_err());
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
fn apply_base_url_handles_env_and_override() {
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
        is_active: true,
    };
    let (joined, base) = apply_base_url("/login", Some(&env), Some("auth"), Some("mod_a"));
    assert_eq!(joined, "https://auth.example.com/login");
    assert_eq!(base, "https://auth.example.com");

    let (passthru, _) = apply_base_url("/login", None, None, None);
    assert_eq!(passthru, "/login");

    // 没有模块就没有格子：同一个服务只有 `mod_a` 有 URL，且不向它或别的服务兜底。
    let (no_module, empty) = apply_base_url("/login", Some(&env), Some("auth"), None);
    assert_eq!(no_module, "/login");
    assert_eq!(empty, "");
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
