//! 更新检查：版本比较、GitHub release 抓取与进程内缓存。
//!
//! 自动安装**不走这里**：桌面用 tauri updater 插件（`src-tauri`）按签名清单装安装包。
//! 但「这一版改了什么」只有 [`check`] 拿得到（线上 release body），故桌面壳的
//! `update_check` 命令也调它；`serve-web` 的三条 `/api/update/*` 路由同样调这里。
//! 缓存是进程级的——桌面进程与容器进程各一份。

pub mod check;
pub mod version;

pub use check::{
    check, current_version, global, parse_release, user_agent, GithubFetcher, Release,
    ReleaseFetcher, UpdateCheck, UpdateError, CACHE_TTL, FETCH_TIMEOUT, FORCE_MIN_INTERVAL,
    NOTES_LIMIT_BYTES, RELEASES_URL,
};
pub use version::{compare, is_newer, parse, BadVersion, Version};
