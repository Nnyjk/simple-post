//! 「配置一致就复用、不一致才重启」这条判断的回归网。
//!
//! `daemon_supervisor.rs` 的同级 `#[path]` 模块（以 `mod reuse_tests` 挂载）。
//! 断言的对象是纯函数与源码形状：不 spawn 进程、不绑端口、不碰 `mcp.port`。

use super::*;

/// 从 `src` 里截出 `marker` 之后的那一段，直到 `stop` 之前。
fn text_after<'a>(src: &'a str, marker: &str, stop: &str) -> &'a str {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker:?} must exist in daemon_supervisor.rs"))
        + marker.len();
    let rest = &src[start..];
    let end = rest
        .find(stop)
        .unwrap_or_else(|| panic!("stop marker {stop:?} must exist in daemon_supervisor.rs"));
    &rest[..end]
}

#[test]
fn matching_fingerprints_adopt_the_running_daemon() {
    // 指纹一致：那个 daemon 正在服务我要的这份配置，收养它。
    assert_eq!(reuse_decision(Some("fp-1"), "fp-1"), ReuseDecision::Adopt);
}

#[test]
fn different_fingerprints_restart_the_running_daemon() {
    // 指纹不同：那个 daemon 服务的不是我要的这份配置，重启。
    assert_eq!(reuse_decision(Some("fp-1"), "fp-2"), ReuseDecision::Restart);
}

#[test]
fn a_record_without_a_fingerprint_cannot_be_judged_and_therefore_restarts() {
    // 旧端口文件里没有指纹这一项：判断不了，而判断不了的方向是重启。
    assert_eq!(reuse_decision(None, "fp-1"), ReuseDecision::Restart);
}

#[test]
fn no_adopt_branch_tears_the_running_daemon_down() {
    // 收养的意思就是不 kill、不重拉：收养那一支里不许出现 teardown。
    // 这一条是本任务的核心回归点——启动时无条件 teardown 会杀掉桌面应用起的
    // daemon，桌面侧的 MCP 客户端随之全部掉线。
    let src = include_str!("daemon_supervisor.rs");
    let mut rest = src;
    let mut checked = 0usize;
    while let Some(pos) = rest.find("ReuseDecision::Adopt =>") {
        let arm = &rest[pos + "ReuseDecision::Adopt =>".len()..];
        let end = arm
            .find("ReuseDecision::")
            .expect("an adopt branch must end at the next match branch");
        let arm = &arm[..end];
        assert!(
            !arm.contains("teardown"),
            "an adopt branch tears the running daemon down:\n{arm}"
        );
        checked += 1;
        rest = &rest[pos + "ReuseDecision::Adopt =>".len()..];
    }
    assert!(
        checked >= 2,
        "both reuse checks must have an adopt branch, got {checked}"
    );
}

#[test]
fn the_restart_branch_tears_the_old_daemon_down() {
    // 重启的那一支必须真的杀掉那个 daemon：只是不收养它却不杀，旧 daemon 会带着
    // 它的端口继续服务，而端口文件已经指向了新 PID。
    let src = include_str!("daemon_supervisor.rs");
    let arm = text_after(src, "ReuseDecision::Restart =>", "ReuseDecision::");
    assert!(
        arm.contains("teardown"),
        "the restart branch must tear the old daemon down:\n{arm}"
    );
}

#[test]
fn the_spawn_path_writes_the_configuration_fingerprint_into_the_port_record() {
    // 端口记录里没有指纹，下一个监管者就判断不了这个 daemon 服务的是哪份配置，
    // 于是每次启动都会把它重启一遍。
    let src = include_str!("daemon_supervisor.rs");
    assert!(
        src.contains("write_with_fingerprint"),
        "the spawn path must write the configuration fingerprint into the port record"
    );
}
