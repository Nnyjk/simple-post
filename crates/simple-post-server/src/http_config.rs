// =========================================================================
// MCP HTTP 监听配置
// =========================================================================
//
// daemon 的绑定地址、端口、挂载路径与 Host/Origin 白名单以 JSON blob 形式持久化在
// `preferences` 的 `mcp.http_config` 下。
//
// **为什么是这对辅助函数而不是直接写 preferences。** daemon 是*子进程*：这一行的内容
// 就是 `daemon_supervisor::daemon_cli_args` 在下次 spawn 时放进命令行的东西。故保存
// 并不会重配正在运行的 daemon，响应里因此带 `requires_restart`，让 UI 能把这件事说
// 出来，而不是静默展示一份根本没生效的设置。
//
// 两条 transport 都走下面这两个普通函数——`src-tauri/src/commands/mcp_http_config.rs`
// 的 Tauri 命令与 `web_api/routes/mcp.rs` 的 REST 路由都是薄包装，故桌面端与 web 端在
// 校验上、在「是否需要重启」这个判断上都不会漂移。

use serde::Serialize;

use crate::db::DbPool;
use crate::domain::McpHttpConfig;
use crate::error::AppError;
use crate::port_registry::{self, PortEntry};

/// 一次写入的响应信封：存下的配置，加上「正在运行的 daemon 是否已经在服务别的东西」。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpConfigDto {
    pub config: McpHttpConfig,
    /// 新配置与正在运行的 daemon 实际在服务的东西不同，须重启才生效。
    pub requires_restart: bool,
}

/// 读持久化配置。行缺失返回 [`McpHttpConfig::default`]；无法解析则报错（见
/// `db::preferences::mcp_http_config`）。
pub fn read_http_config(pool: &DbPool) -> Result<McpHttpConfig, AppError> {
    crate::db::with_conn(pool, crate::db::preferences::mcp_http_config)
}

/// 校验、持久化，并报告是否需要重启。
pub fn write_http_config(
    pool: &DbPool,
    config: McpHttpConfig,
) -> Result<McpHttpConfigDto, AppError> {
    config.validate()?;
    let previous = read_http_config(pool)?;
    let requires_restart = requires_restart(&config, &previous, port_registry::read_entry());
    crate::db::with_tx(pool, |c| {
        crate::db::preferences::set_mcp_http_config(c, &config)
    })?;
    Ok(McpHttpConfigDto {
        config,
        requires_restart,
    })
}

/// 新配置与 daemon 现在服务的东西是否不同？
///
/// 真值来源有三个：
///
///   * **记录里的配置指纹** —— 运行中的 daemon 实际在服务的那份配置的指纹。supervisor
///     spawn daemon 时按它实际用的那份配置算出这个指纹并写进 `mcp.port` 记录，故它
///     说的就是「这个 daemon 现在服务什么」。指纹与新配置的指纹不同就是有差异。
///   * **host / port** —— 运行中 daemon 自己的 `--bind` 与它实际绑定的端口发布在
///     `mcp.port` 记录里。配置里的 `port: 0` 表示「由 OS 分配」，运行中的 daemon
///     早已解析完毕，故永远不构成差异。
///   * **path / allow_remote / 白名单** —— 旧记录里既没有这些也没有指纹，故只能拿
///     *先前持久化的*那一行当参照。对本应用 spawn 出来的每个 daemon 这都是准确的
///     （supervisor 用同一行构造 CLI）；对手工拉起的 daemon 会偏向「要重启」，而对一个
///     用户无从自行核验的 flag 来说这正是安全的方向。
///
/// `record: None` 是「判断不了」的情形，回答 `true`。记录的 PID 已消失则回答
/// `false`：没有东西在服务，下一次 start 自然会取到新行，中间不需要重启步骤。
/// 旧记录读不出指纹也是「判断不了」，故它落到先前持久化的那一行上——那份参照对本
/// 应用 spawn 出来的 daemon 是准确的。
fn requires_restart(
    new: &McpHttpConfig,
    previous: &McpHttpConfig,
    record: Option<PortEntry>,
) -> bool {
    let Some(record) = record else {
        return true;
    };
    if !port_registry::is_pid_alive(record.record.pid) {
        return false;
    }
    // 记录里有指纹时，它说的就是这个 daemon 实际在服务什么：直接比指纹。
    if let Some(recorded) = record.config_fingerprint.as_deref() {
        return config_fingerprint(new) != recorded;
    }
    new.host != record.record.bind
        || (new.port != 0 && new.port != record.record.port)
        || new.path != previous.path
        || new.allow_remote != previous.allow_remote
        || new.allowed_hosts != previous.allowed_hosts
        || new.allowed_origins != previous.allowed_origins
}

/// 一份 MCP HTTP 监听配置的指纹：两份配置服务的内容完全相同时，指纹相同。
///
/// 参与计算的有五项：绑定地址、挂载路径、远程开关、允许的 `Host` 名单、允许的
/// `Origin` 名单。监听端口不参与——daemon 实际在听的端口由 OS 分配并记在端口文件
/// 里，而配置里的 `port` 描述的是 `serve-web` 那个「只服务 MCP」的监听器，与
/// daemon 子进程无关。
///
/// 两张名单先排序再拼，故「同一组名单、顺序不同」得到同一个指纹：那两张名单是成员
/// 判定，顺序不改变行为。
///
/// 指纹是规范化后的 JSON 文本，不是摘要：这个 crate 里没有可用的哈希依赖，而这个
/// 形态写在端口文件里还能直接读懂。比较它即比较两份配置。
pub fn config_fingerprint(config: &McpHttpConfig) -> String {
    let canonical = serde_json::json!({
        "v": 1,
        "host": config.host.trim(),
        "path": config.normalized_path(),
        "allow_remote": config.allow_remote,
        "allowed_hosts": sorted_copy(&config.allowed_hosts),
        "allowed_origins": sorted_copy(&config.allowed_origins),
    });
    canonical.to_string()
}

/// 拷贝一份名单并按字典序排好。
fn sorted_copy(items: &[String]) -> Vec<String> {
    let mut copy = items.to_vec();
    copy.sort();
    copy
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "http_config_test.rs"]
mod tests;
