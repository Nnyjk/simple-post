//! MCP HTTP daemon 的端口注册表。
//!
//! daemon 启动时把一条小 JSON 记录（`mcp.port`）写到平台数据目录。GUI 启动时读它
//! 以发现 daemon 端点；独立的 `simple-post-mcp-proxy` 二进制读同一个文件，得知把
//! JSON-RPC 流量转到哪里。
//!
//! 并发：两个 GUI 实例同时启动都会抢这个文件。写路径用进程级
//! `tokio::sync::Mutex` 串起来，并用 `O_EXCL`（`OpenOptions::create_new`）让 OS
//! 自己拒掉第二个写入者。输家重读、发现是个活 PID，于是报 `Validation` 错误，而不是
//! 悄悄覆盖赢家的记录。
//!
//! 文件路径：
//!   * Windows: `%APPDATA%\simple-post\mcp.port`
//!   * macOS:   `~/Library/Application Support/simple-post/mcp.port`
//!   * Linux:   `$XDG_DATA_HOME/simple-post/mcp.port`（或
//!     `~/.local/share/simple-post/mcp.port`）
//!
//! 测试下用 `SIMPLE_POST_DATA_DIR` 覆盖基准目录，免得单测踩掉真实用户的端口文件。
//!
//! ## 文件分工
//!
//! 本文件是门面，只保留「读/删文件」这两个入口，其余按职责落在同名子目录下：
//!
//!   * `paths` —— 平台目录解析与 `mcp.port` 路径拼装。
//!   * `record` —— 文件里那条记录的形状与它自己的时间戳。
//!   * `claim` —— 多实例申领：`create_new` 与它的回落/报错，整块不可再拆。
//!   * `proc` —— 进程表探测与按 PID 强杀。
//!   * `health` —— `/mcp/healthz` 存活探测。

mod claim;
mod health;
mod paths;
mod proc;
mod record;

// 门面即公开面：子模块的私有性不该改变调用方的路径写法，`port_registry::write` /
// `port_registry::read` / … 在 `src-tauri` 与 daemon CLI 里都按原样解析。
pub use claim::write;
pub use health::healthz;
pub use paths::{data_dir, port_file_path, FILE_NAME};
pub use proc::{is_pid_alive, terminate_pid};
pub use record::PortRecord;

/// 读当前记录。文件不存在、读不了或反序列化失败（损坏 / 写了一半——按「没有 daemon」
/// 处理，比 panic 安全）时返回 `None`。
pub fn read() -> Option<PortRecord> {
    let path = port_file_path().ok()?;
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice::<PortRecord>(&bytes) {
        Ok(rec) => Some(rec),
        Err(e) => {
            tracing::debug!(
                target: "port_registry",
                "mcp.port present but unparseable, treating as missing: {e}"
            );
            None
        }
    }
}

/// 删掉端口文件（GUI 退出时调用）。幂等——`NotFound` 视作成功，故 renderer 可以
/// 随意调用而不必先 `if path.exists()`。
pub fn release() -> Result<(), crate::error::AppError> {
    let path = port_file_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(crate::error::AppError::Io(e)),
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "port_registry_test.rs"]
mod tests;
