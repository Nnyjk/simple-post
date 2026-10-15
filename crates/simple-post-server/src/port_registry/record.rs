//! 端口文件里的那条记录：形状 + 它自己的时间戳。

use serde::{Deserialize, Serialize};

/// daemon 启动时写入的内容。renderer 从不看到它；proxy 二进制只取其中的 `port`，
/// 其余原样转发。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortRecord {
    /// daemon 进程 PID。0 表示「尚不知道」（文件写入与 `child.id()` 往返之间会
    /// 短暂出现）。
    pub pid: u32,
    /// daemon 正在监听的 TCP 端口。
    pub port: u16,
    /// 绑定接口，如 `127.0.0.1`。proxy 原样使用。
    pub bind: String,
    /// daemon 写入本记录的 Unix epoch 秒。便于 GUI / proxy 在没有外部状态的情况下
    /// 判断「这个端口文件已经 12 小时了，daemon 大概死了」。
    pub started_at: u64,
    /// daemon 当时服务的数据目录绝对路径。仅供诊断，也让 GUI 能核对它与写入方
    /// 实例是否一致。
    pub data_dir: String,
    /// 写入方二进制的构建版本（`CARGO_PKG_VERSION`）。
    pub version: String,
}

impl PortRecord {
    /// 用 daemon 自己的元数据构造记录。首次写入时 `pid == 0` 可以接受；
    /// `ensure_started` 会在 `child.id()` 已知后重写一次。
    pub fn new(pid: u32, port: u16, bind: impl Into<String>, data_dir: impl Into<String>) -> Self {
        Self {
            pid,
            port,
            bind: bind.into(),
            started_at: now_secs(),
            data_dir: data_dir.into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

pub(super) fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
