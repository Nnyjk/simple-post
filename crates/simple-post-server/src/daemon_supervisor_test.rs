//! daemon 生命周期决策的纯逻辑测试。
//!
//! `daemon_supervisor.rs` 的同级 `#[path]` 模块（以 `mod lifecycle_tests` 挂载），
//! 之所以拆出来，是为了让这些断言绝无可能 spawn daemon、绑定端口或删掉开发者真实的
//! `mcp.port`。这里跑的全是调用方已经持有的值之上的全函数：
//!
//!   * [`teardown_targets`] —— 一个生命周期动词必须杀哪些 PID、走哪条通道。
//!   * [`start_is_noop`] —— 手动 ON 开关的幂等规则。
//!   * [`watcher_owns_state`] —— 阻止「有意 teardown 自己的 kill」被报成崩溃的守卫。
//!
//! 需要真实进程的（spawn 路径、有界 teardown 等待）按设计不在此列，宁可不测也不伪造。

use super::*;
// 这三个纯函数住在子模块里，本文件从那里取。
use super::probe::start_is_noop;
use super::reap::teardown_targets;
use super::watcher::watcher_owns_state;

#[test]
fn owned_child_is_killed_through_its_handle() {
    let targets = teardown_targets(Some(4242), None);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].pid, 4242);
    assert!(
        targets[0].via_handle,
        "a daemon we spawned must be reaped via the Child handle"
    );
}

#[test]
fn adopted_daemon_is_killed_by_pid_even_without_a_handle() {
    // 核心回归点。`ensure_started` 的复用路径记下一个它从未 spawn 的 daemon 的 PID，
    // 故 `state.child` 是 `None`。只查句柄的 teardown 什么都找不到，重启便静默地重新
    // 收养了同一个活进程。
    let targets = teardown_targets(None, Some(4242));
    assert_eq!(targets.len(), 1, "an adopted daemon must still be targeted");
    assert_eq!(targets[0].pid, 4242);
    assert!(
        !targets[0].via_handle,
        "there is no handle, so the PID has to be killed out-of-band"
    );
}

#[test]
fn same_pid_is_targeted_once_via_the_handle() {
    // 既 spawn 又在记录里见到的 daemon：只杀一次，且句柄是更廉价的回收方式（try_wait，
    // 无需信号往返）。
    let targets = teardown_targets(Some(4242), Some(4242));
    assert_eq!(
        targets,
        vec![TeardownTarget {
            pid: 4242,
            via_handle: true
        }]
    );
}

#[test]
fn distinct_pids_are_both_targeted() {
    // 陈旧记录的寿命长于我们仍持有句柄的 daemon 时会出现；两个都得杀，否则重启后仍在服务
    // 旧端口。
    let targets = teardown_targets(Some(11), Some(22));
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].pid, 11);
    assert!(targets[0].via_handle);
    assert_eq!(targets[1].pid, 22);
    assert!(!targets[1].via_handle);
}

#[test]
fn nothing_tracked_means_nothing_to_kill() {
    // 从未启动过就 stop 的路径必须是安全的 no-op：既不报错，也不把杀招对准 PID 0。
    assert!(teardown_targets(None, None).is_empty());
}

#[test]
fn zero_pids_are_filtered_out() {
    // spawn 后 `child.id()` 立刻返回 `None`，端口记录在往返完成前也带 `pid: 0`。`0`
    // 从不是真 PID，而 `taskkill /F /PID 0` 是一场乱杀。
    assert!(teardown_targets(Some(0), Some(0)).is_empty());
    let mixed = teardown_targets(Some(0), Some(77));
    assert_eq!(mixed.len(), 1);
    assert_eq!(mixed[0].pid, 77);
    assert!(!mixed[0].via_handle);
}

#[test]
fn start_is_a_noop_only_for_a_healthy_tracked_daemon() {
    assert!(start_is_noop(DaemonStatus::Running, Some(4173), true));
}

#[test]
fn start_respawns_when_the_daemon_is_not_healthy() {
    // 对一个活着但卡死的 daemon 点「启动」，正是那种「不能什么都不做却报告成功」的情形。
    assert!(!start_is_noop(DaemonStatus::Running, Some(4173), false));
    assert!(!start_is_noop(DaemonStatus::Unhealthy, Some(4173), false));
    assert!(!start_is_noop(
        DaemonStatus::Crashed { exit_code: Some(1) },
        Some(4173),
        true
    ));
    assert!(!start_is_noop(DaemonStatus::Restarting, Some(4173), true));
}

#[test]
fn start_respawns_when_running_without_a_tracked_port() {
    // 「已收养、随后丢了端口记录」——无法确认存活，故诚实的答案是重新 spawn，而不是报一个
    // 我们无法核实的身份。
    assert!(!start_is_noop(DaemonStatus::Running, None, true));
}

#[test]
fn watcher_only_claims_state_it_still_owns() {
    assert!(watcher_owns_state(Some(4242), 4242));
    assert!(
        !watcher_owns_state(Some(43), 4242),
        "a restart replaced the pid"
    );
    assert!(
        !watcher_owns_state(None, 4242),
        "teardown clears pid before killing, so the watcher woken by our \
         own kill must not report a crash"
    );
}

// -------------------------------------------------------------------------
// 源码级守卫
// -------------------------------------------------------------------------
//
// 「spawn 路径不含 `AppHandle`」是一条**结构性**不变量，没有任何编译期强制：没什么能
// 阻止后来某次改动把 Tauri 类型重新串回 `ensure_started_with`。一旦发生，本 crate 仍
// 编译，`cargo build -p simple-post` 仍产出可用的二进制，所有既有测试仍全绿——然后
// `simple-post` 的**测试**二进制在 Windows 上彻底加载不了：
//
//   simple_post_lib-<hash>.exe : exit 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)
//
// 未解析的导入是 `comctl32.dll!TaskDialogIndirect`，只由 comctl32 **v6** 导出——经
// activation context 解析的 side-by-side 程序集，而非 `System32` 里的 v5 副本。它是
// 被一个被持有的 `AppHandle` 留在本模块可达图里的 `tao` / `muda` 窗口栈带进来的。这个
// 故障表现为「整个 crate 的测试静默地不跑了」，看起来像环境问题而不是代码问题。
//
// 故这里刻意做源码扫描，与
// `crates/simple-post-mcp/src/mcp/mod_test.rs` 的 `no_gate_is_passed_the_literal_global`
// 同理：这个缺陷没有可断言的行为特征，因为它的行为就是「这个测试二进制起不来」。
//
// 它在编译期读取同级源码，故不会像手工维护的白名单那样随时间腐化。

/// 从签名开始用括号配对提取一个顶层 `fn` 的函数体。签名不存在时返回 `None`。
fn fn_body<'a>(src: &'a str, signature: &str) -> Option<&'a str> {
    let start = src.find(signature)? + signature.len();
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut seen_open = false;
    for (i, b) in bytes[start..].iter().enumerate() {
        match b {
            b'{' => {
                depth += 1;
                seen_open = true;
            }
            b'}' => {
                depth -= 1;
                if seen_open && depth == 0 {
                    return Some(&src[start..start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn the_shared_spawn_path_stays_free_of_tauri() {
    let src = include_str!("daemon_supervisor.rs");
    // 注意结尾那个 `(`。`ensure_started_with` 是 `ensure_started_without_app_handle`
    // 的子串——"with" + "out" 拼出 "without"——所以不带括号去搜会悄悄匹配到**错误**的
    // 函数，于是无论注入什么这个测试都通过。同样的陷阱适用于 `ensure_started` 与
    // `ensure_started_…`。
    let body = fn_body(src, "async fn ensure_started_with(")
        .expect("ensure_started_with must exist in daemon_supervisor.rs");

    // 按名字找 `AppHandle`，再加上会把窗口运行时一并拖进来的那些 Tauri trait / 路径。
    // 类型别名或完全限定的 `tauri::AppHandle` 由第二项检查（`tauri::`）覆盖。
    for forbidden in ["AppHandle", "tauri::", "Emitter", "EmitterExt"] {
        assert!(
            !body.contains(forbidden),
            "ensure_started_with references `{forbidden}`. The shared spawn path must \
             stay Tauri-free or the --lib test binary gains comctl32!TaskDialogIndirect \
             and stops loading (0xc0000139). Hand the AppHandle back at the Tauri \
             binding instead — see the note on `struct Started`."
        );
    }
}

#[test]
fn the_tauri_binding_is_where_the_app_handle_is_accepted() {
    // 上面那次扫描的对照：`ensure_started` **必须**仍收 `AppHandle`，因为它才是 GUI 的
    // 入口。没有这条，上一个测试靠删掉该参数就能通过，同时静默弄坏每个调用方。
    let src = include_str!("daemon_supervisor.rs");
    let body = fn_body(src, "pub async fn ensure_started(")
        .expect("ensure_started must exist in daemon_supervisor.rs");
    let sig = &src[src
        .find("pub async fn ensure_started(")
        .expect("ensure_started signature")..];
    let sig = &sig[..sig.find('{').expect("ensure_started body opens")];
    assert!(
        sig.contains("AppHandle"),
        "the Tauri binding must still accept an AppHandle; got: {sig}"
    );
    assert!(
        body.contains("tauri_finish"),
        "the binding should hand the handle to Started::tauri_finish"
    );
}
