//! 「不闪终端窗口」的源码守卫。
//!
//! 漏掉一次 `hide_console_*` 没有任何可观察的返回值：它只在真实 Windows 上表现为一扇
//! 闪过的黑框，跑单测永远绿。故这里按源码形状钉住每一处控制台 spawn。

/// `#[cfg(target_os = "windows")]` / `#[cfg(windows)]` 起、到对称的 not 分支前的片段。
fn windows_blocks(src: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(start) = ["#[cfg(target_os = \"windows\")]", "#[cfg(windows)]"]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
    {
        let body = &rest[start..];
        let end = [
            "#[cfg(not(target_os = \"windows\"))]",
            "#[cfg(not(windows))]",
        ]
        .iter()
        .filter_map(|marker| body.find(marker))
        .min()
        .unwrap_or(body.len());
        blocks.push(&body[..end]);
        rest = &body[end..];
    }
    blocks
}

#[test]
fn windows_spawn_sites_hide_their_console() {
    for (name, src) in [
        (
            "port_registry/proc.rs",
            include_str!("port_registry/proc.rs"),
        ),
        ("update_api.rs", include_str!("update_api.rs")),
    ] {
        for block in windows_blocks(src) {
            if block.contains("process::Command::new(") {
                assert!(
                    block.contains("hide_console_std(") || block.contains("hide_console_tokio("),
                    "{name}: a Windows spawn block must call hide_console_*, otherwise every \
                     run flashes a console window on the desktop"
                );
            }
        }
    }

    // daemon spawn 没有 cfg 分支：它在所有平台上都走同一条路径，标志由 helper 自己隔离。
    let supervisor = include_str!("daemon_supervisor.rs");
    assert!(
        supervisor.contains("hide_console_tokio(&mut cmd)"),
        "daemon_supervisor.rs must hide the daemon's console window"
    );
}

#[cfg(windows)]
#[test]
fn the_flag_is_create_no_window() {
    // 0x08000000 是 Win32 的 `CREATE_NO_WINDOW`。写成别的值不会编译失败，只会继续闪。
    assert_eq!(super::CREATE_NO_WINDOW, 0x0800_0000);
}
