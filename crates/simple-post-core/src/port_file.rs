//! `mcp.port` 端口文件：路径解析、记录形状与读写。
//!
//! daemon 启动时把一条小 JSON 记录（`mcp.port`）写到平台数据目录。GUI 启动时读它
//! 以发现 daemon 端点；独立的 `simple-post-mcp` 桥接二进制读同一个文件，得知把
//! JSON-RPC 流量转到哪里。
//!
//! 它住在 core 里而不是 server crate 里，因为**读它的一方不止一个**：
//! `simple-post-server` 写它，`simple-post-mcp` 读它，而后者不能依赖前者（依赖方向是
//! 单向的：`server → core`、`mcp → core`）。规则若各写一遍，两处就会漂——漂了之后
//! 桥接二进制会连不上一个正在服务的 daemon，而症状离原因很远。
//!
//! 文件路径：
//!   * Windows: `%APPDATA%\simple-post\mcp.port`
//!   * macOS:   `~/Library/Application Support/simple-post/mcp.port`
//!   * Linux:   `$XDG_DATA_HOME/simple-post/mcp.port`（或
//!     `~/.local/share/simple-post/mcp.port`）
//!
//! 测试下用 `SIMPLE_POST_DATA_DIR` 覆盖基准目录，免得单测踩掉真实用户的端口文件。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// 数据目录内的注册文件名。导出，好让读它的一方不必重新推导目录结构就能打开
/// 同一路径。
pub const FILE_NAME: &str = "mcp.port";

/// 测试用数据目录覆盖环境变量。设置后 `port_file_path()` 用
/// `$SIMPLE_POST_DATA_DIR/simple-post/mcp.port` 而非平台默认。
pub const DATA_DIR_ENV: &str = "SIMPLE_POST_DATA_DIR";

/// 解析基准目录：环境变量覆盖 → `dirs::data_dir()`。返回的路径是容纳
/// `simple-post/mcp.port` 的父目录。
fn base_dir() -> Result<PathBuf, AppError> {
    if let Some(s) = std::env::var_os(DATA_DIR_ENV) {
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    dirs::data_dir().ok_or_else(|| {
        crate::error::AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "platform data_dir is unavailable",
        ))
    })
}

/// 公开：daemon 写数据的目录（也是 `mcp.port` 文件所在处）。公开是因为 daemon 与
/// GUI 需要在读它的一方将要打开的同一路径下打开 SQLite 池。
///
/// 设置 `SIMPLE_POST_DATA_DIR` 时原样返回它；否则回退 `dirs::data_dir()`，**不带**
/// `com.simple-post` 段——需要该后缀的调用方自己 `.join("com.simple-post")`。
pub fn data_dir() -> Result<PathBuf, AppError> {
    base_dir()
}

/// 在任意基准目录下算出端口文件路径。供不想碰用户真实数据目录的测试使用。
pub fn port_file_path_at(base: &Path) -> Result<PathBuf, AppError> {
    let dir = base.join("simple-post");
    std::fs::create_dir_all(&dir).map_err(crate::error::AppError::Io)?;
    Ok(dir.join(FILE_NAME))
}

/// `mcp.port` 的绝对路径。必要时创建 `simple-post` 父目录（幂等——全新安装上第一个
/// 写它的就是 daemon 进程）。
pub fn port_file_path() -> Result<PathBuf, AppError> {
    port_file_path_at(&base_dir()?)
}

/// 端口文件里的那条记录：形状 + 它自己的时间戳 + 它服务的配置的指纹。
///
/// daemon 启动时写入的内容。renderer 从不看到它；桥接二进制只取其中的 `port`，
/// 其余原样转发。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortRecord {
    /// daemon 进程 PID。0 表示「尚不知道」（文件写入与 `child.id()` 往返之间会
    /// 短暂出现）。
    pub pid: u32,
    /// daemon 正在监听的 TCP 端口。
    pub port: u16,
    /// 绑定接口，如 `127.0.0.1`。读它的一方原样使用。
    pub bind: String,
    /// daemon 写入本记录的 Unix epoch 秒。便于 GUI 在没有外部状态的情况下
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
/// [`PortRecord`] 在 daemon 侧被逐字段构造，给它加字段就得改那个文件。顶层键的
/// 形状不动，故读这个文件的第三方（桥接二进制、`scripts/mcp-repl.mjs`）解析得了。
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
    /// 监管者会在 `child.id()` 已知后重写一次。
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

/// 读当前记录与它服务的配置的指纹。文件不存在、读不了或反序列化失败（损坏 / 写
/// 了一半——按「没有 daemon」处理，比 panic 安全）时返回 `None`。
pub fn read_entry() -> Option<PortEntry> {
    let path = port_file_path().ok()?;
    let bytes = std::fs::read(&path).ok()?;
    match parse_entry(&bytes) {
        Ok(entry) => Some(entry),
        Err(e) => {
            tracing::debug!(
                target: "port_registry",
                "mcp.port present but unparseable, treating as missing: {e}"
            );
            None
        }
    }
}

/// 读当前记录。文件不存在、读不了或反序列化失败时返回 `None`。
pub fn read() -> Option<PortRecord> {
    read_entry().map(|entry| entry.record)
}

/// 读端口文件里的端口号。读它的一方（桥接二进制）只需要这一个数。
///
/// 与 [`read_entry`] 的差别有两处：它**不**创建 `simple-post` 父目录（读路径不该在
/// 用户的磁盘上留下目录），它也只取端口号，不关心别的字段。
pub fn read_port() -> Option<u16> {
    let path = base_dir().ok()?.join("simple-post").join(FILE_NAME);
    let bytes = std::fs::read(path).ok()?;
    parse_entry(&bytes).ok().map(|entry| entry.record.port)
}

/// 当前 Unix epoch 秒。
pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
