//! 进程表操作：问一个 PID 活着没有、按 PID 强杀。两个后端都是 spawn 一个平台自带
//! 的命令行工具，不为此引入 libc 一类的直接依赖。

/// 尽力而为的 PID 存活探测。
///
///   * Windows —— spawn `tasklist /FI "PID eq <pid>"`，在 CSV 行里找这个 PID。
///     PID 不存在时 `tasklist` 仍以 0 退出且有输出（打 "INFO: No tasks …"），故唯一
///     可靠的信号是该 PID 是否出现在 stdout 中。
///
///   * Unix —— spawn `kill -0 <pid>` 看退出码。退出 0 表示进程存在（哪怕属于别的
///     用户）。不引入新 crate：`kill` 在所有支持平台的 coreutils 里都有。选
///     `std::process` 而非 `libc::kill`，只为一个系统调用不值得多一个直接依赖。
///
/// 两个后端都是尽力而为：不持有内核句柄时，本检查与调用方据此行动之间的 TOCTOU
/// 无法避免。调用方（`write`）把「PID 说活着」当作「另一个 daemon 占着文件，拒绝」，
/// 把「PID 说死了」当作「unlink 后重试」——最坏结果是一次多余的重启，不是数据丢失。
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let needle = format!("\"{pid}\"");
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();
        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout);
                // PID 不存在时 tasklist 打的是 "INFO: No tasks are running..."，那行不含我们加引号的
                // needle，故成员判断即足够。
                s.contains(&needle)
            }
            Err(_) => false,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// 强制终止 `PortRecord` 点名的 PID。
///
/// 这是 [`is_pid_alive`] 的带外对应物，且只有一个调用方：daemon supervisor 的
/// teardown。`ensure_started` 的复用路径会*收养*已在运行的 daemon（端口记录里是活
/// PID + healthz 绿）却从未 spawn 它，故监管者对该进程没有 `Child` 句柄，无法走
/// 常规 reap。只查自己手上那个句柄的重启会静默地重新收养它本该替换掉的进程；本函数
/// 才让这一种情况真的被终止。
///
///   * Windows —— `taskkill /F /PID <pid>`。强制（没有优雅窗口：daemon 是无状态的
///     JSON-RPC 循环，没什么要 flush），且**刻意不带** `/T`，这样被回收复用的 PID
///     最多让调用方损失自己的 daemon，绝不牵连无关进程树。
///   * Unix —— `kill -9 <pid>`。
///
/// 返回 OS 是否接受了该信号。`false` 是尽力而为下的噪声，不是契约违约：调用方会在
/// 有界等待里用 [`is_pid_alive`] 复核，而「进程本来就已经没了」对 teardown 而言完全
/// 是个可接受的结果。
pub fn terminate_pid(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
