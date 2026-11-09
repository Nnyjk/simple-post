//! base URL 的解析、路径前缀改写、拼接与斜杠收拢。

use simple_post_core::domain::RewriteRule;

use super::base_url::{
    apply_rewrite, collapse_url_slashes, join_base_url, resolve_base_url, BaseUrlSource,
};
use super::resolve::resolve_endpoint;
use super::test_support::{
    empty_env, env_with_two_modules, in_module_a, minimal_endpoint, set_cell_with_rewrite, MOD_A,
    MOD_B,
};

// ---- 解析 ------------------------------------------------------------

#[test]
fn the_module_cell_resolves_the_endpoints_own_address() {
    let env = env_with_two_modules();
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "https://mod-a.example.com");
    assert_eq!(got.rewrite, None);
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
fn a_cell_with_a_rewrite_reports_both_the_address_and_the_rule() {
    let mut env = empty_env();
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/project-a/air",
    );
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "127.0.0.1:8080");
    assert_eq!(
        got.rewrite,
        Some(RewriteRule {
            from: "/api/air".into(),
            to: "/api/project-a/air".into(),
        })
    );
    assert_eq!(got.source, BaseUrlSource::Module);
}

#[test]
fn no_module_context_resolves_to_nothing() {
    // 调用方没说端点在哪个模块，所以没有格子可读。找不到格子并不构成去读别的模块的理由。
    let env = env_with_two_modules();
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), None);
    assert_eq!(got.base_url, "");
    assert_eq!(got.rewrite, None);
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
fn a_cell_with_only_a_rewrite_resolves_to_no_address() {
    // 没有地址就没有可拼接的模块地址；但改写规则仍然读得到——它改的是端点自己的路径。
    let mut env = empty_env();
    set_cell_with_rewrite(&mut env, MOD_A, "", "/api/air", "/api/project-a/air");
    let got = resolve_base_url(&minimal_endpoint(), Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "");
    assert_eq!(
        got.rewrite,
        Some(RewriteRule {
            from: "/api/air".into(),
            to: "/api/project-a/air".into(),
        })
    );
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
    assert_eq!(got.rewrite, None);
    assert_eq!(got.source, BaseUrlSource::Override);
}

#[test]
fn a_per_endpoint_override_still_gets_the_cells_rewrite_rule() {
    // 改写只作用在端点自己的路径上，与主机是谁的无关：端点钉了自己的主机，路径照样改写。
    let mut env = empty_env();
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/project-a/air",
    );
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    let got = resolve_base_url(&ep, Some(&env), Some(MOD_A));
    assert_eq!(got.base_url, "https://custom.example.com");
    assert_eq!(
        got.rewrite,
        Some(RewriteRule {
            from: "/api/air".into(),
            to: "/api/project-a/air".into(),
        })
    );
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
fn resolve_endpoint_rewrites_the_endpoints_own_path() {
    // 端到端：模块地址是 `127.0.0.1:8080`，端点路径是 `/api/air/xxxxx`，那一格配了
    // `/api/air` → `/api/project-a/air`，发出去的是 `127.0.0.1:8080/api/project-a/air/xxxxx`。
    let mut env = empty_env();
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/project-a/air",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/api/air/xxxxx".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "127.0.0.1:8080/api/project-a/air/xxxxx");
}

#[test]
fn resolve_endpoint_leaves_the_url_alone_when_the_rule_does_not_match() {
    // 端点路径不以起点开头时，改写不参与：它匹配的是端点自己路径的开头。
    let mut env = empty_env();
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/project-a/air",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/api/water/xxxxx".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "127.0.0.1:8080/api/water/xxxxx");
}

#[test]
fn the_rewrite_target_gets_its_own_variable_replacement() {
    // 改写的目标可能自身是模板，故它进 URL 之后照常被替换一遍。
    let mut env = empty_env();
    env.variables.insert("tenant".into(), "project-a".into());
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/{{tenant}}/air",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/api/air/xxxxx".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "127.0.0.1:8080/api/project-a/air/xxxxx");
    assert!(
        r.missing_vars.is_empty(),
        "改写目标里的变量解析出来了，不该报缺值：{:?}",
        r.missing_vars
    );
}

#[test]
fn an_unresolved_variable_in_the_rewrite_target_lands_in_the_missing_report() {
    // 改写目标里的 `{{var}}` 解析不出来时，缺值报告里得有它，用户才看得见缺哪个变量。
    let mut env = empty_env();
    set_cell_with_rewrite(
        &mut env,
        MOD_A,
        "127.0.0.1:8080",
        "/api/air",
        "/api/{{tenant}}/air",
    );
    let mut ep = minimal_endpoint();
    ep.url = "/api/air/xxxxx".into();
    let r = resolve_endpoint(&ep, Some(&env), Some(&in_module_a())).expect("resolves");
    assert!(
        r.missing_vars.contains(&"tenant".to_string()),
        "改写目标里缺的那个变量没进缺值报告：{:?}",
        r.missing_vars
    );
}

// ---- apply_rewrite ----------------------------------------------------
#[test]
fn apply_rewrite_replaces_the_prefix() {
    assert_eq!(
        apply_rewrite(
            "/api/air/xxxxx",
            Some(&RewriteRule {
                from: "/api/air".into(),
                to: "/api/project-a/air".into(),
            }),
        ),
        "/api/project-a/air/xxxxx"
    );
}

#[test]
fn apply_rewrite_without_a_rule_changes_nothing() {
    assert_eq!(apply_rewrite("/api/air/xxxxx", None), "/api/air/xxxxx");
    assert_eq!(
        apply_rewrite(
            "/api/air/xxxxx",
            Some(&RewriteRule {
                from: "".into(),
                to: "/api/project-a/air".into(),
            }),
        ),
        "/api/air/xxxxx",
        "起点为空白的规则匹配不了任何东西"
    );
}

#[test]
fn apply_rewrite_only_matches_on_a_path_boundary() {
    // `/api/air` 不该命中 `/api/airplane`。
    assert_eq!(
        apply_rewrite(
            "/api/airplane/xxxxx",
            Some(&RewriteRule {
                from: "/api/air".into(),
                to: "/api/project-a/air".into(),
            }),
        ),
        "/api/airplane/xxxxx"
    );
}

#[test]
fn apply_rewrite_matches_the_url_exactly() {
    assert_eq!(
        apply_rewrite(
            "/api/air",
            Some(&RewriteRule {
                from: "/api/air".into(),
                to: "/api/project-a/air".into(),
            }),
        ),
        "/api/project-a/air"
    );
}

#[test]
fn apply_rewrite_with_an_empty_target_drops_the_prefix() {
    assert_eq!(
        apply_rewrite(
            "/api/air/xxxxx",
            Some(&RewriteRule {
                from: "/api/air".into(),
                to: "".into(),
            }),
        ),
        "/xxxxx"
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
