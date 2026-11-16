//! Windows 上不给子进程分配控制台窗口。
//!
//! 从 GUI 进程 spawn 一个控制台程序（`tasklist` / `taskkill` / `cmd`）时，Windows 会为
//! 它新建一扇终端窗口，用户看到的就是一次黑框闪烁。`CREATE_NO_WINDOW` 是压住这件事的
//! 创建标志；非 Windows 上两个函数都是空操作，调用点因此不必写 `#[cfg]`。

/// `CREATE_NO_WINDOW`：进程不分配控制台，也就不会出现在任务栏或屏幕上。
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 给 `std::process::Command` 加上隐藏窗口标志。
pub fn hide_console_std(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// 给 `tokio::process::Command` 加上隐藏窗口标志。
pub fn hide_console_tokio(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
#[path = "win_spawn_test.rs"]
mod tests;
