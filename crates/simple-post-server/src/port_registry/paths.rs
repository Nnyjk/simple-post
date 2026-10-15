//! `mcp.port` 落在哪个路径：平台目录解析 + 记录文件名的拼装。

use std::path::{Path, PathBuf};

use crate::error::AppError;

/// 数据目录内的注册文件名。导出，好让 proxy 二进制不必重新推导目录结构就能打开
/// 同一路径。
pub const FILE_NAME: &str = "mcp.port";

/// 测试用数据目录覆盖环境变量。设置后 `port_file_path()` 用
/// `$SIMPLE_POST_DATA_DIR/simple-post/mcp.port` 而非平台默认。
pub(super) const DATA_DIR_ENV: &str = "SIMPLE_POST_DATA_DIR";

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
/// GUI 需要在 proxy 将要发现的同一路径下打开 SQLite 池。
///
/// 设置 `SIMPLE_POST_DATA_DIR` 时原样返回它；否则回退 `dirs::data_dir()`，**不带**
/// `com.simple-post` 段——需要该后缀的调用方自己 `.join("com.simple-post")`。
pub fn data_dir() -> Result<PathBuf, AppError> {
    base_dir()
}

/// 内部：在任意基准目录下算出端口文件路径。供不想碰用户真实数据目录的测试使用。
pub(super) fn port_file_path_at(base: &Path) -> Result<PathBuf, AppError> {
    let dir = base.join("simple-post");
    std::fs::create_dir_all(&dir).map_err(crate::error::AppError::Io)?;
    Ok(dir.join(FILE_NAME))
}

/// `mcp.port` 的绝对路径。必要时创建 `simple-post` 父目录（幂等——全新安装上第一个
/// 写它的就是 daemon 进程）。
pub fn port_file_path() -> Result<PathBuf, AppError> {
    port_file_path_at(&base_dir()?)
}
