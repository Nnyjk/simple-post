//! `update_api.rs` 的两道门与命令执行。
//!
//! 全部打到本机进程：`SIMPLE_POST_UPDATE_COMMAND` 指向一条无关紧要的 `echo`/`sleep`，
//! 不碰真镜像也不碰网络。环境变量是全进程共享的单个槽位，故每个测试先拿一把进程级
//! mutex（手法照 `port_registry_test.rs`）。

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use super::*;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 在持有进程级锁的前提下设置这两个环境变量，drop 时恢复原值。
struct ScopedEnv {
    _guard: MutexGuard<'static, ()>,
    prev_command: Option<OsString>,
    prev_shape: Option<OsString>,
}

impl ScopedEnv {
    fn with(command: Option<&str>, deployment_shape: Option<&str>) -> Self {
        // 先拿锁，*再*换变量：顺序反了就会出现「换了变量、锁还没拿到」的窗口。
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_command = std::env::var_os(UPDATE_COMMAND_ENV);
        let prev_shape = std::env::var_os(DEPLOYMENT_ENV);
        // SAFETY：锁保证没有别的线程看得见中间状态。
        unsafe {
            match command {
                Some(value) => std::env::set_var(UPDATE_COMMAND_ENV, value),
                None => std::env::remove_var(UPDATE_COMMAND_ENV),
            }
            match deployment_shape {
                Some(value) => std::env::set_var(DEPLOYMENT_ENV, value),
                None => std::env::remove_var(DEPLOYMENT_ENV),
            }
        }
        Self {
            _guard: guard,
            prev_command,
            prev_shape,
        }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        unsafe {
            match &self.prev_command {
                Some(value) => std::env::set_var(UPDATE_COMMAND_ENV, value),
                None => std::env::remove_var(UPDATE_COMMAND_ENV),
            }
            match &self.prev_shape {
                Some(value) => std::env::set_var(DEPLOYMENT_ENV, value),
                None => std::env::remove_var(DEPLOYMENT_ENV),
            }
        }
        // `_guard` 在本方法之后才 drop，把锁让给下一个测试。
    }
}

#[tokio::test]
async fn apply_is_disabled_without_a_configured_command() {
    let _env = ScopedEnv::with(None, None);

    let deployment = deployment(true);
    assert!(!deployment.apply_enabled);
    assert!(matches!(apply(true).await, Err(ApplyError::Disabled)));
}

#[tokio::test]
async fn apply_is_disabled_without_a_password() {
    let _env = ScopedEnv::with(Some("echo hi"), None);

    // 命令配了，但这一进程没有口令模式 —— `serve-web` 未设口令时任何人可访问，
    // 那种部署里不能给「执行命令」开一个入口。
    let deployment = deployment(false);
    assert!(!deployment.apply_enabled);
    assert!(matches!(apply(false).await, Err(ApplyError::Disabled)));
}

#[tokio::test]
async fn apply_runs_the_configured_command() {
    let _env = ScopedEnv::with(Some("echo hi"), None);

    let deployment = deployment(true);
    assert!(deployment.apply_enabled, "两道门都成立时应可执行");

    let result = apply(true).await.expect("command runs");
    assert_eq!(result.command, "echo hi");
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("hi"), "stdout: {:?}", result.stdout);
}

#[tokio::test]
async fn a_second_apply_while_one_is_running_is_busy() {
    let slow = if cfg!(windows) {
        "ping -n 3 127.0.0.1 > NUL"
    } else {
        "sleep 2"
    };
    let _env = ScopedEnv::with(Some(slow), None);

    let first = tokio::spawn(async { apply(true).await });
    // 等第一条真的占住那道门：`try_lock` 失败才会返回 409，而它失败的前提是
    // 第一个已经拿到锁并在跑。
    tokio::time::sleep(Duration::from_millis(500)).await;

    let second = apply(true).await;
    assert!(matches!(second, Err(ApplyError::Busy)));

    // 把第一轮等完，门才会在下一个测试开始前释放。
    let first = first.await.expect("join first apply");
    assert_eq!(first.expect("first apply finishes").exit_code, 0);
}

#[tokio::test]
async fn docker_shape_is_reported_when_declared() {
    let _env = ScopedEnv::with(None, Some("docker"));

    let deployment = deployment(true);
    assert_eq!(deployment.shape, "docker");
    assert_eq!(deployment.image, Some(DOCKER_IMAGE));
    assert_eq!(deployment.command, Some(DOCKER_COMMAND));
    // 形态与「能不能执行」是两件事：这条部署没声明命令，故仍不可执行。
    assert!(!deployment.apply_enabled);
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ApplyError::Disabled.code(), "update_disabled");
    assert_eq!(ApplyError::Busy.code(), "update_in_progress");
    assert!(!ApplyError::Disabled.message().is_empty());
}

#[test]
fn output_is_clamped_on_a_char_boundary() {
    let short = b"hi";
    assert_eq!(clamp(short), "hi");

    let long = vec![b'a'; OUTPUT_LIMIT_BYTES + 100];
    assert_eq!(clamp(&long).len(), OUTPUT_LIMIT_BYTES);
}
