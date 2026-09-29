use crate::db::DbPool;
use crate::error::AppError;

use super::send::{
    resolve_send_input, EndpointSendArgs, EndpointSendOverrides, EndpointSendSettings,
};

/// DB in a temp dir, isolated per test, carrying the minimal fixture
/// chain the send path exercises:
/// `p_demo → m_auth → c_profile → e_user_get`, plus the `def_default`
/// base URL definition and the dev/staging environments. The atomic
/// counter disambiguates when several tests construct their pool
/// inside the same millisecond (cargo runs tests in parallel).
///
/// These rows are built here through the public CRUD API rather than
/// a seed file: Simple Post ships no seed data, and
/// `simple_post_core::db::test_support` is `pub(crate)`, so a
/// dependent crate has no way to reach it. Each test therefore states
/// exactly the rows it depends on.
fn seeded_pool(tag: &str) -> (DbPool, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-endpoint-send-{}-{}-{}",
        std::process::id(),
        tag,
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = crate::db::init(&dir).unwrap();

    use crate::domain::{
        AuthConfig, AuthType, BaseUrlDefinition, EndpointDetail, Environment, HttpMethod, KeyValue,
        Project, RequestBody, ResourceNode, ResourceNodeType,
    };

    let node = |id: &str, kind: ResourceNodeType, parent: Option<&str>| ResourceNode {
        id: id.into(),
        root_id: "p_demo".into(),
        parent_id: parent.map(String::from),
        kind,
        name: "n".into(),
        description: None,
        sort_order: 0,
        created_at: 1,
        updated_at: 1,
    };

    crate::db::with_tx(&pool, |c| {
        crate::db::projects::create(
            c,
            &Project {
                id: "p_demo".into(),
                name: "Demo API".into(),
                description: None,
                default_base_url_definition_id: Some("def_default".into()),
                base_url_definitions: vec![],
                base_url: None,
                color: "#60a5fa".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        crate::db::base_url_defs::create(
            c,
            &BaseUrlDefinition {
                id: "def_default".into(),
                name: "default".into(),
                description: None,
            },
            "p_demo",
        )?;
        crate::db::nodes::create(c, &node("m_auth", ResourceNodeType::Module, None), None)?;
        crate::db::nodes::create(
            c,
            &node("c_profile", ResourceNodeType::Collection, Some("m_auth")),
            None,
        )?;
        // The endpoint's two halves land together — a node without
        // its detail row is a tree the reader cannot open.
        crate::db::nodes::create(
            c,
            &node("e_user_get", ResourceNodeType::Endpoint, Some("c_profile")),
            Some(&EndpointDetail {
                node_id: "e_user_get".into(),
                method: HttpMethod::Get,
                url: "/users/{{userId}}".into(),
                environment_id: None,
                base_url_definition_id: None,
                base_url_override: None,
                params: vec![KeyValue {
                    id: "p1".into(),
                    key: "include".into(),
                    value: "profile,permissions".into(),
                    enabled: true,
                    description: None,
                }],
                headers: vec![],
                body: RequestBody::default(),
                auth: AuthConfig {
                    kind: AuthType::Bearer,
                    bearer: Some("{{token}}".into()),
                    basic: None,
                    apikey: None,
                },
                docs: None,
                notes: None,
                tags: vec![],
            }),
        )?;

        // Both environments deliberately share one base URL and differ
        // only in `variables`, so an assertion on the resolved bearer
        // token is really testing *which* environment was selected.
        let base_urls = || -> std::collections::HashMap<String, String> {
            [(
                "def_default".to_string(),
                "http://localhost:4000/demo".to_string(),
            )]
            .into()
        };
        for (env_id, name, token, is_active) in [
            ("env_dev", "dev", "eyJhbGciOiJIUzI1NiJ9.dev", true),
            (
                "env_staging",
                "staging",
                "eyJhbGciOiJIUzI1NiJ9.staging",
                false,
            ),
        ] {
            crate::db::environments::create(
                c,
                &Environment {
                    id: env_id.into(),
                    project_id: "p_demo".into(),
                    name: name.into(),
                    base_urls: base_urls(),
                    variables: [("token".to_string(), token.to_string())].into(),
                    is_active,
                },
            )?;
        }
        Ok(())
    })
    .expect("seed fixture chain");

    (pool, dir)
}

/// Regression net for a real bug: `resolve_send_input` read only
/// `endpoint.environment_id`, so an endpoint without one resolved to a
/// base-less path and Send failed with
/// `invalid URL '/users/{{userId}}': relative URL without a base`.
/// The renderer's rule is "endpoint's env, else the project's active
/// env"; this pins the second half of that rule.
///
/// `e_user_get` carries an enabled `include` param, which
/// `resolve_endpoint` appends — so the expected URL is base + path +
/// query. It deliberately has no `environment_id` of its own.
#[test]
fn resolves_base_url_from_the_projects_active_env() {
    let (pool, _dir) = seeded_pool("active-env");

    let args = EndpointSendArgs {
        endpoint_id: "e_user_get".into(),
        overrides: None,
        settings: None,
    };
    let input = resolve_send_input(&pool, &args).expect("must resolve");

    // env_dev is the active env and its baseUrls["def_default"] is
    // http://localhost:4000/demo by construction above.
    assert_eq!(
        input.url, "http://localhost:4000/demo/users/{{userId}}?include=profile%2Cpermissions",
        "base URL + path + params must be composed server-side"
    );
    assert_eq!(input.method, "GET");
}

/// The draft path from the URL input travels as `overrides.url` and
/// replaces the stored path, while the base URL and params still come
/// from the database.
#[test]
fn draft_path_override_keeps_the_stored_base_url() {
    let (pool, _dir) = seeded_pool("draft");
    let args = EndpointSendArgs {
        endpoint_id: "e_user_get".into(),
        overrides: Some(EndpointSendOverrides {
            url: Some("/users/42".into()),
            ..Default::default()
        }),
        settings: Some(EndpointSendSettings {
            timeout_ms: Some(1234),
            follow_redirects: Some(false),
            max_response_size_kb: Some(7),
        }),
    };
    let input = resolve_send_input(&pool, &args).expect("must resolve");
    assert_eq!(
        input.url,
        "http://localhost:4000/demo/users/42?include=profile%2Cpermissions"
    );
    assert_eq!(input.timeout_ms, 1234);
    assert!(!input.follow_redirects);
    assert_eq!(input.max_response_size_kb, 7);
}

/// A draft that already carries a scheme wins outright — the join
/// helper treats an absolute path as self-contained.
#[test]
fn absolute_draft_url_wins_over_the_base_url() {
    let (pool, _dir) = seeded_pool("absolute");
    let args = EndpointSendArgs {
        endpoint_id: "e_user_get".into(),
        overrides: Some(EndpointSendOverrides {
            url: Some("https://other.example.com/x".into()),
            ..Default::default()
        }),
        settings: None,
    };
    let input = resolve_send_input(&pool, &args).expect("must resolve");
    assert_eq!(
        input.url,
        "https://other.example.com/x?include=profile%2Cpermissions"
    );
}

/// Settings default sanely when the renderer omits them.
#[test]
fn settings_have_documented_defaults() {
    let (pool, _dir) = seeded_pool("defaults");
    let args = EndpointSendArgs {
        endpoint_id: "e_user_get".into(),
        overrides: None,
        settings: None,
    };
    let input = resolve_send_input(&pool, &args).expect("must resolve");
    assert_eq!(input.timeout_ms, 30_000);
    assert!(input.follow_redirects);
    assert_eq!(input.max_response_size_kb, 10_240);
}

/// The caller's environment selection wins over the database's
/// `is_active` flag.
///
/// Regression net for a silent wrong-credential request. The "ENV"
/// pill writes `activeEnvironmentId` in Zustand only —
/// `setEnvironmentActive`, the only thing that would flip `is_active`,
/// has no caller in `src/`. So the DB always says `env_dev` while the
/// user may be looking at `env_staging`.
///
/// The dev and staging environments deliberately share one base URL
/// and differ only in `variables`, so the assertion is on the
/// resolved bearer token, not the URL — that is the credential which
/// would have gone out wrong.
#[test]
fn caller_environment_selection_wins_over_the_active_flag() {
    let (pool, _dir) = seeded_pool("caller-env");

    let token_of = |input: &crate::http_executor::HttpRequestInput| -> String {
        input
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };

    // Sanity: the fixture chain really does have env_dev active.
    let envs = crate::db::with_conn(&pool, |c| {
        crate::db::environments::list_by_project(c, "p_demo")
    })
    .unwrap();
    assert_eq!(
        envs.iter().find(|e| e.is_active).map(|e| e.id.as_str()),
        Some("env_dev")
    );

    // No override: falls back to the project's active env, i.e. dev.
    let default = resolve_send_input(
        &pool,
        &EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: None,
            settings: None,
        },
    )
    .unwrap();
    assert!(
        token_of(&default).ends_with(".dev"),
        "unselected should use the active env (dev), got {:?}",
        token_of(&default)
    );

    // Override with staging: its token must win.
    let staging = resolve_send_input(
        &pool,
        &EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: Some(EndpointSendOverrides {
                environment_id: Some("env_staging".into()),
                ..Default::default()
            }),
            settings: None,
        },
    )
    .unwrap();
    assert!(
        token_of(&staging).ends_with(".staging"),
        "the selected environment must supply the credential, got {:?}",
        token_of(&staging)
    );
}

/// An unknown endpoint fails before any network I/O.
#[test]
fn unknown_endpoint_is_not_found() {
    let (pool, _dir) = seeded_pool("missing");
    let args = EndpointSendArgs {
        endpoint_id: "e_nope".into(),
        overrides: None,
        settings: None,
    };
    assert!(matches!(
        resolve_send_input(&pool, &args),
        Err(AppError::NotFound(_))
    ));
}
