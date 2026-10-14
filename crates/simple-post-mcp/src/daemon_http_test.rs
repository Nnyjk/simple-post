//! Host / Origin 守卫 + router 路径挂载的测试。
//!
//! 刻意是对那些纯判定函数做**单元**测试而不是 HTTP 往返：这里每条规则都只是两个头
//! 与一份配置的函数，挂上 socket 只会平白增加抖动而观察不到任何新东西。中间件本身只是
//! `HeaderMap` 与 [`super::check_access`] 之间的适配层。

use super::*;
use simple_post_core::domain::McpHttpConfig;

/// 出厂配置：仅回环。
fn local_only() -> McpHttpConfig {
    McpHttpConfig::default()
}

/// `allow_remote: true` 且带显式 host 白名单。
fn remote_with(hosts: &[&str]) -> McpHttpConfig {
    McpHttpConfig {
        allow_remote: true,
        allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
        ..McpHttpConfig::default()
    }
}

// -------------------------------------------------------------------------
// Host 头解析
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
    // 括号不闭合的 host 不是我们能推理的东西，所以原样放过，而不是悄悄截断成某个
    // 可能命中白名单的值。
    assert_eq!(host_name("[::1"), "[::1");
}

#[test]
fn host_name_keeps_a_bare_host_untouched() {
    assert_eq!(host_name("localhost"), "localhost");
    assert_eq!(host_name("  10.0.0.5  "), "10.0.0.5");
}

// -------------------------------------------------------------------------
// 规则 1 —— allow_remote == false：只允许回环
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
    // fail closed：连宿主都无法归属的请求，不该去猜。
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
// 规则 2 —— allow_remote == true：白名单，空列表一律拒绝
// -------------------------------------------------------------------------

#[test]
fn allow_remote_with_a_matching_host_is_allowed() {
    let cfg = remote_with(&["studio.local", "10.0.0.5"]);
    assert!(host_permitted(&cfg, Some("studio.local:4173")));
    assert!(host_permitted(&cfg, Some("10.0.0.5:4173")));
    // 主机名大小写不敏感。
    assert!(host_permitted(&cfg, Some("STUDIO.local:4173")));
}

#[test]
fn allow_remote_with_a_non_matching_host_is_denied() {
    let cfg = remote_with(&["studio.local"]);
    assert!(!host_permitted(&cfg, Some("10.0.0.9:4173")));
}

#[test]
fn allow_remote_with_an_empty_allowlist_denies_every_remote_host() {
    // 这里是最要安全的一种情形。「没配白名单」必须读成「什么都不额外允许」，而不是
    // 「全都允许」—— 后者等于让这个远程开关本身成为漏洞。
    let cfg = remote_with(&[]);
    assert!(!host_permitted(&cfg, Some("10.0.0.5:4173")));
    assert!(!host_permitted(&cfg, Some("studio.local")));
    // 回环仍要能通，`port_registry::healthz` 正靠这一点在只含内网地址的白名单下继续
    // 报绿。
    assert!(host_permitted(&cfg, Some("127.0.0.1:4173")));
}

#[test]
fn allow_remote_still_denies_when_the_switch_is_off() {
    // 光有一份非空白名单自己不起作用 —— 没有开关时，绑定内网本身是个矛盾，`validate`
    // 会拒绝。
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
// 规则 3 —— Origin
// -------------------------------------------------------------------------

#[test]
fn a_request_without_an_origin_is_allowed() {
    // curl / Claude Code / Claude Desktop 根本不发这个头；强制要求会把所有真实 AI
    // 客户端挡在门外。
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
    // 空白名单遇到浏览器形状的请求要拒绝 —— 默认配置并不是给浏览器用的端点。
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
    // 两项都失败的请求必须报出 host，好让消息指向用户最可能弄错的那一处。
    let cfg = McpHttpConfig::default();
    let err = check_access(&cfg, Some("10.0.0.5:4173"), Some("https://evil.example"))
        .expect_err("must deny");
    assert!(err.contains("host"), "got: {err}");
}

// -------------------------------------------------------------------------
// router 挂载
// -------------------------------------------------------------------------

#[test]
fn a_custom_path_is_also_served_on_the_canonical_mcp_route() {
    // `port_registry::healthz` 探的是写死的 `/mcp/healthz`，每个已配置好的 AI 客户端也都
    // 指着 `/mcp`。没有这个别名，一次改名就会悄悄打断 supervisor 的存活探测。
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
    // 把 `/mcp` 与自身合并会让 axum panic，所以配置路径本身就是 `/mcp` 时必须跳过
    // 这个别名。
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
// 路径归一
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
// 校验 —— 自相矛盾的组合
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
// 辅助函数
// -------------------------------------------------------------------------

#[test]
fn the_router_builds_for_every_mount_shape() {
    // 走真实的 `build_router_with_config` 合并 + 中间件接线，而不只测那个纯前缀辅助
    // 函数。重复合并（axum 会 panic）或状态类型声明错误，都会在此暴露而不是在 daemon
    // 启动时。
    for path in ["/mcp", "/mcp/", "mcp", "/ai/mcp", "/", ""] {
        let cfg = McpHttpConfig {
            path: path.to_string(),
            ..McpHttpConfig::default()
        };
        let _router = build_router_with_config(Arc::new(test_state()), &cfg);
    }
}

/// `AppState`，它的池只被 router 构建类测试**借用** —— 不会跑任何 handler，所以内存
/// 数据库根本没被碰到。
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
