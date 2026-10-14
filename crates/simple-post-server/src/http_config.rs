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
use crate::port_registry::{self, PortRecord};

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
    let requires_restart = requires_restart(&config, &previous, port_registry::read());
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
/// 有三个真值来源，但保存时只有两个可知：
///
///   * **host / port** —— 运行中 daemon 自己的 `--bind` 与它实际绑定的端口发布在
///     `mcp.port` 记录里。配置里的 `port: 0` 表示「由 OS 分配」，运行中的 daemon
///     早已解析完毕，故永远不构成差异。
///   * **path / allow_remote / 白名单** —— 记录里没有这些，故只能拿*先前持久化的*
///     那一行当参照。对本应用 spawn 出来的每个 daemon 这都是准确的（supervisor 用同一
///     行构造 CLI）；对手工拉起的 daemon 会偏向「要重启」，而对一个用户无从自行核验的
///     flag 来说这正是安全的方向。
///
/// `record: None` 是「判断不了」的情形，回答 `true`。记录的 PID 已消失则回答
/// `false`：没有东西在服务，下一次 start 自然会取到新行，中间不需要重启步骤。
fn requires_restart(
    new: &McpHttpConfig,
    previous: &McpHttpConfig,
    record: Option<PortRecord>,
) -> bool {
    let Some(record) = record else {
        return true;
    };
    if !port_registry::is_pid_alive(record.pid) {
        return false;
    }
    new.host != record.bind
        || (new.port != 0 && new.port != record.port)
        || new.path != previous.path
        || new.allow_remote != previous.allow_remote
        || new.allowed_hosts != previous.allowed_hosts
        || new.allowed_origins != previous.allowed_origins
}
