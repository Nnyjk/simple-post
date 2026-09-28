use super::*;
use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode, KeyValue, RequestBody,
};

fn empty_env() -> Environment {
    Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: HashMap::new(),
        variables: HashMap::new(),
        is_active: true,
    }
}

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

fn minimal_endpoint() -> Endpoint {
    Endpoint {
        id: "ep1".into(),
        collection_id: "c1".into(),
        name: "demo".into(),
        description: None,
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
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    }
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
    //      does NOT behave any differently — proving the resolver
    //      is a pure single pass, idempotent on its own output.
    let mut vars = HashMap::new();
    vars.insert("a".into(), "{{b}}".into());
    let r = resolve_template("{{a}}", &vars);
    assert_eq!(r.resolved, "{{b}}");
    assert!(r.missing.is_empty(), "missing = {:?}", r.missing);

    // And the doubled-up case stays verbatim — the first `}}`
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
/// override has to win over the stored definition.
#[test]
fn resolve_base_url_forced_definition_wins() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    env.base_urls
        .insert("auth".into(), "https://auth.example.com".into());
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some("auth"));
    assert_eq!(url, "https://auth.example.com");
    assert_eq!(src, BaseUrlSource::Definition);
}

/// An empty string means "no base URL" — the renderer sends that when
/// the user cleared the picker. It must NOT silently fall back to the
/// project default, or clearing the picker would do nothing.
#[test]
fn resolve_base_url_empty_forced_id_means_none() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    let project = project_with_defs(); // default = "data"
    let ep = minimal_endpoint();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some(""));
    assert_eq!(url, "");
    assert_eq!(src, BaseUrlSource::None);
    // Sanity: without the override the default does apply.
    let (url2, src2, _) = resolve_base_url_with(&ep, Some(&env), &project, None);
    assert_eq!(url2, "https://data.example.com");
    assert_eq!(src2, BaseUrlSource::Default);
}

/// A per-endpoint hard host stays the strongest signal even when the
/// caller also names a definition.
#[test]
fn resolve_base_url_endpoint_host_still_wins_over_forced() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url_with(&ep, Some(&env), &project, Some("data"));
    assert_eq!(url, "https://custom.example.com");
    assert_eq!(src, BaseUrlSource::Override);
}

#[test]
fn resolve_base_url_override_wins() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    let mut ep = minimal_endpoint();
    ep.base_url_override = Some("https://custom.example.com".into());
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
    assert_eq!(url, "https://custom.example.com");
    assert_eq!(src, BaseUrlSource::Override);
}

#[test]
fn resolve_base_url_definition_chosen() {
    let mut env = empty_env();
    env.base_urls
        .insert("auth".into(), "https://auth.example.com".into());
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("auth".into());
    let project = project_with_defs();
    let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
    assert_eq!(url, "https://auth.example.com");
    assert_eq!(src, BaseUrlSource::Definition);
}

#[test]
fn resolve_base_url_falls_back_to_project_default() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    let ep = minimal_endpoint(); // base_url_definition_id = None
    let project = project_with_defs(); // default = "data"
    let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
    assert_eq!(url, "https://data.example.com");
    assert_eq!(src, BaseUrlSource::Default);
}

#[test]
fn resolve_base_url_no_env_returns_empty() {
    let project = project_with_defs();
    let ep = minimal_endpoint();
    let (u, src, _) = resolve_base_url(&ep, None, &project);
    assert_eq!(u, "");
    assert_eq!(src, BaseUrlSource::None);
}

#[test]
fn resolve_base_url_env_lacks_value_for_base_url() {
    // def id is known, env is selected, but env doesn't carry a
    // value for it — return None.
    let env = empty_env();
    let mut ep = minimal_endpoint();
    ep.base_url_definition_id = Some("auth".into());
    let project = project_with_defs();
    let (u, src, _) = resolve_base_url(&ep, Some(&env), &project);
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

// ---- resolve_endpoint baseUrl precedence -------------------------

#[test]
fn resolve_endpoint_picks_override_over_definition() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://data.example.com".into());
    let mut ep = minimal_endpoint();
    ep.url = "/users".into();
    ep.base_url_definition_id = Some("data".into());
    ep.base_url_override = Some("https://override.example.com".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert_eq!(r.url, "https://override.example.com/users");
}

#[test]
fn resolve_endpoint_substitutes_vars_in_url_path() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
    env.variables.insert("id".into(), "42".into());
    let mut ep = minimal_endpoint();
    ep.url = "/users/{{id}}".into();
    ep.base_url_definition_id = Some("data".into());
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert_eq!(r.url, "https://api.example.com/users/42");
    assert!(r.missing_vars.is_empty());
}

#[test]
fn resolve_endpoint_collects_missing_vars_across_url_headers_body_auth() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
            in_: ApiKeyLocation::Header,
        }),
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
            in_: ApiKeyLocation::Query,
        }),
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    // TS behavior — fetch keeps the last value of a record key,
    // cURL would otherwise emit two `-H` lines for the same name).
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.body = RequestBody {
        mode: BodyMode::Json,
        content: "".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert!(r.body.is_none());
}

#[test]
fn resolve_endpoint_skips_none_mode() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
    let mut ep = minimal_endpoint();
    ep.url = "/me".into();
    ep.base_url_definition_id = Some("data".into());
    ep.body = RequestBody {
        mode: BodyMode::None,
        content: "ignored".into(),
        schema: None,
    };
    let project = project_with_defs();
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert!(r.body.is_none());
}

#[test]
fn resolve_endpoint_auto_content_type_added() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
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
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert!(r.url.contains("q=hello+world"));
    assert!(!r.url.contains("disabled="));
    assert!(!r.url.contains("empty="));
}

#[test]
fn resolve_endpoint_merges_existing_query_with_params() {
    // URL already has a query string; params should be appended
    // with `&`, not `?`.
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
    let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
    assert_eq!(r.url, "https://api.example.com/search?existing=1&extra=2");
}

// ---- overrides ---------------------------------------------------

#[test]
fn resolve_endpoint_applies_url_override() {
    let mut env = empty_env();
    env.base_urls
        .insert("data".into(), "https://api.example.com".into());
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
