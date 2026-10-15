//! 监管者句柄与子进程 CLI 契约的单元测试。
//!
//! `daemon_supervisor.rs` 的同级 `#[path]` 模块（以 `mod handle_tests` 挂载）。
//! 全部是对已构造值的纯断言：不 spawn 进程、不绑端口、不碰 `mcp.port`。

use super::*;
use crate::domain::McpHttpConfig;

#[test]
fn daemon_cli_args_use_real_http_transport_flag() {
    assert_eq!(
        daemon_cli_args(43123, &McpHttpConfig::default()),
        vec![
            "mcp",
            "--transport=http",
            "--port=43123",
            "--bind=127.0.0.1",
            "--path=/mcp",
            "--allowed-hosts=",
            "--allowed-origins=",
        ]
    );
}

#[test]
fn daemon_cli_args_carry_the_persisted_listener_configuration() {
    // 子进程 CLI 必须与设置面板写入的那一行一致，否则面板显示的是 LAN daemon 而它
    // 实际仍绑 loopback（或反过来）。
    let cfg = McpHttpConfig {
        host: "0.0.0.0".into(),
        path: "/ai/mcp/".into(),
        allow_remote: true,
        allowed_hosts: vec!["studio.local".into(), "10.0.0.5".into()],
        allowed_origins: vec!["https://claude.ai".into()],
        ..McpHttpConfig::default()
    };
    let args = daemon_cli_args(4173, &cfg);
    for expected in [
        "--bind=0.0.0.0",
        "--path=/ai/mcp", // normalised: no trailing slash
        "--allow-remote",
        "--allowed-hosts=studio.local,10.0.0.5",
        "--allowed-origins=https://claude.ai",
    ] {
        assert!(
            args.iter().any(|a| a == expected),
            "missing {expected} in {args:?}"
        );
    }
}

#[test]
fn daemon_cli_args_omit_the_remote_flag_when_it_is_off() {
    // `--allow-remote` 是个 flag，设置关闭时仍传它等于打开用户明确关掉的 LAN 暴露。
    let args = daemon_cli_args(1, &McpHttpConfig::default());
    assert!(!args.iter().any(|a| a == "--allow-remote"), "got {args:?}");
}

#[test]
fn daemon_cli_args_pass_empty_lists_explicitly() {
    // 配合 `--allow-remote`，空 host 名单意味着 daemon *拒绝*所有非 loopback 的
    // `Host`，这与「clap 取了默认值」不是一回事——监管者不能依赖这种巧合。
    let cfg = McpHttpConfig {
        allow_remote: true,
        ..McpHttpConfig::default()
    };
    let args = daemon_cli_args(1, &cfg);
    assert!(args.iter().any(|a| a == "--allow-remote"));
    assert!(args.iter().any(|a| a == "--allowed-hosts="));
    assert!(args.iter().any(|a| a == "--allowed-origins="));
}

#[test]
fn new_handle_snapshot_is_stopped() {
    let h = McpDaemonHandle::new();
    let s = h.snapshot();
    assert!(!s.running, "new handle should be stopped");
    assert_eq!(s.pid, None);
    assert_eq!(s.port, None);
    assert_eq!(s.requests_handled, 0);
    // 不能写 `assert_eq!(.., 0)`：`McpMetrics::new()` 打上
    // `started_at_ms = now_ms()`，`snapshot()` 返回的是时间差，跨过一条语句的
    // 毫秒边界就会读到 1。那是两条相邻语句之间的墙钟差，不是状态变化——精确
    // 断言 0 会在机器忙到被抢占时随机把 CI 弄红。
    assert!(
        s.uptime_ms <= 1,
        "uptime of a handle that was never started should be ~0, got {}",
        s.uptime_ms
    );
    assert!(s.last_error.is_none());
}

#[test]
fn default_matches_new() {
    let h = McpDaemonHandle::default();
    assert!(!h.snapshot().running);
}

#[test]
fn status_strings_match_wire_contract() {
    // renderer 直接按这些字面量字符串分支，保持稳定。
    assert_eq!(DaemonStatus::Stopped.as_str(), "stopped");
    assert_eq!(DaemonStatus::Starting.as_str(), "starting");
    assert_eq!(DaemonStatus::Running.as_str(), "running");
    assert_eq!(DaemonStatus::Restarting.as_str(), "restarting");
    assert_eq!(
        DaemonStatus::Crashed { exit_code: Some(1) }.as_str(),
        "crashed"
    );
    assert_eq!(DaemonStatus::Unhealthy.as_str(), "unhealthy");
}

#[test]
fn crashed_serializes_with_exit_code() {
    // 线格式自检——renderer 依赖 `{ "crashed": { "exit_code": 1 } }`。
    let s = DaemonStatus::Crashed { exit_code: Some(1) };
    let v = serde_json::to_value(s).unwrap();
    let obj = v.as_object().expect("struct variant serializes to object");
    assert!(obj.contains_key("crashed"), "got: {v}");
    let inner = &obj["crashed"];
    assert_eq!(inner["exit_code"], 1);
}

#[test]
fn crashed_with_no_exit_code_uses_null() {
    let s = DaemonStatus::Crashed { exit_code: None };
    let v = serde_json::to_value(s).unwrap();
    let obj = v.as_object().unwrap();
    assert!(obj["crashed"]["exit_code"].is_null());
}
