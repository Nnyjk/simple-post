//! base URL 的解析优先级、拼接与斜杠收拢。

use super::base_url::{
    collapse_url_slashes, join_base_url, resolve_base_url, resolve_base_url_with, BaseUrlSource,
};
use super::resolve::resolve_endpoint;
use super::test_support::{
    empty_env, env_with_two_modules, in_module_a, minimal_endpoint, project_with_defs, set_cell,
    MOD_A, MOD_B,
};
// ---- project-scoped services, per-module values ---------------
//
// 服务应该绑定到项目，每个模块都看到全部。按模块区分的是**取值** —— 同一个服务可以在
// 两个模块指向不同 host，而空格子是一个真实答案，不是可以穿过去的洞。

#[test]
fn the_project_star_resolves_the_endpoints_own_module_cell() {
    let env = env_with_two_modules();
    let project = project_with_defs(); // ★ = "data"
    let (url, src, def) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "https://mod-a.example.com");
    assert_eq!(src, BaseUrlSource::Default);
    assert_eq!(def.as_deref(), Some("data"));
}

#[test]
fn the_same_service_resolves_a_different_host_per_module() {
    // 服务是共享的，按模块区分的只有它的地址。这个测试会在 `module_id` 被当成
    // 「哪些服务存在」的过滤器（而不是取值层的键）时失败。

    let env = env_with_two_modules();
    let project = project_with_defs();
    let (url, src, def) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, Some(MOD_B));
    assert_eq!(url, "https://mod-b.example.com");
    assert_eq!(src, BaseUrlSource::Default);
    assert_eq!(def.as_deref(), Some("data"));
}

#[test]
fn a_definition_id_the_project_does_not_hold_resolves_to_nothing() {
    // 同一批覆盖服务已删、从别的项目复制来的 id、以及端点上的指针过期。
    // 回退到 ★会把请求打到用户没选过的 host 上。

    let env = env_with_two_modules();
    let project = project_with_defs();
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("ghost".into());
    let (url, src, def) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
    assert_eq!(def, None);
}

#[test]
fn a_forced_definition_the_project_does_not_hold_is_also_refused() {
    // 调用方自己点名的那一个，同样要过这份项目列表的校验。
    let env = env_with_two_modules();
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(
        &minimal_endpoint(),
        Some(&env),
        &project,
        Some("ghost"),
        Some(MOD_A),
    );
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn a_project_with_no_star_resolves_to_nothing() {
    // NULL 正是新建项目的状态。它的端点会不带 base URL 发出，而不是借用任何服务。

    let env = env_with_two_modules();
    let mut project = project_with_defs();
    project.default_base_url_definition_id = None;
    let (url, src, _) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn no_module_context_resolves_to_nothing() {
    // 调用方没说端点在哪个模块，所以没有格子可读 ——
    // is no cell to read — and no cell is not a licence to read another
    // module's.
    let env = env_with_two_modules();
    let project = project_with_defs();
    let (url, src, _) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, None);
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn an_empty_cell_does_not_fall_back_to_another_module() {
    // `data` 给 MOD_B 配了 URL 而没给 MOD_A 配。借用 MOD_B 的
    // 会把一个 `air` 请求发到 `water` 的 host 上 —— 这正是
    // 整个 (环境 × 模块 × 服务) 形状要防的错误。
    let mut env = env_with_two_modules();
    env.module_base_urls.get_mut("data").unwrap().remove(MOD_A);
    let project = project_with_defs();
    let (url, src, _) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn the_deprecated_base_urls_column_is_never_read() {
    // `Environment.baseUrls` 是 `db::migrate` 已清空的那层死列。留在那里的
    // 值不该解析成功，否则 shape-B 迁移后就仍会走一条没人写的路径发请求。

    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://stale.example.com".into());
    let project = project_with_defs();
    let (url, src, _) =
        resolve_base_url_with(&minimal_endpoint(), Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "", "the dead baseUrls column must not resolve");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn resolve_endpoint_applies_the_module_from_overrides() {
    // MCP 工具经 `EndpointOverrides` 传模块而不是用参数，所以这才是 daemon
    // 真正跑到的形状。

    let env = env_with_two_modules();
    let project = project_with_defs();
    let r = resolve_endpoint(
        &minimal_endpoint(),
        Some(&env),
        &project,
        Some(&in_module_a()),
    )
    .expect("resolves");
    assert!(
        r.url.starts_with("https://mod-a.example.com"),
        "got {}",
        r.url
    );
}

#[test]
fn resolve_endpoint_without_module_context_has_no_base_url() {
    let env = env_with_two_modules();
    let project = project_with_defs();
    let r = resolve_endpoint(&minimal_endpoint(), Some(&env), &project, None).expect("resolves");
    assert_eq!(r.url, "/users/{{id}}", "no module, no cell, no prefix");
}

#[test]
fn a_per_endpoint_override_still_beats_the_service_chain() {
    // 自定义模式始终是最强信号；它之下的任何一层都不得改投一个 URL 字段里
    // 已经是完整地址的请求。

    let env = env_with_two_modules();
    let project = project_with_defs();
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("".to_string());
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::Override);
}

// ---- resolve_base_url --------------------------------------------

/// The renderer's base-URL picker is ephemeral: it commits to the DB
/// on change, but `resolve_endpoint` may still be handed a different
/// choice (a draft, or a caller that resolved a default itself). The
/// override has to win over the stored choice.
#[test]
fn resolve_base_url_forced_definition_wins() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    set_cell(&mut env, "auth", MOD_A, "https://auth.example.com");
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some("auth"), Some(MOD_A));
    assert_eq!(url, "https://auth.example.com");
    assert_eq!(src, BaseUrlSource::Definition);
}

/// An empty string means "no base URL" —the renderer sends that when
/// the user cleared the picker. It must NOT silently fall back to the
/// project ★ or clearing the picker would do nothing.
#[test]
fn resolve_base_url_empty_forced_id_means_none() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let project = project_with_defs(); // ★ = "data"
    let ep = minimal_endpoint();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some(""), Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
    // 对照：没有覆盖值时项目 ★确实生效。
    let (url2, src2, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url2, "https://data.example.com");
    assert_eq!(src2, BaseUrlSource::Default);
}

/// 即便调用方同时点名了一个定义，每端点的硬 host 仍是最强信号。

#[test]
fn resolve_base_url_endpoint_host_still_wins_over_forced() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some("data"), Some(MOD_A));
    assert_eq!(url, "https://custom.example.com");
    assert_eq!(src, BaseUrlSource::Override);
}

#[test]
fn resolve_base_url_override_wins() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    // No module needed: an explicit per-endpoint host is decided before
    // any service is looked up.
    let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
    assert_eq!(url, "https://custom.example.com");
    assert_eq!(src, BaseUrlSource::Override);
}

#[test]
fn resolve_base_url_empty_override_is_a_choice_not_an_absence() {
    // 自定义with no host typed yet. The renderer writes `Some("")` to
    // *record the mode*; only `None` clears it. If an empty override fell
    // through to the service chain, selecting 自定义would silently
    // resolve the project's base URL again —the exact "the click did
    // nothing" bug on the Rust side.
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some(String::new());
    let project = project_with_defs(); // ★ = "data"
    let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::Override);

    // End to end: the URL field's whole address survives the join, with
    // no base URL prepended —including a path that carries no scheme.
    let mut ep2 = minimal_endpoint();
    ep2.base_url_override = Some(String::new());
    ep2.url = "https://api.other.com/users/1".into();
    let r = resolve_endpoint(&ep2, Some(&env), &project, Some(&in_module_a())).expect("resolves");
    assert_eq!(r.url, "https://api.other.com/users/1");

    let mut ep3 = minimal_endpoint();
    ep3.base_url_override = Some(String::new());
    ep3.url = "/users/1".into();
    let r2 = resolve_endpoint(&ep3, Some(&env), &project, Some(&in_module_a())).expect("resolves");
    assert_eq!(r2.url, "/users/1");

    // 对照：清掉覆盖值后项目 ★回来
    let cleared = minimal_endpoint();
    let (url2, src2, _) = resolve_base_url_with(&cleared, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url2, "https://data.example.com");
    assert_eq!(src2, BaseUrlSource::Default);
}

#[test]
fn resolve_base_url_definition_chosen() {
    let mut env = empty_env();
    set_cell(&mut env, "auth", MOD_A, "https://auth.example.com");
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("auth".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "https://auth.example.com");
    assert_eq!(src, BaseUrlSource::Definition);
}

#[test]
fn resolve_base_url_falls_back_to_the_project_star() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://data.example.com");
    let ep = minimal_endpoint(); // base_url_definition_id = None
    let project = project_with_defs(); // ★ = "data"
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "https://data.example.com");
    assert_eq!(src, BaseUrlSource::Default);
}

#[test]
fn resolve_base_url_no_env_returns_empty() {
    // No environment selected at all, so there is no `moduleBaseUrls` map
    // to read and no value to resolve — even though the service and the
    // project ★ both exist.
    let project = project_with_defs();
    let ep = minimal_endpoint();
    let (u, src, _) = resolve_base_url_with(&ep, None, &project, None, Some(MOD_A));
    assert_eq!(u, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn resolve_base_url_env_lacks_value_for_this_module() {
    // The service is named and the project holds it, but this module has
    // no cell for it —the slot exists and is empty, which is the same
    // answer as naming nothing.
    let env = empty_env();
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("auth".into());
    let project = project_with_defs();
    let (u, src, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(u, "");
    assert_eq!(src, BaseUrlSource::None);
}

// ---- join_base_url -----------------------------------------------

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
