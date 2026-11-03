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
