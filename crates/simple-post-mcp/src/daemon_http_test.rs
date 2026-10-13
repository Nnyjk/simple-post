//! Host / Origin guard + router path-mounting tests.
//!
//! These are deliberately *unit* tests against the pure decision
//! functions rather than HTTP round-trips: every rule here is a
//! function of two headers and a config, so a socket would only add
//! flakiness without observing anything new. The middleware itself is
//! the adapter between `HeaderMap` and [`super::check_access`].

use super::*;
use simple_post_core::domain::McpHttpConfig;

/// Loopback-only defaults — the shipped configuration.
fn local_only() -> McpHttpConfig {
    McpHttpConfig::default()
}

/// `allow_remote: true` with an explicit host allowlist.
fn remote_with(hosts: &[&str]) -> McpHttpConfig {
    McpHttpConfig {
        allow_remote: true,
        allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
        ..McpHttpConfig::default()
    }
}

// -------------------------------------------------------------------------
// Host header parsing
// -------------------------------------------------------------------------

#[test]
fn host_name_strips_a_numeric_port() {
    assert_eq!(host_name("127.0.0.1:43123"), "127.0.0.1");
    assert_eq!(host_name("studio.local:4173"), "studio.local");
}

#[test]
fn host_name_keeps_a_bracketed_ipv6_literal_intact() {
    assert_eq!(host_name("[::1]:4173"), "::1");
    assert_eq!(host_name("[::1]"), "::1");
    // An unterminated bracket is not a host we can reason about, so it
    // is passed through rather than silently truncated to something
    // that might match the allowlist.
    assert_eq!(host_name("[::1"), "[::1");
}

#[test]
fn host_name_keeps_a_bare_host_untouched() {
    assert_eq!(host_name("localhost"), "localhost");
    assert_eq!(host_name("  10.0.0.5  "), "10.0.0.5");
}

// -------------------------------------------------------------------------
// Rule 1 — allow_remote == false: loopback only
// -------------------------------------------------------------------------

#[test]
fn local_host_is_allowed_by_default() {
    let cfg = local_only();
    for host in [
        "127.0.0.1:43123",
        "localhost:4173",
        "[::1]:4173",
        "127.0.0.1",
    ] {
        assert!(
            host_permitted(&cfg, Some(host)),
            "loopback must be accepted under the default config: {host}"
        );
    }
}

#[test]
fn remote_host_is_denied_by_default() {
    let cfg = local_only();
    for host in ["10.0.0.5:4173", "studio.local", "evil.example.com:4173"] {
        assert!(
            !host_permitted(&cfg, Some(host)),
            "a non-loopback Host must be 403 under the default config: {host}"
        );
    }
}

#[test]
fn a_missing_host_header_is_denied() {
    // Fail closed: a request the server cannot attribute to a host is
    // not one to guess about.
    assert!(!host_permitted(&local_only(), None));
}

#[test]
fn the_configured_bind_host_is_always_acceptable() {
    let cfg = McpHttpConfig {
        host: "127.0.0.1".into(),
        ..McpHttpConfig::default()
    };
    assert!(host_permitted(&cfg, Some("127.0.0.1:9999")));
}

// -------------------------------------------------------------------------
// Rule 2 — allow_remote == true: allowlist, empty list denies
// -------------------------------------------------------------------------

#[test]
fn allow_remote_with_a_matching_host_is_allowed() {
    let cfg = remote_with(&["studio.local", "10.0.0.5"]);
    assert!(host_permitted(&cfg, Some("studio.local:4173")));
    assert!(host_permitted(&cfg, Some("10.0.0.5:4173")));
    // Host names are case-insensitive.
    assert!(host_permitted(&cfg, Some("STUDIO.local:4173")));
}

#[test]
fn allow_remote_with_a_non_matching_host_is_denied() {
    let cfg = remote_with(&["studio.local"]);
    assert!(!host_permitted(&cfg, Some("10.0.0.9:4173")));
}

#[test]
fn allow_remote_with_an_empty_allowlist_denies_every_remote_host() {
    // The security-critical case. "No allowlist" must read as "nothing
    // extra is allowed", not "everything is allowed" — the latter turns
    // the remote switch itself into the vulnerability.
    let cfg = remote_with(&[]);
    assert!(!host_permitted(&cfg, Some("10.0.0.5:4173")));
    assert!(!host_permitted(&cfg, Some("studio.local")));
    // Loopback keeps working, which is what lets `port_registry::healthz`
    // keep reporting green with a LAN-only allowlist configured.
    assert!(host_permitted(&cfg, Some("127.0.0.1:4173")));
}

#[test]
fn allow_remote_still_denies_when_the_switch_is_off() {
    // A populated allowlist does nothing on its own — without the
    // switch, a LAN bind is a contradiction `validate` refuses.
    let cfg = McpHttpConfig {
        allow_remote: false,
        allowed_hosts: vec!["studio.local".into()],
        ..McpHttpConfig::default()
    };
    assert!(!host_permitted(&cfg, Some("studio.local")));
}

#[test]
fn a_star_entry_matches_any_host() {
    let cfg = remote_with(&["*"]);
    assert!(host_permitted(&cfg, Some("studio.local:4173")));
    assert!(host_permitted(&cfg, Some("10.0.0.5:4173")));
    assert!(host_permitted(&cfg, Some("anything.example.com")));
    assert!(
        !host_permitted(&cfg, None),
        "wildcard is not a licence to skip the header"
    );
}

// -------------------------------------------------------------------------
// Rule 3 — Origin
// -------------------------------------------------------------------------

#[test]
fn a_request_without_an_origin_is_allowed() {
    // curl / Claude Code / Claude Desktop never send one; requiring it
    // would lock out every real AI client.
    let cfg = McpHttpConfig::default();
    assert!(origin_permitted(&cfg, None));
    assert!(check_access(&cfg, Some("127.0.0.1:4173"), None).is_ok());
}

#[test]
fn an_origin_outside_the_allowlist_is_denied() {
    let cfg = McpHttpConfig {
        allowed_origins: vec!["https://claude.ai".into()],
        ..McpHttpConfig::default()
    };
    assert!(origin_permitted(&cfg, Some("https://claude.ai")));
    assert!(!origin_permitted(&cfg, Some("https://evil.example")));
    // An empty allowlist with a browser-shaped request denies it — the
    // default config is not a browser endpoint.
    assert!(!origin_permitted(
        &McpHttpConfig::default(),
        Some("https://evil.example")
    ));
}

#[test]
fn a_star_origin_entry_matches_any_origin() {
    let cfg = McpHttpConfig {
        allowed_origins: vec!["*".into()],
        ..McpHttpConfig::default()
    };
    assert!(origin_permitted(&cfg, Some("https://anything.example")));
}

#[test]
fn origin_check_runs_after_the_host_check() {
    // A request that fails both must report the host, so the message
    // points at the thing the user most likely got wrong.
    let cfg = McpHttpConfig::default();
    let err = check_access(&cfg, Some("10.0.0.5:4173"), Some("https://evil.example"))
        .expect_err("must deny");
    assert!(err.contains("host"), "got: {err}");
}

// -------------------------------------------------------------------------
// Router mounting
// -------------------------------------------------------------------------

#[test]
fn a_custom_path_is_also_served_on_the_canonical_mcp_route() {
    // `port_registry::healthz` probes a hardcoded `/mcp/healthz`, and
    // every already-configured AI client points at `/mcp`. Without this
    // alias a rename silently breaks the supervisor's liveness probe.
    let cfg = McpHttpConfig {
        path: "/ai/mcp".into(),
        ..McpHttpConfig::default()
    };
    assert_eq!(mounted_prefixes(&cfg), vec!["/ai/mcp", "/mcp"]);
    let (json_rpc, healthz, tools) = route_paths("/ai/mcp");
    assert_eq!(json_rpc, "/ai/mcp");
    assert_eq!(healthz, "/ai/mcp/healthz");
    assert_eq!(tools, "/ai/mcp/tools");
}

#[test]
fn the_default_path_is_not_mounted_twice() {
    // Merging `/mcp` with itself would panic in axum, so the alias
    // must be skipped when the configured path already *is* `/mcp`.
    assert_eq!(mounted_prefixes(&McpHttpConfig::default()), vec!["/mcp"]);
}

#[test]
fn a_trailing_slash_still_yields_a_single_mcp_mount() {
    let cfg = McpHttpConfig {
        path: "/mcp/".into(),
        ..McpHttpConfig::default()
    };
    assert_eq!(mounted_prefixes(&cfg), vec!["/mcp"]);
}

// -------------------------------------------------------------------------
// Path normalisation
// -------------------------------------------------------------------------

#[test]
fn paths_normalise_to_leading_slash_without_trailing_slash() {
    let cases = [
        ("/mcp", "/mcp"),
        ("mcp", "/mcp"),
        ("/mcp/", "/mcp"),
        ("/ai/mcp/", "/ai/mcp"),
        ("  /ai/mcp  ", "/ai/mcp"),
        ("/", "/mcp"),
        ("", "/mcp"),
    ];
    for (input, expected) in cases {
        let cfg = McpHttpConfig {
            path: input.to_string(),
            ..McpHttpConfig::default()
        };
        assert_eq!(cfg.normalized_path(), expected, "input: {input:?}");
    }
}

// -------------------------------------------------------------------------
// Validation — the self-contradicting combination
// -------------------------------------------------------------------------

#[test]
fn a_non_loopback_bind_without_allow_remote_is_rejected() {
    let cfg = McpHttpConfig {
        host: "0.0.0.0".into(),
        allow_remote: false,
        ..McpHttpConfig::default()
    };
    let err = cfg
        .validate()
        .expect_err("must refuse LAN bind with the guard closed");
    assert!(err.to_string().contains("allowRemote"), "got: {err}");
}

#[test]
fn a_non_loopback_bind_with_allow_remote_is_accepted() {
    let cfg = McpHttpConfig {
        host: "0.0.0.0".into(),
        allow_remote: true,
        ..McpHttpConfig::default()
    };
    assert!(cfg.validate().is_ok());
}

#[test]
fn loopback_binds_validate_without_allow_remote() {
    for host in ["127.0.0.1", "localhost", "::1"] {
        let cfg = McpHttpConfig {
            host: host.to_string(),
            ..McpHttpConfig::default()
        };
        assert!(cfg.validate().is_ok(), "host {host} must validate");
    }
}

#[test]
fn an_empty_host_is_rejected() {
    let cfg = McpHttpConfig {
        host: "  ".into(),
        ..McpHttpConfig::default()
    };
    assert!(cfg.validate().is_err());
}

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

#[test]
fn the_router_builds_for_every_mount_shape() {
    // Exercises the real `build_router_with_config` merge + middleware
    // wiring rather than only the pure prefix helper. A duplicate merge
    // (which axum panics on) or a mis-declared state type surfaces here
    // instead of at daemon boot.
    for path in ["/mcp", "/mcp/", "mcp", "/ai/mcp", "/", ""] {
        let cfg = McpHttpConfig {
            path: path.to_string(),
            ..McpHttpConfig::default()
        };
        let _router = build_router_with_config(Arc::new(test_state()), &cfg);
    }
}

/// `AppState` whose pool is only ever *borrowed* by the router-building
/// tests — no handler runs, so the in-memory database is never touched.
fn test_state() -> AppState {
    AppState {
        pool: r2d2::Pool::builder()
            .max_size(1)
            .build(r2d2_sqlite::SqliteConnectionManager::memory())
            .expect("in-memory pool"),
        started_at: std::time::Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    }
}
