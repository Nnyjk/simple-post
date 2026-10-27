//! `resolve_endpoint` 的拼接/替换顺序、baseUrl 优先级、body 与 query 的取舍。

use simple_post_core::domain::{
    AuthConfig, AuthType, BodyMode, EndpointDetail, HttpMethod, KeyValue, RequestBody,
};

use super::resolve::{resolve_endpoint, resolve_endpoint_with};
use super::test_support::{
    empty_env, in_module_a, minimal_endpoint, project_with_defs, set_cell, MOD_A,
};
use super::EndpointOverrides;
// ---- resolve_endpoint join/substitute ordering ---------------------
//
// Same boundaries as the TS `prepareRequest` suite in
// `src/lib/__tests__/http-build.test.ts`.

#[test]
fn resolve_endpoint_leading_slash_var_does_not_double_the_separator() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200");
    env.variables.insert("var".into(), "/api".into());
    let mut ep = minimal_endpoint();
    ep.url = "{{var}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "http://host:9200/api");
    assert!(r.missing_vars.is_empty());
}

#[test]
fn resolve_endpoint_leading_slash_var_with_trailing_slash_base() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200/");
    env.variables.insert("var".into(), "/api".into());
    let mut ep = minimal_endpoint();
    ep.url = "{{var}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "http://host:9200/api");
}

#[test]
fn resolve_endpoint_collapses_marker_boundary_mid_path() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200");
    env.variables.insert("var".into(), "/api".into());
    let mut ep = minimal_endpoint();
    ep.url = "{{var}}/users".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "http://host:9200/api/users");
}

#[test]
fn resolve_endpoint_var_without_leading_slash_keeps_separator() {
    // 收的是*重复*斜杠，绝不碰拼接插入的那一个 —— 否则 `api` 会被粘到 host 上。

    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200");
    env.variables.insert("var".into(), "api".into());
    let mut ep = minimal_endpoint();
    ep.url = "{{var}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "http://host:9200/api");
}

#[test]
fn resolve_endpoint_schemed_path_still_wins_over_base() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://custom.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "https://other.example.com/users/1".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://other.example.com/users/1");
}

#[test]
fn resolve_endpoint_unresolved_marker_survives_the_collapse() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200");
    let mut ep = minimal_endpoint();
    ep.url = "{{gone}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // The marker is text, not a separator; `missing_vars` is the only
    // 调用方唯一能看出 URL 不完整的信号。
    assert_eq!(r.url, "http://host:9200/{{gone}}");
    assert_eq!(r.missing_vars, vec!["gone".to_string()]);
}

#[test]
fn resolve_endpoint_appends_params_after_collapsing() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "http://host:9200");
    env.variables.insert("var".into(), "/api".into());
    let mut ep = minimal_endpoint();
    ep.url = "{{var}}?foo=bar".into();
    ep.base_url_definition_id = Some("data".into());
    ep.params = vec![KeyValue {
        id: "p1".into(),
        key: "page".into(),
        value: "1".into(),
        enabled: true,
        description: None,
    }];
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // 收斜杠发生在追加 query 之前，且不能吃掉已经存在的 query ——
    // `?`/`&` 的选择要读这整个 URL。
    assert_eq!(r.url, "http://host:9200/api?foo=bar&page=1");
}

// ---- resolve_endpoint 的 baseUrl 优先级 -------------------------

#[test]
fn resolve_endpoint_picks_override_over_definition() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/users".into();
    ep.base_url_definition_id = Some("data".into());
    ep.base_url_override = Some("https://override.example.com".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://override.example.com/users");
}

#[test]
fn resolve_endpoint_substitutes_vars_in_url_path() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("id".into(), "42".into());
    let mut ep = minimal_endpoint();
    ep.url = "/users/{{id}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/42");
    assert!(r.missing_vars.is_empty());
}

#[test]
fn a_disabled_variable_is_not_substituted() {
    // 停用的变量照样存、照样显示，但不参与替换：`{{id}}` 留在原样并进 `missing_vars`，
    // 用户看得见「这个变量没被替换」，而不是静默被替掉。
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("id".into(), "42".into());
    env.disabled_variables.push("id".into());
    let mut ep = minimal_endpoint();
    ep.url = "/users/{{id}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/{{id}}");
    assert_eq!(r.missing_vars, vec!["id".to_string()]);
}

#[test]
fn an_enabled_sibling_variable_is_still_substituted() {
    // 停用一个变量不能碰到它的邻居。
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("id".into(), "42".into());
    env.variables.insert("name".into(), "ada".into());
    env.disabled_variables.push("id".into());
    let mut ep = minimal_endpoint();
    ep.url = "/users/{{id}}/{{name}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/{{id}}/ada");
    assert_eq!(r.missing_vars, vec!["id".to_string()]);
}

#[test]
fn resolve_endpoint_collects_missing_vars_across_url_headers_body_auth() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/users/{{user_id}}".into();
    ep.base_url_definition_id = Some("data".into());
    ep.headers = vec![KeyValue {
        id: "h1".into(),
        key: "X-Trace".into(),
        value: "{{trace_id}}".into(),
        enabled: true,
        description: None,
    }];
    ep.body = RequestBody {
        mode: BodyMode::Raw,
        content: "post={{payload}}".into(),
        schema: None,
    };
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("Bearer {{token}}".into()),
        basic: None,
        apikey: None,
        oauth2: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // URL + headers + body + auth 的 missing 变量会合并、去重，
    // and ordered by first appearance.
    assert!(r.missing_vars.contains(&"user_id".to_string()));
    assert!(r.missing_vars.contains(&"trace_id".to_string()));
    assert!(r.missing_vars.contains(&"payload".to_string()));
    assert!(r.missing_vars.contains(&"token".to_string()));
    // 而未解析的标记原地不动（从
    // body 字符串与 URL 里都能看到）：
    assert!(r.url.contains("{{user_id}}"));
    assert!(r.body.unwrap().contains("{{payload}}"));
}

// ---- body inclusion ----------------------------------------------

#[test]
fn resolve_endpoint_skips_empty_body() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.body = RequestBody {
        mode: BodyMode::Json,
        content: "".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert!(r.body.is_none());
}

#[test]
fn resolve_endpoint_skips_none_mode() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.body = RequestBody {
        mode: BodyMode::None,
        content: "ignored".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert!(r.body.is_none());
}

#[test]
fn resolve_endpoint_auto_content_type_added() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.method = HttpMethod::Post;
    ep.body = RequestBody {
        mode: BodyMode::Json,
        content: "{\"x\":1}".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    let ct = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .unwrap();
    assert_eq!(ct.1, "application/json");
    let accept = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("accept"))
        .unwrap();
    assert_eq!(accept.1, "application/json");
}

#[test]
fn resolve_endpoint_user_content_type_wins() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.method = HttpMethod::Post;
    ep.headers = vec![KeyValue {
        id: "h1".into(),
        key: "Content-Type".into(),
        value: "application/vnd.custom+json".into(),
        enabled: true,
        description: None,
    }];
    ep.body = RequestBody {
        mode: BodyMode::Json,
        content: "{}".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    let ct = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .unwrap();
    assert_eq!(ct.1, "application/vnd.custom+json");
}

// ---- query params -------------------------------------------------

#[test]
fn resolve_endpoint_appends_enabled_params_to_url() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/search".into();
    ep.base_url_definition_id = Some("data".into());
    ep.params = vec![
        KeyValue {
            id: "p1".into(),
            key: "q".into(),
            value: "hello world".into(),
            enabled: true,
            description: None,
        },
        KeyValue {
            id: "p2".into(),
            key: "disabled".into(),
            value: "x".into(),
            enabled: false,
            description: None,
        },
        KeyValue {
            id: "p3".into(),
            key: "empty".into(),
            value: "".into(),
            enabled: true,
            description: None,
        },
    ];
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // 空格是 `%20`（与渲染进程的 `encodeURIComponent` 同字符集），不是 form 编码的 `+`。
    assert!(r.url.contains("q=hello%20world"));
    assert!(!r.url.contains("disabled="));
    assert!(!r.url.contains("empty="));
}

#[test]
fn resolve_endpoint_merges_existing_query_with_params() {
    // URL already has a query string; params should be appended
    // with `&`, not `?`.
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/search?existing=1".into();
    ep.base_url_definition_id = Some("data".into());
    ep.params = vec![KeyValue {
        id: "p1".into(),
        key: "extra".into(),
        value: "2".into(),
        enabled: true,
        description: None,
    }];
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/search?existing=1&extra=2");
}

// ---- overrides ---------------------------------------------------

#[test]
fn resolve_endpoint_applies_url_override() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let ep = minimal_endpoint();
    let project = project_with_defs();
    let overrides = EndpointOverrides {
        url: Some("https://override.example.com/custom".into()),
        ..Default::default()
    };
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&overrides)).unwrap();
    // 完整路径的覆盖值（自带 scheme）压过 baseUrl + path。
    assert_eq!(r.url, "https://override.example.com/custom");
}

// ---- 动态变量的值进池 ---------------------------------------------
//
// 动态变量的值必须在**替换之前**并进变量池：合并规则是「动态变量的值优先于同名环境
// 变量；被停用的环境变量不影响动态变量」，替换一旦发生，池里就没有同名变量的概念了。

#[test]
fn the_dynamic_value_replaces_the_marker() {
    let env = empty_env();
    let mut ep = minimal_endpoint();
    ep.url = "/points/{{sns}}".into();
    let project = project_with_defs();
    let dynamic =
        std::collections::HashMap::from([("sns".to_string(), "[{\"code\":\"A001\"}]".to_string())]);
    let r =
        resolve_endpoint_with(&ep, Some(&env), &project, Some(&in_module_a()), &dynamic).unwrap();
    assert_eq!(r.url, "/points/[{\"code\":\"A001\"}]");
    assert!(r.missing_vars.is_empty(), "已解析的动态变量不该报缺失");
}

#[test]
fn the_dynamic_value_wins_over_the_environment_variable_of_the_same_name() {
    let mut env = empty_env();
    env.variables.insert("sns".into(), "env-value".into());
    let mut ep = minimal_endpoint();
    ep.url = "/points/{{sns}}".into();
    let project = project_with_defs();
    let dynamic = std::collections::HashMap::from([("sns".to_string(), "dyn-value".to_string())]);
    let r =
        resolve_endpoint_with(&ep, Some(&env), &project, Some(&in_module_a()), &dynamic).unwrap();
    assert_eq!(r.url, "/points/dyn-value");
}

#[test]
fn a_disabled_environment_variable_does_not_block_the_dynamic_value() {
    let mut env = empty_env();
    env.variables.insert("sns".into(), "env-value".into());
    env.disabled_variables.push("sns".into());
    let mut ep = minimal_endpoint();
    ep.url = "/points/{{sns}}".into();
    let project = project_with_defs();
    let dynamic = std::collections::HashMap::from([("sns".to_string(), "dyn-value".to_string())]);
    let r =
        resolve_endpoint_with(&ep, Some(&env), &project, Some(&in_module_a()), &dynamic).unwrap();
    assert_eq!(r.url, "/points/dyn-value");
    assert!(r.missing_vars.is_empty());
}

#[test]
fn a_disabled_environment_variable_stays_unresolved() {
    let mut env = empty_env();
    env.variables.insert("token".into(), "abc".into());
    env.disabled_variables.push("token".into());
    let mut ep = minimal_endpoint();
    ep.url = "/x?token={{token}}".into();
    let project = project_with_defs();
    let r = resolve_endpoint_with(
        &ep,
        Some(&env),
        &project,
        Some(&in_module_a()),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    assert_eq!(r.url, "/x?token={{token}}");
    assert_eq!(r.missing_vars, vec!["token".to_string()]);
}

#[test]
fn the_four_argument_form_behaves_as_before() {
    // 保留 4 参包装：没有动态变量时行为与从前一致。
    let env = empty_env();
    let mut ep = minimal_endpoint();
    ep.url = "/x/{{a}}".into();
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "/x/{{a}}");
    assert_eq!(r.missing_vars, vec!["a".to_string()]);
}

// ---- 路径参数 -------------------------------------------------------
//
// URL 路径里的 `{占位符}` 取端点自己的路径参数：替换进路径本身，不进查询串。

/// 一个带路径参数的端点。
fn endpoint_with_path_param(url: &str, key: &str, value: &str) -> EndpointDetail {
    let mut ep = minimal_endpoint();
    ep.url = url.into();
    ep.path_params = vec![KeyValue {
        id: "pp1".into(),
        key: key.into(),
        value: value.into(),
        enabled: true,
        description: None,
    }];
    ep
}

#[test]
fn resolve_endpoint_substitutes_a_path_parameter_into_the_path() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = endpoint_with_path_param("/users/{id}", "id", "42");
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/42");
    assert!(r.missing_path_params.is_empty());
}

#[test]
fn resolve_endpoint_reports_a_path_parameter_without_a_value() {
    // 缺值的占位符原样留在 URL 里，并被报出来 —— 发送那侧据此拦下请求。
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = minimal_endpoint();
    ep.url = "/users/{id}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/{id}");
    assert_eq!(r.missing_path_params, vec!["id".to_string()]);
}

#[test]
fn resolve_endpoint_encodes_a_path_parameter_value() {
    // 与查询参数同一套字符集：空格是 %20，斜杠是 %2F。
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = endpoint_with_path_param("/files/{name}", "name", "a b/c");
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/files/a%20b%2Fc");
}

#[test]
fn resolve_endpoint_keeps_a_query_parameter_out_of_the_path() {
    // 同名的查询参数仍然进查询串：路径参数与查询参数是两份数据。
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    let mut ep = endpoint_with_path_param("/users/{id}", "id", "42");
    ep.base_url_definition_id = Some("data".into());
    ep.params = vec![KeyValue {
        id: "p1".into(),
        key: "id".into(),
        value: "7".into(),
        enabled: true,
        description: None,
    }];
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/42?id=7");
}

#[test]
fn resolve_endpoint_treats_an_unresolved_variable_in_a_path_parameter_as_missing() {
    // 值里的 `{{var}}` 解析不出来时，这个路径参数算「还没准备好」：
    // 编码过的 `%7B%7B…%7D%7D` 塞进路径，用户看不出缺的是哪个变量。
    let env = empty_env();
    let ep = endpoint_with_path_param("/users/{id}", "id", "{{uid}}");
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert_eq!(r.url, "/users/{id}");
    assert_eq!(r.missing_path_params, vec!["id".to_string()]);
    assert_eq!(r.missing_vars, vec!["uid".to_string()]);
}
