// =========================================================================
// MCP HTTP 监听配置
// =========================================================================
//
// daemon 的绑定地址、端口、挂载路径与 Host/Origin 白名单以 JSON blob 形式持久化在
// `preferences` 的 `mcp.http_config` 下。daemon 是*子进程*：这一行的内容就是
// `daemon_supervisor::daemon_cli_args` 在下次 spawn 时放进命令行的东西，故保存不会重配
// 正在运行的 daemon，响应里因此带 `requires_restart`，让 UI 能把这件事说出来。
//
// 这一行里的**端口**不属于 daemon 子进程：daemon 的端口由操作系统分配并记在端口文件里，
// 而配置里的端口描述的是 `serve-web` 那个「只服务 MCP」的监听器——那个监听器是 Web 进程
// 启动时开的，故端口的改动由另一条判断 `requires_listener_restart` 单独报出来。
//
// 两条 transport 都走下面这两个普通函数（`src-tauri/src/commands/mcp_http_config.rs` 的
// Tauri 命令与 `web_api/routes/mcp.rs` 的 REST 路由都是薄包装），故桌面端与 web 端在
// 校验上、在「是否需要重启」这两个判断上都不会漂移。

use serde::Serialize;

use crate::db::DbPool;
use crate::domain::McpHttpConfig;
use crate::error::AppError;
use crate::port_registry::{self, PortEntry};

/// AI 客户端在 `mcpServers` 里启动的 `simple-post-mcp` 的绝对路径，取 `current_exe()` 的所在目录：
/// 两个二进制经 `cargo build --workspace` 落在同一 `target/<profile>/`，没有 npm 包装路径可指。
/// 查找失败时返回 `"<unresolved current_exe>"`，渲染侧按不可操作的占位串处理。
pub fn mcp_proxy_path() -> String {
    fn build(proxy_name: &str) -> Option<std::path::PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post-mcp.exe")
    } else {
        build("simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// 一次写入的响应信封：存下的配置，加上两条互不重叠的「要不要重启」判断。
///
/// 两条判断的**对象**不同，故不能合成一个布尔：合成一个之后，端口的改动就会被算进
/// 「守护进程要重启」里，而端口与守护进程毫无关系——守护进程的端口由操作系统分配，
/// 配置里写着的那个数字属于 `serve-web` 的专用监听器。
///
///   * `requires_restart` —— **守护进程**必须重启才生效。改的是绑定地址、挂载路径、
///     远程开关或两张白名单：这些是 daemon 子进程启动时读进命令行的东西。
///   * `requires_listener_restart` —— **MCP 专用监听器**必须由 Web 服务重新开一次才生效。
///     改的是端口、挂载路径、远程开关或两张白名单：那个监听器是 Web 进程启动时开的，
///     端口写进去就得重启 Web 服务（容器）才生效。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpConfigDto {
    pub config: McpHttpConfig,
    /// 守护进程必须重启才生效。
    pub requires_restart: bool,
    /// MCP 专用监听器必须由 Web 服务重新开一次才生效。
    pub requires_listener_restart: bool,
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
    let requires_listener_restart = listener_config_changed(&config, &previous);
    crate::db::with_tx(pool, |c| {
        crate::db::preferences::set_mcp_http_config(c, &config)
    })?;
    Ok(McpHttpConfigDto {
        config,
        requires_restart,
        requires_listener_restart,
    })
}

/// 新配置与 daemon 现在服务的东西是否不同？
///
/// 这个判断的**对象是守护进程**：它管的是「守护进程要不要重启才吃到新配置」。
/// 端口不在其中——守护进程的端口由操作系统分配、记在 `mcp.port` 记录里，从来不是配置里
/// 写着的那个数字；把两者放在一起比，比的是两个无关的数。配置里的端口属于 MCP 专用监听器，
/// 由 `listener_config_changed` 单独判断。
///
/// 真值来源有三个：
///
///   * **记录里的配置指纹** —— 运行中的 daemon 实际在服务的那份配置的指纹，故它说的就是
///     「这个 daemon 现在服务什么」；指纹与新配置的指纹不同就是有差异。
///   * **host** —— 运行中 daemon 自己的 `--bind` 发布在 `mcp.port` 记录里。
///   * **path / allow_remote / 白名单** —— 旧记录里既没有这些也没有指纹，故只能拿*先前持久化的*
///     那一行当参照。对本应用 spawn 出来的每个 daemon 这都是准确的；对手工拉起的 daemon 会
///     偏向「要重启」，而对一个用户无从自行核验的 flag 来说这正是安全的方向。
///
/// `record: None` 是「判断不了」的情形，回答 `true`。记录的 PID 已消失则回答 `false`：没有东西
/// 在服务，下一次 start 自然会取到新行，中间不需要重启步骤。旧记录读不出指纹也是「判断不了」，
/// 故它落到先前持久化的那一行上。
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
        || new.path != previous.path
        || new.allow_remote != previous.allow_remote
        || new.allowed_hosts != previous.allowed_hosts
        || new.allowed_origins != previous.allowed_origins
}

/// MCP 专用监听器的形状变了没有？变了就要重启 Web 服务才生效。
///
/// 那个监听器是 `serve-web` **进程启动时**开的：端口、挂载路径、远程开关与两张白名单都在
/// 那一刻固化下来，故其中任何一项变了都要重启 Web 服务（容器）才生效。绑定地址不在其中——
/// 它绑的是主监听器的地址（容器里的 `0.0.0.0`），不是配置里的 `host`。
///
/// 这个判断与运行中的守护进程无关：守护进程停着的时候，端口改了照样要重启 Web 服务。
pub fn listener_config_changed(new: &McpHttpConfig, previous: &McpHttpConfig) -> bool {
    new.port != previous.port
        || new.normalized_path() != previous.normalized_path()
        || new.allow_remote != previous.allow_remote
        || sorted_copy(&new.allowed_hosts) != sorted_copy(&previous.allowed_hosts)
        || sorted_copy(&new.allowed_origins) != sorted_copy(&previous.allowed_origins)
}

/// 一份 MCP HTTP 监听配置的指纹：两份配置服务的内容完全相同时，指纹相同。
/// 参与计算的有五项：绑定地址、挂载路径、远程开关、允许的 `Host` 名单、允许的 `Origin`
/// 名单。监听端口不参与——daemon 实际在听的端口由 OS 分配并记在端口文件里，而配置里的
/// `port` 描述的是 `serve-web` 那个「只服务 MCP」的监听器，与 daemon 子进程无关。
/// 两张名单先排序再拼，故「同一组名单、顺序不同」得到同一个指纹：那两张名单是成员判定，
/// 顺序不改变行为。
/// 指纹是规范化后的 JSON 文本，不是摘要：这个 crate 里没有可用的哈希依赖，而这个形态写在
/// 端口文件里还能直接读懂。比较它即比较两份配置。
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
