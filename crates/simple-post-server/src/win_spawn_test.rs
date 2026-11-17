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

/// 受检的、带 `#[cfg(windows)]` spawn 的源文件（相对 `src/`）。
///
/// `include_str!` 是编译期的，清单只能手写；`every_windows_spawn_site_is_guarded` 用
/// `std::fs` 走目录，把「漏登记」这件事变成一条会红的断言。
const CHECKED_SOURCES: &[(&str, &str)] = &[
    (
        "port_registry/proc.rs",
        include_str!("port_registry/proc.rs"),
    ),
    ("update_api.rs", include_str!("update_api.rs")),
];

#[test]
fn windows_spawn_sites_hide_their_console() {
    for (name, src) in CHECKED_SOURCES {
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

/// `src/` 下每个 `.rs`（测试文件除外），键为相对 `src/` 的路径。
fn rust_sources(dir: &std::path::Path, rel: &str, out: &mut Vec<(String, String)>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("读不到 {}: {e}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            rust_sources(&path, &rel_path, out);
        } else if name.ends_with(".rs") && !name.ends_with("_test.rs") {
            out.push((rel_path, std::fs::read_to_string(&path).unwrap_or_default()));
        }
    }
}

/// 清单与源码必须一致——双向。
///
/// ① 任何落在 `#[cfg(windows)]` 块里的 `process::Command::new(` 都必须来自清单里的文件，
///    否则新加的那个 spawn 永远没人检查它有没有 `hide_console_*`（表现为桌面上一扇闪过的
///    黑框，跑单测永远绿）。
/// ② 清单里的文件必须仍然含这样的块，防「spawn 挪走了、清单还在」——那种情况下
///    `windows_spawn_sites_hide_their_console` 会变成空转。
#[test]
fn every_windows_spawn_site_is_guarded() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    rust_sources(&src_dir, "", &mut sources);

    let guarded: Vec<String> = CHECKED_SOURCES
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect();
    let mut missed: Vec<String> = Vec::new();
    for (rel, text) in &sources {
        if guarded.contains(rel) {
            continue;
        }
        if windows_blocks(text)
            .iter()
            .any(|block| block.contains("process::Command::new("))
        {
            missed.push(rel.clone());
        }
    }
    assert!(
        missed.is_empty(),
        "这些文件在 #[cfg(windows)] 块里 spawn 进程但不在 CHECKED_SOURCES 里：{missed:?}"
    );

    let mut stale: Vec<String> = Vec::new();
    for (name, _) in CHECKED_SOURCES {
        let Some((_, text)) = sources.iter().find(|(rel, _)| rel == name) else {
            stale.push(format!("{name}（文件已不存在）"));
            continue;
        };
        let still_spawns = windows_blocks(text)
            .iter()
            .any(|block| block.contains("process::Command::new("));
        if !still_spawns {
            stale.push(format!(
                "{name}（已没有任何 #[cfg(windows)] spawn 块——从清单里删掉）"
            ));
        }
    }
    assert!(stale.is_empty(), "CHECKED_SOURCES 已腐烂：{stale:?}");
}

#[cfg(windows)]
#[test]
fn the_flag_is_create_no_window() {
    // 0x08000000 是 Win32 的 `CREATE_NO_WINDOW`。写成别的值不会编译失败，只会继续闪。
    assert_eq!(super::CREATE_NO_WINDOW, 0x0800_0000);
}
