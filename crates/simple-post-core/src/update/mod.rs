//! 更新检查：版本比较、GitHub release 抓取与进程内缓存。
//!
//! 桌面端**不走这里**：它用 tauri updater 插件（`src-tauri`）自己查签名清单。本模块
//! 只服务无头 `serve-web` 的三条 `/api/update/*` 路由，缓存也因而是进程级的——
//! 一个容器进程一份。

pub mod check;
pub mod version;

pub use check::{
    check, current_version, global, parse_release, user_agent, GithubFetcher, Release,
    ReleaseFetcher, UpdateCheck, UpdateError, CACHE_TTL, FETCH_TIMEOUT, FORCE_MIN_INTERVAL,
    NOTES_LIMIT_BYTES, RELEASES_URL,
};
pub use version::{compare, is_newer, parse, BadVersion, Version};
