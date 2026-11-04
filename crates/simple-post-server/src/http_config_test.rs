//! 配置指纹的回归网。
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

#[test]
fn the_listener_port_does_not_take_part_in_the_fingerprint() {
    // daemon 实际在听的端口由 OS 分配并记在端口文件里；配置里的 port 描述的是
    // serve-web 那个「只服务 MCP」的监听器，与 daemon 子进程无关。
    let a = McpHttpConfig::default();
    let b = McpHttpConfig {
        port: 8765,
        ..McpHttpConfig::default()
    };
    assert_eq!(config_fingerprint(&a), config_fingerprint(&b));
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
// 「要不要重启」的两条判断
//
// 守护进程与 MCP 专用监听器是两个各自独立的对象，故它们各有各的判断：守护进程读的是
// spawn 时进命令行的那几项，专用监听器读的是 Web 进程启动时固化下来的那几项。端口只属于
// 后者。
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
fn a_port_change_asks_for_a_listener_restart_but_not_for_a_daemon_restart() {
    // 端口属于 MCP 专用监听器：改了它要重启 Web 服务。守护进程吃不到这个数字
    // （它的端口由操作系统分配），故不该因为端口被要求重启。
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        port: 54174,
        ..McpHttpConfig::default()
    };
    assert!(listener_config_changed(&new, &previous));
    let fingerprint = config_fingerprint(&previous);
    assert!(!requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, Some(fingerprint)))
    ));
}

#[test]
fn the_bind_address_is_not_part_of_the_listener_shape() {
    // 绑定地址是 daemon 的 `--bind`，改它要重启守护进程。专用监听器绑的是主监听器的
    // 地址（容器里的 0.0.0.0），故与配置里的 host 无关。
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        host: "0.0.0.0".into(),
        ..McpHttpConfig::default()
    };
    assert!(!listener_config_changed(&new, &previous));
    let fingerprint = config_fingerprint(&previous);
    assert!(requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, Some(fingerprint)))
    ));
}

#[test]
fn a_remote_access_change_asks_for_both_restarts() {
    // 远程开关与两张白名单两处都要用到：daemon 把它们变成命令行参数，专用监听器把它们
    // 变成 Host/Origin 守卫。故改这两项时，两个对象都要重来一次。
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        allow_remote: true,
        allowed_hosts: vec!["*".into()],
        allowed_origins: vec!["*".into()],
        ..McpHttpConfig::default()
    };
    assert!(listener_config_changed(&new, &previous));
    let fingerprint = config_fingerprint(&previous);
    assert!(requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, Some(fingerprint)))
    ));
}

#[test]
fn the_configured_port_is_never_compared_with_the_daemon_port() {
    // 旧记录里没有指纹时，判定只能拿先前持久化的那一行当参照。曾经有一版在这里拿
    // 「配置里的端口」与「记录里的 daemon 端口」比——两个无关的数：配置里写 54174 而
    // daemon 实际在 33143，于是每次保存都回答「要重启」，而守护进程那边什么都没变。
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        port: 54174,
        ..McpHttpConfig::default()
    };
    assert!(!requires_restart(
        &new,
        &previous,
        Some(live_entry(33143, None))
    ));
}

#[test]
fn the_listener_judgement_ignores_the_order_of_the_allowlists() {
    // 两张名单是成员判定，顺序不改变行为，故换个顺序不该要求重启 Web 服务。
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
    assert!(!listener_config_changed(&new, &previous));
}

#[test]
fn the_listener_judgement_notices_a_path_change() {
    // 专用监听器的挂载路径是启动时固化的，故改路径同样要重启 Web 服务。
    let previous = McpHttpConfig::default();
    let new = McpHttpConfig {
        path: "/ai/mcp".into(),
        ..McpHttpConfig::default()
    };
    assert!(listener_config_changed(&new, &previous));
}
