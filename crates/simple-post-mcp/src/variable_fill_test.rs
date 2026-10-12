use super::*;
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode, KeyValue, RequestBody,
};

/// The module the endpoint under test sits in. Two modules exist so a
/// test can prove the module selects the *cell* — under v2 the service
/// list is project-wide, so the module is no longer a filter over which
/// services exist.
const MOD_A: &str = "mod_a";
/// A second module, with its own address for the same service.
const MOD_B: &str = "mod_b";

fn empty_env() -> Environment {
    Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        module_base_urls: HashMap::new(),
        variables: HashMap::new(),
        is_active: true,
    }
}

/// Write one `(environment × service × module)` cell.
fn set_cell(env: &mut Environment, def_id: &str, module: &str, url: &str) {
    env.module_base_urls
        .entry(def_id.to_string())
        .or_default()
        .insert(module.to_string(), url.to_string());
}

/// The project's service list, project-scoped and flat, with `data`
/// carrying the ★.
fn project_with_defs() -> Project {
    Project {
        id: "p1".into(),
        name: "demo".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![
            simple_post_core::domain::BaseUrlDefinition {
                id: "auth".into(),
                name: "auth".into(),
                description: None,
            },
            simple_post_core::domain::BaseUrlDefinition {
                id: "data".into(),
                name: "data".into(),
                description: None,
            },
        ],
        default_base_url_definition_id: Some("data".into()),
        color: "#000".into(),
        created_at: 0,
        updated_at: 0,
    }
}

/// Overrides carrying only the module context, for the tests that are
/// about substitution and need a resolvable base URL but no draft.
fn in_module_a() -> EndpointOverrides {
    EndpointOverrides {
        module_id: Some(MOD_A.to_string()),
        ..Default::default()
    }
}

fn minimal_endpoint() -> EndpointDetail {
    EndpointDetail {
        node_id: "ep1".into(),
        method: HttpMethod::Get,
        url: "/users/{{id}}".into(),
        environment_id: Some("e1".into()),
        base_url_definition_id: None,
        base_url_override: None,
        params: Vec::new(),
        headers: Vec::new(),
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
    }
}

// ---- project-scoped services, per-module values ---------------
//
// 服务应该绑定到项目：the service list belongs to the project and every
// module sees all of it. What is per-module is the *value* — the same
// service can point at two different hosts for two modules, and an
// empty cell is a real answer rather than a hole to fall through.

/// An environment holding a distinct URL per (service × module), so a
/// wrong module resolving cannot pass as a right answer.
fn env_with_two_modules() -> Environment {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://mod-a.example.com");
    set_cell(&mut env, "data", MOD_B, "https://mod-b.example.com");
    set_cell(&mut env, "auth", MOD_A, "https://mod-a-auth.example.com");
    env
}

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
    // The service is shared; only its address is per-module. This is the
    // test that fails if `module_id` is ever read as a filter over which
    // services exist instead of a key into the value layer.
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
    // Covers a deleted service, an id copied from another project, and a
    // stale endpoint pointer alike. Falling back to the ★would aim the
    // request at a host the user did not pick.
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
    // The caller's own pick is checked against the same project list.
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
    // NULL is what a freshly created project has. Its endpoints go out
    // with no base URL rather than borrowing any service.
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
    // The caller did not say which module the endpoint is in, so there
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
    // `data` has a URL for MOD_B but none for MOD_A. Borrowing MOD_B's
    // would send an `air` request to `water`'s host — the mistake the
    // whole (environment × module × service) shape exists to prevent.
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
    // `Environment.baseUrls` is the dead layer `db::migrate` emptied. A
    // value left there must not resolve, or the shape-B migration would
    // keep serving through a path nothing writes.
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
    // The MCP tools pass the module through `EndpointOverrides` rather
    // than as a parameter, so this is the shape the daemon actually
    // exercises.
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
    // 自定义mode is the strongest signal and has always been; nothing
    // below it may retarget a request whose URL field carries the whole
    // address.
    let env = env_with_two_modules();
    let project = project_with_defs();
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("".to_string());
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::Override);
}
// ---- resolve_template --------------------------------------------

#[test]
fn resolve_template_hits_known_var() {
    let mut vars = HashMap::new();
    vars.insert("name".into(), "alice".into());
    let r = resolve_template("hello {{name}}", &vars);
    assert_eq!(r.resolved, "hello alice");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_unknown_leaves_marker() {
    let vars = HashMap::new();
    let r = resolve_template("hi {{who}}", &vars);
    assert_eq!(r.resolved, "hi {{who}}");
    assert_eq!(r.missing, vec!["who"]);
}

#[test]
fn resolve_template_dedupes_missing_list() {
    let vars = HashMap::new();
    let r = resolve_template("{{x}}/{{x}}/{{y}}", &vars);
    assert_eq!(r.resolved, "{{x}}/{{x}}/{{y}}");
    assert_eq!(r.missing, vec!["x", "y"]);
}

#[test]
fn resolve_template_empty_input_returns_empty() {
    let vars = HashMap::new();
    let r = resolve_template("", &vars);
    assert_eq!(r.resolved, "");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_chinese_var_name_and_value() {
    let mut vars = HashMap::new();
    vars.insert("名字".into(), "小明".into());
    // Note: `\w+` is `[A-Za-z0-9_]+`, so non-ASCII names don't match
    // the marker syntax. The marker passes through verbatim.
    let r = resolve_template("hi {{名字}}", &vars);
    assert_eq!(r.resolved, "hi {{名字}}");
    // Whitespace inside the marker is also not a word.
    let r = resolve_template("hi {{ name }}", &vars);
    assert_eq!(r.resolved, "hi {{ name }}");
}

#[test]
fn resolve_template_empty_value_is_kept() {
    // An empty-string entry IS a hit; the substitution must produce
    // an empty segment, not leave the marker.
    let mut vars = HashMap::new();
    vars.insert("empty".into(), "".into());
    let r = resolve_template("x={{empty}}!", &vars);
    assert_eq!(r.resolved, "x=!");
    assert!(r.missing.is_empty());
}

#[test]
fn resolve_template_doubled_marker_does_not_recurse() {
    // "No recursion" property: a substitution result that itself
    // contains `{{...}}` markers must NOT be re-scanned in the
    // same pass.
    //
    // The single-pass scanner finds the first `}}` it sees in
    // `{{{{a}}}}`, considers the interior `{{a` a malformed token
    // (because it contains `{`), and emits the marker verbatim. So
    // the assertion below confirms two things at once:
    //   1. Substituting `a = "{{b}}"` for an outer `{{a}}` produces
    //      a string that still contains `{{b}}` (no second scan
    //      pulled it in).
    //   2. Running resolve_template on the already-expanded string
    //      does NOT behave any differently —proving the resolver
    //      is a pure single pass, idempotent on its own output.
    let mut vars = HashMap::new();
    vars.insert("a".into(), "{{b}}".into());
    let r = resolve_template("{{a}}", &vars);
    assert_eq!(r.resolved, "{{b}}");
    assert!(r.missing.is_empty(), "missing = {:?}", r.missing);

    // And the doubled-up case stays verbatim —the first `}}`
    // wins, the marker is left untouched, and the input does not
    // mutate at all.
    let mut vars2 = HashMap::new();
    vars2.insert("a".into(), "X".into());
    let r2 = resolve_template("{{{{a}}}}", &vars2);
    assert_eq!(r2.resolved, "{{{{a}}}}");
    assert!(r2.missing.is_empty());
    // No second pass swallowed the inner braces either.
    assert!(!r2.resolved.contains("X"));
}

// ---- list_missing_vars -------------------------------------------

#[test]
fn list_missing_vars_dedupes_and_preserves_order() {
    let v = list_missing_vars("{{a}}-{{b}}-{{a}}/{{c}}");
    assert_eq!(v, vec!["a", "b", "c"]);
}

#[test]
fn list_missing_vars_ignores_non_word_markers() {
    let v = list_missing_vars("{{ with space }}{{ a.b }}{{ok}}");
    assert_eq!(v, vec!["ok"]);
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
    // Sanity: without the override the project ★does apply.
    let (url2, src2, _) = resolve_base_url_with(&ep, Some(&env), &project, None, Some(MOD_A));
    assert_eq!(url2, "https://data.example.com");
    assert_eq!(src2, BaseUrlSource::Default);
}

/// A per-endpoint hard host stays the strongest signal even when the
/// caller also names a definition.
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

    // Sanity: clearing the override restores the project ★
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
    // `//` with no scheme is the marker that says "next is a host".
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
    // Still collapses the path side of the same URL.
    assert_eq!(
        collapse_url_slashes("http://host//x?a=1"),
        "http://host/x?a=1"
    );
}

#[test]
fn collapse_url_slashes_empty_input() {
    assert_eq!(collapse_url_slashes(""), "");
}

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
    // Collapsing removes *duplicates*, never the single slash the join
    // contributed —otherwise `api` would be glued onto the host.
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
    // signal the caller gets that the URL is incomplete.
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
    // The collapse runs before the query append and must not eat a query
    // string already present —the `?`/`&` choice reads the URL.
    assert_eq!(r.url, "http://host:9200/api?foo=bar&page=1");
}

// ---- resolve_endpoint baseUrl precedence -------------------------

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
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // URL + headers + body + auth missing vars are unioned, deduped,
    // and ordered by first appearance.
    assert!(r.missing_vars.contains(&"user_id".to_string()));
    assert!(r.missing_vars.contains(&"trace_id".to_string()));
    assert!(r.missing_vars.contains(&"payload".to_string()));
    assert!(r.missing_vars.contains(&"token".to_string()));
    // And the unresolved markers stayed put (we can see them in the
    // body string and URL):
    assert!(r.url.contains("{{user_id}}"));
    assert!(r.body.unwrap().contains("{{payload}}"));
}

// ---- auth injection ----------------------------------------------

#[test]
fn resolve_endpoint_bearer_auth_injects_authorization_header() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "secret-xyz".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{token}}".into()),
        basic: None,
        apikey: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .expect("Authorization header must be present");
    assert_eq!(auth.1, "Bearer secret-xyz");
}

#[test]
fn resolve_endpoint_basic_auth_injects_authorization_header() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("u".into(), "alice".into());
    env.variables.insert("p".into(), "wonder".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.auth = AuthConfig {
        kind: AuthType::Basic,
        bearer: None,
        basic: Some(BasicAuth {
            username: "{{u}}".into(),
            password: "{{p}}".into(),
        }),
        apikey: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .expect("Authorization header must be present");
    // "alice:wonder" base64 → "YWxpY2U6d29uZGVy"
    assert!(auth.1.starts_with("Basic "));
    assert!(auth.1.contains("YWxpY2U6d29uZGVy"));
}

#[test]
fn resolve_endpoint_apikey_header_injects_custom_header() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "ABC123".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.auth = AuthConfig {
        kind: AuthType::Apikey,
        bearer: None,
        basic: None,
        apikey: Some(ApiKeyAuth {
            key: "X-API-Key".into(),
            value: "{{k}}".into(),
            in_location: ApiKeyLocation::Header,
        }),
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    let hit = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
        .expect("apikey header must be present");
    assert_eq!(hit.1, "ABC123");
    // And the URL must NOT carry the apikey in the query.
    assert!(!r.url.contains("ABC123"));
}

#[test]
fn resolve_endpoint_apikey_query_appends_to_url() {
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("k".into(), "Q-VAL".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.auth = AuthConfig {
        kind: AuthType::Apikey,
        bearer: None,
        basic: None,
        apikey: Some(ApiKeyAuth {
            key: "access_token".into(),
            value: "{{k}}".into(),
            in_location: ApiKeyLocation::Query,
        }),
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    assert!(r.url.contains("access_token=Q-VAL"));
    // And there's NO Authorization header.
    assert!(r
        .headers
        .iter()
        .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")));
    // And the URL doesn't carry an apikey-shaped header.
    assert!(r
        .headers
        .iter()
        .all(|(k, _)| !k.eq_ignore_ascii_case("access_token")));
}

#[test]
fn resolve_endpoint_user_authorization_wins_over_auth_config() {
    // If the user already declared an Authorization header, the
    // auth-config-derived one must NOT replace it (matches the
    // TS behavior —fetch keeps the last value of a record key,
    // cURL would otherwise emit two `-H` lines for the same name).
    let mut env = empty_env();
    set_cell(&mut env, "data", MOD_A, "https://api.example.com");
    env.variables.insert("token".into(), "from-env".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.headers = vec![KeyValue {
        id: "h1".into(),
        key: "Authorization".into(),
        value: "Bearer user-set".into(),
        enabled: true,
        description: None,
    }];
    ep.auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{token}}".into()),
        basic: None,
        apikey: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, Some(&in_module_a())).unwrap();
    // The user's "Bearer user-set" should win.
    let auth = r
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .unwrap();
    assert_eq!(auth.1, "Bearer user-set");
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
    assert!(r.url.contains("q=hello+world"));
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
    // Full-path override (with scheme) wins over baseUrl + path.
    assert_eq!(r.url, "https://override.example.com/custom");
}
