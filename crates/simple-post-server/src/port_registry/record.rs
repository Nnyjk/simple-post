//! 端口文件里的那条记录：形状 + 它自己的时间戳 + 它服务的配置的指纹。

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

/// 端口文件里那条记录的旁边那一项：daemon 服务的 HTTP 配置的指纹所放的键名。
///
/// 指纹是同一个 JSON 对象里的另一个顶层键，**不在** [`PortRecord`] 结构里。
/// [`PortRecord`] 在 `lib.rs`（daemon 侧）被逐字段构造，给它加字段就得改那个文件。顶层键的
/// 形状不动，故读这个文件的第三方（proxy 二进制、`scripts/mcp-repl.mjs`）解析得了。
pub const CONFIG_FINGERPRINT_KEY: &str = "config_fingerprint";

/// 端口文件里的一条完整内容：那条记录 + 它服务的 HTTP 配置的指纹。
#[derive(Debug, Clone)]
pub struct PortEntry {
    /// 记录本身：PID、端口、绑定地址、写入时刻、数据目录、版本。
    pub record: PortRecord,
    /// 这个 daemon 服务的 HTTP 配置的指纹。旧记录里没有这一项，故为 `None`——
    /// 那表示「判断不了这个 daemon 服务的是哪份配置」。
    pub config_fingerprint: Option<String>,
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

/// 把一条记录与它的配置指纹序列化成端口文件的内容。
///
/// 指纹是同一个 JSON 对象里的另一个顶层键，故 `port` / `pid` 这些键的位置与形状
/// 一个字没动。没有指纹时那一项整个不出现——daemon 侧自己写的记录就是这样。
pub fn serialize_entry(
    rec: &PortRecord,
    config_fingerprint: Option<&str>,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut value = serde_json::to_value(rec)?;
    if let Some(fingerprint) = config_fingerprint {
        if let Some(map) = value.as_object_mut() {
            map.insert(
                CONFIG_FINGERPRINT_KEY.to_string(),
                serde_json::Value::String(fingerprint.to_string()),
            );
        }
    }
    serde_json::to_vec(&value)
}

/// 解析端口文件的内容。
///
/// 记录本身解析不出来就返回 `Err`：那说明文件坏了，与「文件不存在」是两件事。
/// 没有配置指纹的记录里那一项是 `None`——「判断不了」。
pub fn parse_entry(bytes: &[u8]) -> Result<PortEntry, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let config_fingerprint = value
        .get(CONFIG_FINGERPRINT_KEY)
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let record: PortRecord = serde_json::from_value(value)?;
    Ok(PortEntry {
        record,
        config_fingerprint,
    })
}

pub(super) fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
