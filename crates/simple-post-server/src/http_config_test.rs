//! 配置指纹与「要不要重启」的回归网。
//!
//! `http_config.rs` 的同级 `#[path]` 模块（以 `mod tests` 挂载）。全部是对纯
//! 函数的断言：不碰数据库、不写文件。

use super::*;
use crate::domain::McpHttpConfig;

#[test]
fn the_same_configuration_always_produces_the_same_fingerprint() {
    let cfg = McpHttpConfig::default();
    assert_eq!(config_fingerprint(&cfg), config_fingerprint(&cfg));
}

#[test]
fn a_different_mount_path_produces_a_different_fingerprint() {
    let base = McpHttpConfig::default();
    let other = McpHttpConfig {
        path: "/ai/mcp".into(),
        ..McpHttpConfig::default()
    };
    assert_ne!(config_fingerprint(&base), config_fingerprint(&other));
}

#[test]
fn a_different_allowlist_produces_a_different_fingerprint() {
    let base = McpHttpConfig::default();

    let other_hosts = McpHttpConfig {
        allowed_hosts: vec!["studio.local".into()],
        ..McpHttpConfig::default()
    };
    assert_ne!(config_fingerprint(&base), config_fingerprint(&other_hosts));

    let other_origins = McpHttpConfig {
        allowed_origins: vec!["https://claude.ai".into()],
        ..McpHttpConfig::default()
    };
    assert_ne!(
        config_fingerprint(&base),
        config_fingerprint(&other_origins)
    );
}

#[test]
fn the_order_of_the_allowlists_does_not_change_the_fingerprint() {
    // 那两张名单是成员判定，顺序不改变行为，故顺序不同不该触发重启。
    let a = McpHttpConfig {
        allowed_hosts: vec!["a.local".into(), "b.local".into()],
        ..McpHttpConfig::default()
    };
    let b = McpHttpConfig {
        allowed_hosts: vec!["b.local".into(), "a.local".into()],
        ..McpHttpConfig::default()
    };
    assert_eq!(config_fingerprint(&a), config_fingerprint(&b));
}

/// 端口参与指纹。
///
/// 这条断言是「MCP 没按配置的端口启动」那个缺陷的防线：端口不参与指纹时，监管者会
/// 「收养」那个还在旧端口上服务的 daemon，于是改了端口也不生效。
#[test]
fn the_listener_port_takes_part_in_the_fingerprint() {
    let a = McpHttpConfig::default();
    let b = McpHttpConfig {
        port: 8765,
        ..McpHttpConfig::default()
    };
    assert_ne!(config_fingerprint(&a), config_fingerprint(&b));
}

#[test]
fn the_bind_address_takes_part_in_the_fingerprint() {
    let a = McpHttpConfig::default();
    let b = McpHttpConfig {
        host: "0.0.0.0".into(),
        ..McpHttpConfig::default()
    };
    assert_ne!(config_fingerprint(&a), config_fingerprint(&b));
}

// -------------------------------------------------------------------------
// 「要不要重启」
//
// MCP 的监听器**就是**守护进程，故只有一条判断：改任何一项都要重启它。
// -------------------------------------------------------------------------

/// 一条「还活着的 daemon」记录。PID 取当前进程，故 `is_pid_alive` 一定为真。
fn live_entry(port: u16, fingerprint: Option<String>) -> crate::port_registry::PortEntry {
    crate::port_registry::PortEntry {
        record: crate::port_registry::PortRecord {
            pid: std::process::id(),
            port,
            bind: "127.0.0.1".into(),
            started_at: 0,
            data_dir: "C:/tmp".into(),
            version: "test".into(),
        },
        config_fingerprint: fingerprint,
    }
}

#[test]
fn no_record_at_all_asks_for_a_restart() {
    // 判断不了的方向是重启。
    let cfg = McpHttpConfig::default();
    assert!(requires_restart(&cfg, &cfg, None));
}

#[test]
fn a_dead_record_asks_for_no_restart() {
    // 没有东西在服务，下一次 start 自然会取到新行，中间不需要重启步骤。
    let cfg = McpHttpConfig::default();
    let entry = crate::port_registry::PortEntry {
        record: crate::port_registry::PortRecord {
            pid: 0,
            port: 33143,
            bind: "127.0.0.1".into(),
            started_at: 0,
            data_dir: "C:/tmp".into(),
            version: "test".into(),
        },
        config_fingerprint: Some(config_fingerprint(&cfg)),
    };
    assert!(!requires_restart(&cfg, &cfg, Some(entry)));
}

#[test]
fn a_matching_fingerprint_asks_for_no_restart() {
    let cfg = McpHttpConfig::default();
    let entry = live_entry(33143, Some(config_fingerprint(&cfg)));
    assert!(!requires_restart(&cfg, &cfg, Some(entry)));
}

/// 改端口要重启：端口就是对外暴露的地址，端口变了 daemon 就不是在服务同一份配置。
#[test]
fn a_port_change_asks_for_a_restart() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        port: 54174,
        ..McpHttpConfig::default()
    };
    let entry = live_entry(33143, Some(config_fingerprint(&previous)));
    assert!(requires_restart(&new, &previous, Some(entry)));
}

/// 旧记录里没有指纹时，配置里写死的端口必须与 daemon 实际在听的那个一致。
#[test]
fn a_configured_port_must_match_the_port_the_daemon_listens_on() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        port: 54174,
        ..McpHttpConfig::default()
    };
    assert!(requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, None))
    ));
    assert!(!requires_restart(
        &new,
        &previous,
        Some(live_entry(54174, None))
    ));
}

/// 端口写 `0` 时由操作系统分配，与记录里的那个数无关，故两者不该被比较。
#[test]
fn an_os_assigned_port_is_never_compared_with_the_recorded_one() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig::default();
    assert!(!requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, None))
    ));
}

#[test]
fn the_bind_address_change_asks_for_a_restart() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        host: "0.0.0.0".into(),
        ..McpHttpConfig::default()
    };
    let entry = live_entry(33143, Some(config_fingerprint(&previous)));
    assert!(requires_restart(&new, &previous, Some(entry)));
}

#[test]
fn a_remote_access_change_asks_for_a_restart() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        allow_remote: true,
        allowed_hosts: vec!["*".into()],
        allowed_origins: vec!["*".into()],
        ..McpHttpConfig::default()
    };
    let entry = live_entry(33143, Some(config_fingerprint(&previous)));
    assert!(requires_restart(&new, &previous, Some(entry)));
}

#[test]
fn a_mount_path_change_asks_for_a_restart() {
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        path: "/ai/mcp".into(),
        ..McpHttpConfig::default()
    };
    let entry = live_entry(33143, Some(config_fingerprint(&previous)));
    assert!(requires_restart(&new, &previous, Some(entry)));
}

#[test]
fn the_order_of_the_allowlists_does_not_ask_for_a_restart() {
    // 两张名单是成员判定，顺序不改变行为，故换个顺序不该要求重启。
    let previous = McpHttpConfig {
        allowed_hosts: vec!["a.local".into(), "b.local".into()],
        allowed_origins: vec!["https://claude.ai".into()],
        ..McpHttpConfig::default()
    };
    let new = McpHttpConfig {
        allowed_hosts: vec!["b.local".into(), "a.local".into()],
        allowed_origins: vec!["https://claude.ai".into()],
        ..McpHttpConfig::default()
    };
    assert!(!requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, None))
    ));
}
