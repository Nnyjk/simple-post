//! `serve-web` 启动路径的回归网。
//!
//! `serve.rs` 的同级 `#[path]` 模块（以 `mod tests` 挂载）。断言的对象是源码
//! 形状：`serve-web` 启动时必须走「配置一致就复用」的那条路径。

#[test]
fn serve_web_reuses_a_running_daemon_instead_of_restarting_it() {
    // 走 `restart_without_app_handle` 会在启动时无条件杀掉端口文件里记着的
    // daemon——那个 daemon 可能是桌面应用起的，于是桌面侧的 MCP 客户端全部掉线，
    // 而 serve-web 拉起的是新 PID 新端口，桌面应用不知道它的存在。
    let src = include_str!("serve.rs");
    assert!(
        !src.contains("restart_without_app_handle"),
        "serve-web must not restart the daemon on startup"
    );
    assert!(
        src.contains("start_without_app_handle"),
        "serve-web must go through the reuse-first path"
    );
}
