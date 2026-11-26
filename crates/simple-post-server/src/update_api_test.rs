//! `update_api.rs` 的两道门与命令执行。
//!
//! 全部打到本机进程：`SIMPLE_POST_UPDATE_COMMAND` 指向一条无关紧要的 `echo`/`sleep`，
//! 不碰真镜像也不碰网络。环境变量是全进程共享的单个槽位，故每个测试先拿一把进程级
//! mutex（手法照 `port_registry_test.rs`）。

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use simple_post_core::db::DbPool;

use super::*;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 一次性的空库。`preferences` 那一行要能读写，故这里真的建一个库，不 mock 掉 DB。
fn temp_pool(tag: &str) -> DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-update-api-test-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    simple_post_core::db::init(&dir).expect("db init")
}

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

    let deployment = deployment(&temp_pool("disabled-command"), true);
    assert!(!deployment.apply_enabled);
    assert!(matches!(apply(true).await, Err(ApplyError::Disabled)));
}

#[tokio::test]
async fn apply_is_disabled_without_a_password() {
    let _env = ScopedEnv::with(Some("echo hi"), None);

    // 命令配了，但这一进程没有口令模式 —— `serve-web` 未设口令时任何人可访问，
    // 那种部署里不能给「执行命令」开一个入口。
    let deployment = deployment(&temp_pool("disabled-password"), false);
    assert!(!deployment.apply_enabled);
    assert!(matches!(apply(false).await, Err(ApplyError::Disabled)));
}

#[tokio::test]
async fn apply_runs_the_configured_command() {
    let _env = ScopedEnv::with(Some("echo hi"), None);

    let deployment = deployment(&temp_pool("enabled"), true);
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

    let deployment = deployment(&temp_pool("docker"), true);
    assert_eq!(deployment.shape, "docker");
    // 版本随响应走：界面把它钉成「当前版本」，不需要再打一次 GitHub。
    assert_eq!(
        deployment.version,
        simple_post_core::update::current_version()
    );
    assert_eq!(deployment.image, Some(DOCKER_IMAGE));
    assert_eq!(deployment.command, Some(DOCKER_COMMAND));
    // 形态与「能不能执行」是两件事：这条部署没声明命令，故仍不可执行。
    assert!(!deployment.apply_enabled);
}

#[test]
fn docker_command_reads_the_compose_file_from_the_repository_root() {
    // 与 README.md 的 compose 一节、docker/README.md 的「更新」一节逐字一致：缺
    // `--project-directory .` 时 Compose 把 docker/ 当项目目录、读不到根目录 .env，
    // 缺 `-f docker/docker-compose.yml` 时在仓库根执行会报找不到配置文件。
    assert_eq!(
        DOCKER_COMMAND,
        concat!(
            "docker compose --project-directory . -f docker/docker-compose.yml pull simple-post",
            " && ",
            "docker compose --project-directory . -f docker/docker-compose.yml up -d simple-post",
        )
    );
}

#[test]
fn last_checked_at_is_stored_and_returned_by_deployment() {
    let pool = temp_pool("last-checked");
    // 空库：卡片因此说「尚未检查过更新」。
    assert_eq!(stored_last_checked_at(&pool), None);
    assert_eq!(deployment(&pool, false).last_checked_at, None);

    record_last_checked_at(&pool, "2026-11-26T01:02:03Z");
    assert_eq!(
        stored_last_checked_at(&pool).as_deref(),
        Some("2026-11-26T01:02:03Z")
    );
    // 部署形态带上这条时刻：web 端刷新浏览器后，设置页靠它写「上次检查」。
    assert_eq!(
        deployment(&pool, false).last_checked_at.as_deref(),
        Some("2026-11-26T01:02:03Z")
    );
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

    // OUTPUT_LIMIT_BYTES 不是 3 的倍数：按字节硬切会把「中」劈成两半，截断点要退到 8190。
    let multibyte = "中".repeat(OUTPUT_LIMIT_BYTES);
    let clamped = clamp(multibyte.as_bytes());
    assert_eq!(clamped.len(), OUTPUT_LIMIT_BYTES - OUTPUT_LIMIT_BYTES % 3);
    assert!(clamped.chars().all(|ch| ch == '中'));
}
