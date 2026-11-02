//! base URL 的解析、代理前缀替换、拼接与斜杠收拢。

use super::base_url::{
    apply_proxy, collapse_url_slashes, join_base_url, resolve_base_url, BaseUrlSource,
};
use super::resolve::resolve_endpoint;
use super::test_support::{
    empty_env, env_with_two_modules, in_module_a, minimal_endpoint, set_cell_with_proxy, MOD_A,
    MOD_B,
};

// ---- 解析 ------------------------------------------------------------

#[test]
fn the_module_cell_resolves_the_endpoints_own_address() {
    let env = env_with_two_modules();
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "https://mod-a.example.com");
    assert_eq!(got.proxy, None);
    assert_eq!(got.source, BaseUrlSource::Module);
}

#[test]
fn two_modules_resolve_different_hosts() {
    // 模块之间没有继承：MOD_B 的地址不能被 MOD_A 那格顶替。

    let env = env_with_two_modules();
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_B));
    assert_eq!(got.base_url, "https://mod-b.example.com");
    assert_eq!(got.source, BaseUrlSource::Module);
}

#[test]
fn a_cell_with_a_proxy_reports_both_addresses() {
    let mut env = empty_env();
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "https://gw.corp.com/api/xxxx/a",
    );
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "http://svc.internal:8080/api/a");
    assert_eq!(got.proxy.as_deref(), Some("https://gw.corp.com/api/xxxx/a"));
    assert_eq!(got.source, BaseUrlSource::Module);
}

#[test]
fn no_module_context_resolves_to_nothing() {
    // 调用方没说端点在哪个模块，所以没有格子可读。找不到格子并不构成去读别的模块的理由。
    let env = env_with_two_modules();
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), None);
    assert_eq!(got.base_url, "");
    assert_eq!(got.source, BaseUrlSource::None);
}

#[test]
fn an_empty_cell_does_not_fall_back_to_another_module() {
    // MOD_B 有地址而 MOD_A 没有。借用 MOD_B 的会把一个 `air` 请求发到 `water` 的 host 上。
    let env = env_with_two_modules();
    let mut env2 = env.clone();
    env2.module_base_urls.remove(MOD_A);
    let got = resolve_base_url(&minimal_endpoint(), Some(&env2), Some(MOD_A));
    assert_eq!(got.base_url, "");
    assert_eq!(got.source, BaseUrlSource::None);
}

#[test]
fn a_cell_with_only_a_proxy_resolves_to_nothing() {
    // 没有地址就没有可替换的前缀，代理因此也不参与解析。
    let mut env = empty_env();
    set_cell_with_proxy(&mut env, MOD_A, "", "https://gw.corp.com");
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "");
    assert_eq!(got.proxy, None);
    assert_eq!(got.source, BaseUrlSource::None);
}

#[test]
fn no_env_resolves_to_nothing() {
    // 没有选环境，就没有那张 map 可读。
    let got = resolve_base_url(&minimal_endpoint(), None, Some(MOD_A));
    assert_eq!(got.base_url, "");
    assert_eq!(got.source, BaseUrlSource::None);
}

#[test]
fn a_per_endpoint_override_still_beats_the_module_cell() {
    // 自定义模式始终是最强信号：URL 字段里已经是完整地址，不能再前置模块的地址。
    let env = env_with_two_modules();
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("".to_string());
    let got = resolve_base_url(&ep, Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "");
    assert_eq!(got.proxy, None);
    assert_eq!(got.source, BaseUrlSource::Override);
}

#[test]
fn a_per_endpoint_override_does_not_pick_up_the_proxy() {
    // 自定义意味着「地址由我自己填」，代理那一层不参与。
    let mut env = empty_env();
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "https://gw.corp.com/api/xxxx/a",
    );
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    let got = resolve_base_url(&ep, Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "https://custom.example.com");
    assert_eq!(got.proxy, None);
    assert_eq!(got.source, BaseUrlSource::Override);
}

// ---- resolve_endpoint -------------------------------------------------

#[test]
fn resolve_endpoint_applies_the_module_from_overrides() {
    // MCP 工具经 `EndpointOverrides` 传模块而不是用参数，所以这才是 daemon
    // 真正跑到的形状。

    let env = env_with_two_modules();
    let r =
        resolve_endpoint(&minimal_endpoint(), Some(&env), Some(&in_module_a())).expect("resolves");
    assert!(
        r.url.starts_with("https://mod-a.example.com"),
        "got {}",
        r.url
    );
}

#[test]
fn resolve_endpoint_without_module_context_has_no_base_url() {
    let env = env_with_two_modules();
    let r = resolve_endpoint(&minimal_endpoint(), Some(&env), None).expect("resolves");
    assert_eq!(r.url, "/users/{{id}}", "no module, no cell, no prefix");
}

#[test]
fn resolve_endpoint_replaces_the_module_address_with_the_proxy() {
    // 端到端：模块那一格是后端 api 地址，请求发出去时换成实际访问地址。
    let mut env = empty_env();
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "https://gw.corp.com/api/xxxx/a",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/orders/list".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "https://gw.corp.com/api/xxxx/a/orders/list");
}

#[test]
fn resolve_endpoint_leaves_the_url_alone_when_the_proxy_does_not_match() {
    // 端点写的是另一个主机时，代理那一层不参与：它匹配的是模块地址那个前缀。
    let mut env = empty_env();
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "https://gw.corp.com/api/xxxx/a",
    );
    let mut ep = minimal_endpoint();
    ep.url = "https://other.example.com/orders".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "https://other.example.com/orders");
}

#[test]
fn the_proxy_address_gets_its_own_variable_replacement() {
    // 代理地址可能自身是模板，故它单独再替换一次；前缀匹配用的是替换后的值。
    let mut env = empty_env();
    env.variables
        .insert("gw_host".into(), "https://gw.corp.com".into());
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "{{gw_host}}/api/xxxx/a",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/orders/list".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "https://gw.corp.com/api/xxxx/a/orders/list");
    assert!(
        r.missing_vars.is_empty(),
        "代理地址里的变量解析出来了，不该报缺值：{:?}",
        r.missing_vars
    );
}

#[test]
fn an_unresolved_variable_in_the_proxy_lands_in_the_missing_report() {
    // 代理地址里的 `{{var}}` 解析不出来时，缺值报告里得有它，用户才看得见缺哪个变量。
    let mut env = empty_env();
    set_cell_with_proxy(
        &mut env,
        MOD_A,
        "http://svc.internal:8080/api/a",
        "https://gw.corp.com/{{tenant}}/api/xxxx/a",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/orders/list".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert!(
        r.missing_vars.contains(&"tenant".to_string()),
        "代理地址里缺的那个变量没进缺值报告：{:?}",
        r.missing_vars
    );
}

// ---- apply_proxy ------------------------------------------------------
#[test]
fn apply_proxy_replaces_the_prefix() {
    assert_eq!(
        apply_proxy(
            "http://svc.internal:8080/api/a",
            Some("https://gw.corp.com/api/xxxx/a"),
            "http://svc.internal:8080/api/a/orders/list",
        ),
        "https://gw.corp.com/api/xxxx/a/orders/list"
    );
}

#[test]
fn apply_proxy_without_a_proxy_changes_nothing() {
    assert_eq!(
        apply_proxy(
            "http://svc.internal:8080/api/a",
            None,
            "http://svc.internal:8080/api/a/orders/list",
        ),
        "http://svc.internal:8080/api/a/orders/list"
    );
    assert_eq!(
        apply_proxy(
            "http://svc.internal:8080/api/a",
            Some("  "),
            "http://svc.internal:8080/api/a/orders/list",
        ),
        "http://svc.internal:8080/api/a/orders/list"
    );
}

#[test]
fn apply_proxy_only_matches_on_a_path_boundary() {
    // `http://h/api` 不该命中 `http://h/apixyz`。
    assert_eq!(
        apply_proxy(
            "http://svc.internal:8080/api",
            Some("https://gw.corp.com"),
            "http://svc.internal:8080/apixyz/orders",
        ),
        "http://svc.internal:8080/apixyz/orders"
    );
}

#[test]
fn apply_proxy_matches_the_url_exactly() {
    assert_eq!(
        apply_proxy(
            "https://api.example.com",
            Some("https://gw.corp.com"),
            "https://api.example.com",
        ),
        "https://gw.corp.com"
    );
}

#[test]
fn apply_proxy_needs_a_base_url_to_match_against() {
    assert_eq!(
        apply_proxy("", Some("https://gw.corp.com"), "/orders"),
        "/orders"
    );
}

// ---- join_base_url ---------------------------------------------------

#[test]
fn join_base_url_strips_trailing_slash() {
    assert_eq!(
        join_base_url("https://api.example.com/", "/users"),
        "https://api.example.com/users"
    );
}

#[test]
fn join_base_url_full_path_in_path_wins() {
    assert_eq!(
        join_base_url("https://api.example.com", "https://other/foo"),
        "https://other/foo"
    );
}

#[test]
fn join_base_url_no_base_returns_path() {
    assert_eq!(join_base_url("", "/users"), "/users");
}

#[test]
fn join_base_url_inserts_separator_when_missing() {
    assert_eq!(
        join_base_url("https://api.example.com", "users"),
        "https://api.example.com/users"
    );
}

// ---- collapse_url_slashes ------------------------------------------
//
// The join/substitute ordering is deliberate —an environment's own base
// URL may be templated, so `{{var}}` must resolve *after* the join to be
// able to see it. The price is that `join_base_url` inserts a separator
// before it knows what `{{var}}` becomes, and a value already starting
// with `/` turns that separator into a `//`:
// `http://host:9200/{{var}}` → `http://host:9200//api`, a different route
// to most servers. `collapse_url_slashes` is the post-substitution pass.
//
// These cases mirror `collapseUrlSlashes` in `src/lib/__tests__/url.test.ts`
// one-for-one; if the two ever disagree the preview and the request that
// actually goes out describe different URLs.

#[test]
fn collapse_url_slashes_repairs_join_boundary() {
    assert_eq!(
        collapse_url_slashes("http://host:9200//api"),
        "http://host:9200/api"
    );
    // Already clean: must be idempotent, not eat the separator.
    assert_eq!(
        collapse_url_slashes("http://host:9200/api"),
        "http://host:9200/api"
    );
    assert_eq!(
        collapse_url_slashes("http://host:9200//api/users"),
        "http://host:9200/api/users"
    );
}

#[test]
fn collapse_url_slashes_keeps_scheme_intact() {
    assert_eq!(
        collapse_url_slashes("https://host:9200//api"),
        "https://host:9200/api"
    );
    assert_eq!(
        collapse_url_slashes("HTTPS://host:9200//api"),
        "HTTPS://host:9200/api"
    );
    assert_eq!(collapse_url_slashes("http://host/api"), "http://host/api");
}

#[test]
fn collapse_url_slashes_keeps_protocol_relative_reference() {
    // 无 scheme 的 `//` 是「接下来是 host」的标记。
    assert_eq!(
        collapse_url_slashes("//cdn.example.com//x"),
        "//cdn.example.com/x"
    );
}

#[test]
fn collapse_url_slashes_leaves_query_and_fragment_alone() {
    // `//` inside a query value is payload (a nested URL), not a path.
    assert_eq!(
        collapse_url_slashes("http://host/x?redirect=https://a//b#frag//ment"),
        "http://host/x?redirect=https://a//b#frag//ment"
    );
    // 同一个 URL 的路径那一侧仍会收连斜杠。
    assert_eq!(
        collapse_url_slashes("http://host//x?a=1"),
        "http://host/x?a=1"
    );
}

#[test]
fn collapse_url_slashes_empty_input() {
    assert_eq!(collapse_url_slashes(""), "");
}
