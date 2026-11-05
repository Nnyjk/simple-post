//! MCP HTTP daemon 的端口注册表。
//!
//! daemon 启动时把一条小 JSON 记录（`mcp.port`）写到平台数据目录。GUI 启动时读它
//! 以发现 daemon 端点；独立的 `simple-post-mcp` 桥接二进制读同一个文件，得知把
//! JSON-RPC 流量转到哪里。
//!
//! 并发：两个 GUI 实例同时启动都会抢这个文件。写路径用进程级
//! `tokio::sync::Mutex` 串起来，并用 `O_EXCL`（`OpenOptions::create_new`）让 OS
//! 自己拒掉第二个写入者。输家重读、发现是个活 PID，于是报 `Validation` 错误，而不是
//! 悄悄覆盖赢家的记录。
//!
//! **文件路径与记录形状住在 `simple_post_core::port_file`**：读这个文件的一方不止
//! 一个——`simple-post-server` 写它，`simple-post-mcp` 读它，而后者不能依赖前者
//! （依赖方向是单向的：`server → core`、`mcp → core`）。规则若各写一遍，两处就会漂。
//!
//! 本文件是门面，只保留「申领 / 删除」这两个入口，其余按职责落在同名子目录下：
//!
//!   * `claim` —— 多实例申领：`create_new` 与它的回落/报错，整块不可再拆。
//!   * `proc` —— 进程表探测与按 PID 强杀。
//!   * `health` —— `/mcp/healthz` 存活探测。
//!
//! 路径解析与记录形状由 core 提供，见 [`simple_post_core::port_file`]。

mod claim;
mod health;
mod proc;

// 门面即对外接口：子模块的私有性不该改变调用方的路径写法，`port_registry::write` /
// `port_registry::read` / … 在 `src-tauri` 与 daemon CLI 里都按原样解析。
pub use claim::{write, write_with_fingerprint};
pub use health::healthz;
pub use proc::{is_pid_alive, terminate_pid};

// 路径与记录形状来自 core（原因见文件头的说明）。这里再导出一次，
// `crate::port_registry::PortRecord` 这条路径在两个 crate 里都解析得到。
pub use simple_post_core::port_file::{
    data_dir, now_secs, parse_entry, port_file_path, serialize_entry, PortEntry, PortRecord,
    CONFIG_FINGERPRINT_KEY, DATA_DIR_ENV, FILE_NAME,
};

/// 读当前记录与它服务的配置的指纹。文件不存在、读不了或反序列化失败（损坏 / 写
/// 了一半——按「没有 daemon」处理，比 panic 安全）时返回 `None`。
pub fn read_entry() -> Option<PortEntry> {
    simple_post_core::port_file::read_entry()
}

/// 读当前记录。文件不存在、读不了或反序列化失败时返回 `None`。
pub fn read() -> Option<PortRecord> {
    read_entry().map(|entry| entry.record)
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

#[cfg(test)]
#[path = "port_registry_fingerprint_test.rs"]
mod fingerprint_tests;
