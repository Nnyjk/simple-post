//! 子进程要监听的配置：持久化的 `mcp.http_config` 行 → 子进程 argv。
//!
//! 两半放在一起，因为它们之间有一条契约：设置面板显示的监听地址必须等于 daemon
//! 实际绑定的那个。

use crate::domain::McpHttpConfig;

use super::McpDaemonHandle;

/// 构造守护进程的真实 CLI 参数，桌面端与 Web 端共享同一份契约。
///
/// 除端口外全部取自持久化的 `mcp.http_config` 行：在这里读该行，设置面板与子
/// 进程命令行就不可能各说各话。
///
/// `port` 是监管者刚申领到的 OS 分配端口，显式传入而非让子进程自己再挑一次——
/// 监管者会把这个端口写进 `mcp.port` 并拿它做 health 探测。子进程自身的
/// `mcp.http_config.port` 只对手工运行的 `simple-post mcp` 起作用。
///
/// 空的 allowlist 以 `--allowed-hosts=` / `--allowed-origins=` 传下去，clap 解析成
/// 空列表。这是有意义的而非凑数：配合 `--allow-remote`，空 host 列表意味着
/// **拒绝**所有非 loopback 的 `Host`；省掉该 flag 则要依赖默认值恰好一致。
pub(super) fn daemon_cli_args(port: u16, config: &McpHttpConfig) -> Vec<String> {
    let mut args = vec![
        "mcp".to_string(),
        "--transport=http".to_string(),
        format!("--port={port}"),
        format!("--bind={}", config.host),
        format!("--path={}", config.normalized_path()),
    ];
    if config.allow_remote {
        args.push("--allow-remote".to_string());
    }
    args.push(format!(
        "--allowed-hosts={}",
        config.allowed_hosts.join(",")
    ));
    args.push(format!(
        "--allowed-origins={}",
        config.allowed_origins.join(",")
    ));
    args
}

impl McpDaemonHandle {
    /// 要 spawn 的监听配置：本 handle 带池时读持久化行，否则用出厂默认。
    ///
    /// 在 *spawn* 时读而非构造时，所以用户存了新设置再点重启就足够，无需重建受管
    /// state。
    ///
    /// **坏行 fail-closed**：`mcp.http_config` 读不出来就退回
    /// [`McpHttpConfig::default`]——只监听 loopback——而不是把错误抛出去拒绝启动。
    /// 手工编辑或截断的行不该有能力把 daemon 弄废，而默认值正是那个不暴露任何东西
    /// 的方向。
    pub(super) fn http_config(&self) -> McpHttpConfig {
        let Some(pool) = self.pool.as_ref() else {
            return McpHttpConfig::default();
        };
        match crate::db::with_conn(pool, crate::db::preferences::mcp_http_config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "mcp_daemon",
                    "mcp.http_config unreadable ({e:?}) — spawning on the loopback-only default"
                );
                McpHttpConfig::default()
            }
        }
    }
}
