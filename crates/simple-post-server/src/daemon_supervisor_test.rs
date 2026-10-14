//! Pure-logic tests for the daemon lifecycle decisions.
//!
//! Sibling `#[path]` module of `daemon_supervisor.rs` (mounted as
//! `mod lifecycle_tests`), split out because these assertions must never
//! be able to spawn a daemon, bind a port, or delete the developer's
//! real `mcp.port`. Everything exercised here is a total function over
//! values the caller already holds:
//!
//!   * [`teardown_targets`] — which PIDs a lifecycle verb must kill, and
//!     through which channel. This is the regression net for the defect
//!     where `restart` adopted and re-adopted the same live daemon.
//!   * [`start_is_noop`] — the manual ON switch's idempotency rule.
//!   * [`watcher_owns_state`] — the guard that stops a deliberate
//!     teardown's own kill from reporting a spurious crash.
//!
//! Anything that needs a real process (the spawn path, the bounded
//! teardown wait) is out of scope by design and stays untested here
//! rather than faked.

use super::*;

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
    // The core regression. `ensure_started`'s reuse path records the
    // PID of a daemon it never spawned, so `state.child` is `None`.
    // The pre-fix teardown only looked at the handle, found nothing,
    // and restart silently re-adopted the same live process.
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
    // Daemon we both spawned and see in the record: one kill, and the
    // handle is the cheaper reap (try_wait, no signal round-trip).
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
    // Possible when a stale record outlived a daemon we still own a
    // handle for; both have to go or the restart keeps serving the old
    // port.
    let targets = teardown_targets(Some(11), Some(22));
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].pid, 11);
    assert!(targets[0].via_handle);
    assert_eq!(targets[1].pid, 22);
    assert!(!targets[1].via_handle);
}

#[test]
fn nothing_tracked_means_nothing_to_kill() {
    // The `stop`-when-never-started path must be a safe no-op, not an
    // error and not a kill aimed at PID 0.
    assert!(teardown_targets(None, None).is_empty());
}

#[test]
fn zero_pids_are_filtered_out() {
    // `child.id()` is `None` immediately after spawn and the port
    // record carries `pid: 0` until the round-trip completes. `0` is
    // never a real PID, and `taskkill /F /PID 0` would be a wild kill.
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
    // Clicking "start" on a live-but-hung daemon is exactly the case
    // that must not report success while doing nothing.
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
    // "Adopted, then lost the port record" — liveness cannot be
    // confirmed, so the honest answer is a fresh spawn rather than
    // reporting an identity we cannot verify.
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
// Source-level guard
// -------------------------------------------------------------------------
//
// The `AppHandle`-free spawn path is a **structural** invariant with no
// compile-time enforcement: nothing stops a later edit from threading a
// Tauri type back through `ensure_started_with`. When that happens the
// crate still compiles, `cargo build -p simple-post` still produces a
// working binary, every existing test still passes — and then the
// `simple-post` **test** binary stops being able to load at all on
// Windows:
//
//   simple_post_lib-<hash>.exe : exit 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)
//
// The unresolvable import is `comctl32.dll!TaskDialogIndirect`, exported
// only by comctl32 **v6** — a side-by-side assembly resolved through an
// activation context, not by the v5 copy in System32. It arrives with
// the `tao`/`muda` windowing stack that an owned `AppHandle` keeps
// reachable from this module. The failure surfaces as "the whole crate's
// tests silently stop running", which reads like an environment problem
// rather than a code problem — it cost a long bisect to find.
//
// So this is a source scan, on purpose, exactly like
// `no_gate_is_passed_the_literal_global` in
// `crates/simple-post-mcp/src/mcp/mod_test.rs`: the bug has no behavioural
// signature to assert on, because the behaviour is "this test binary
// cannot start".
//
// It reads its sibling source at compile time, so it cannot drift the way
// a hand-maintained allowlist can.

/// Extract a top-level `fn` body by brace matching, starting at its
/// signature. Returns `None` when the signature is absent.
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
    // NOTE the trailing `(`. `ensure_started_with` is a substring of
    // `ensure_started_without_app_handle` — "with" + "out" spells
    // "without" — so a search without the paren silently matches the
    // WRONG function and this test passes no matter what is injected.
    // The same trap applies to `ensure_started` vs `ensure_started_…`.
    let body = fn_body(src, "async fn ensure_started_with(")
        .expect("ensure_started_with must exist in daemon_supervisor.rs");

    // `AppHandle` by name, plus the Tauri traits/paths that would drag
    // the windowing runtime in behind it. A type alias or a fully
    // qualified `tauri::AppHandle` is caught by the second check.
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
    // The counterpart to the scan above: `ensure_started` MUST still take
    // an `AppHandle`, because that is the GUI's entry point. Without this
    // the previous test could be satisfied by deleting the parameter and
    // silently breaking every caller.
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
