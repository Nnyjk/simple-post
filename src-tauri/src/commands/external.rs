//! 用系统默认浏览器打开一条网页地址。
//!
//! 存在的理由是桌面壳不处理 `<a target="_blank">`：wry 的 `NewWindowRequested` 没有
//! 新窗口处理器时一律把请求置为已处理，故 GitHub 那两个入口在桌面端点了没有反应。
//! 浏览器形态不走这条路 —— 那边链接本来就成立，`WEB_ROUTES` 里因此没有条目。

use std::process::Command;

use crate::error::AppError;

/// 打开一个 http(s) 地址。
///
/// 只收 `http` / `https`：参数来自渲染层，放开别的协议就等于给页面一个「本机随便打开」
/// 的口子。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), AppError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::Validation(format!("只支持 http(s) 地址：{url}")));
    }
    launch(&url).map_err(|error| AppError::Validation(format!("无法启动浏览器：{error}")))
}

/// 启动子进程并放一个线程回收它。不等待：浏览器的生命周期不该拖住这条命令。
fn detached(program: &str, args: &[&str]) -> std::io::Result<()> {
    let mut child = Command::new(program).args(args).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Windows：`rundll32 url.dll,FileProtocolHandler` 把地址作为 argv 交给默认协议处理器。
/// 与 `cmd /C start` 的区别是地址不经过命令行解析，故里面的 `&` 不会被当成命令分隔符。
#[cfg(target_os = "windows")]
fn launch(url: &str) -> std::io::Result<()> {
    detached("rundll32", &["url.dll,FileProtocolHandler", url])
}

#[cfg(target_os = "macos")]
fn launch(url: &str) -> std::io::Result<()> {
    detached("open", &[url])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch(url: &str) -> std::io::Result<()> {
    detached("xdg-open", &[url])
}
