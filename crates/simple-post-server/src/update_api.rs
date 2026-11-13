//! 更新检查 / 更新执行的编排层。
//!
//! 三条 `/api/update/*` 路由的薄壳都在 `web_api/routes/update.rs`；这里放判断与执行本身。
//! 桌面端不调这些函数：桌面用 tauri updater 插件，更新由安装包负责。
//!
//! ## `apply` 的威胁模型
//!
//! 能触发更新的前提是拿到 bearer 口令。故它**必须**与口令模式绑定（[`deployment`] 的
//! `applyEnabled` 与 [`apply`] 用同一条判据），在未设口令的部署里永远回 403——否则任何
//! 一个能访问 `/api/*` 的人都能在这台机器上执行一条命令。
//!
//! 命令本身来自运维给定的环境变量 [`UPDATE_COMMAND_ENV`]，**不接受请求体传入**：接受
//! 请求体就等于把「执行任意命令」这个能力交给了调用方，而这道闸的语义是「按运维声明的
//! 那一种方式更新本机」。请求体最多是一个空 JSON 对象。

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Mutex;

pub use simple_post_core::update::UpdateCheck;

/// 容器镜像的引用。**全小写**：镜像仓库按小写建，`src/__tests__/release-image-ref.test.ts`
/// 钉着这一条。
pub const DOCKER_IMAGE: &str = "ghcr.io/nnyjk/simple-post:latest";

/// 容器形态给出的一行更新命令。它读的是同一份 compose 文件，故服务名与镜像标签必须与
/// 部署文档一致。
pub const DOCKER_COMMAND: &str =
    "docker compose pull simple-post && docker compose up -d simple-post";

/// 运维声明「这台机器上怎么更新」的环境变量。为空 = 未声明，`applyEnabled` 为 `false`。
pub const UPDATE_COMMAND_ENV: &str = "SIMPLE_POST_UPDATE_COMMAND";

/// 强制声明部署形态的环境变量，供没有 `/.dockerenv` 的容器运行时（如挂 socket 的
/// sidecar 场景）使用。
pub const DEPLOYMENT_ENV: &str = "SIMPLE_POST_DEPLOYMENT";

/// 更新命令的超时。
pub const APPLY_TIMEOUT: Duration = Duration::from_secs(120);

/// stdout / stderr 各自的上限。
pub const OUTPUT_LIMIT_BYTES: usize = 8 * 1024;

/// `GET /api/update/deployment` 的响应体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeployment {
    /// `"docker"` 或 `"binary"`。
    pub shape: &'static str,
    /// `shape == "docker"` 时的镜像引用，否则 `None`。
    pub image: Option<&'static str>,
    /// `shape == "docker"` 时给用户看的那行命令，否则 `None`。
    pub command: Option<&'static str>,
    /// 这一台机器能不能执行更新。由 [`UPDATE_COMMAND_ENV`] 与口令模式**同时**决定。
    pub apply_enabled: bool,
}

/// `POST /api/update/apply` 的响应体。不做成败判定：退出码与输出就是命令本身的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplyResult {
    /// 实际执行的命令串。
    pub command: String,
    /// 进程退出码；被信号终止、启动失败或超时时为 `-1`。
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 更新执行被拒的两种原因。两者都不带命令输出——命令根本没跑。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyError {
    /// 未声明更新命令，或部署没开口令模式。
    Disabled,
    /// 已经有一次更新在跑。
    Busy,
}

impl ApplyError {
    pub fn code(self) -> &'static str {
        match self {
            ApplyError::Disabled => "update_disabled",
            ApplyError::Busy => "update_in_progress",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            ApplyError::Disabled => {
                "update is not enabled on this deployment: set SIMPLE_POST_UPDATE_COMMAND and run with a bearer password"
            }
            ApplyError::Busy => "another update is already running",
        }
    }
}

/// 走 core 的进程内缓存查一次。`force` 由 `?force=1` 控制。
pub async fn check(force: bool) -> UpdateCheck {
    simple_post_core::update::check(force).await
}

/// 运维声明的更新命令。空白串视同未声明——空命令会以 `sh -c ""` 成功返回，看起来像
/// 「更新成功」。
fn update_command() -> Option<String> {
    std::env::var(UPDATE_COMMAND_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 部署形态。`/.dockerenv` 是 Docker / Podman 容器的通用标记；没有它但运维显式声明了
/// `SIMPLE_POST_DEPLOYMENT=docker` 的，也按容器算。
fn is_docker() -> bool {
    if Path::new("/.dockerenv").exists() {
        return true;
    }
    std::env::var(DEPLOYMENT_ENV)
        .map(|v| v.trim().eq_ignore_ascii_case("docker"))
        .unwrap_or(false)
}

/// `GET /api/update/deployment` 的内容。`password_mode` 由调用方从 `WebState.password`
/// 传进来——口令模式只有那一处判定（`serve-web` 启动时从 `SIMPLE_POST_WEB_PASSWORD` /
/// `--password` 解析），这里不重读环境变量。
pub fn deployment(password_mode: bool) -> UpdateDeployment {
    let docker = is_docker();
    UpdateDeployment {
        shape: if docker { "docker" } else { "binary" },
        image: if docker { Some(DOCKER_IMAGE) } else { None },
        command: if docker { Some(DOCKER_COMMAND) } else { None },
        apply_enabled: password_mode && update_command().is_some(),
    }
}

/// 同一时刻只准有一个更新在跑。`try_lock` 而不是 `lock`：第二个请求要立刻拿到 409，
/// 而不是排队等两分钟。
static APPLY_GATE: Mutex<()> = Mutex::const_new(());

/// 执行运维声明的那条命令并原样返回它的输出。
///
/// `password_mode` 的判据与 [`deployment`] 的 `applyEnabled` 相同——两处必须一起成立，
/// 否则界面显示「可更新」而按钮回 403（或反过来）。
pub async fn apply(password_mode: bool) -> Result<UpdateApplyResult, ApplyError> {
    let Some(command) = update_command() else {
        return Err(ApplyError::Disabled);
    };
    if !password_mode {
        return Err(ApplyError::Disabled);
    }
    let Ok(_guard) = APPLY_GATE.try_lock() else {
        return Err(ApplyError::Busy);
    };
    Ok(run(&command).await)
}

async fn run(command: &str) -> UpdateApplyResult {
    let mut child = shell_command(command);
    // 超时那一刻 `output()` 的 future 被丢弃；`kill_on_drop` 保证子进程不会留在后台，
    // 否则下一次 `apply` 会与上一轮的命令并跑。
    child.kill_on_drop(true);

    match tokio::time::timeout(APPLY_TIMEOUT, child.output()).await {
        Ok(Ok(output)) => UpdateApplyResult {
            command: command.to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: clamp(&output.stdout),
            stderr: clamp(&output.stderr),
        },
        Ok(Err(error)) => UpdateApplyResult {
            command: command.to_string(),
            exit_code: -1,
            stdout: String::new(),
            stderr: format!("无法启动更新命令: {error}"),
        },
        Err(_) => UpdateApplyResult {
            command: command.to_string(),
            exit_code: -1,
            stdout: String::new(),
            stderr: format!(
                "更新命令在 {} 秒内没有结束，已终止",
                APPLY_TIMEOUT.as_secs()
            ),
        },
    }
}

fn shell_command(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

/// 输出截断。按字符边界切，避免把一个多字节字符劈成两半后落进 JSON。
fn clamp(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= OUTPUT_LIMIT_BYTES {
        return text.into_owned();
    }
    let mut cut = OUTPUT_LIMIT_BYTES;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut].to_string()
}

#[cfg(test)]
#[path = "update_api_test.rs"]
mod tests;
