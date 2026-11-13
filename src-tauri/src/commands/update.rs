// ===========================================================================
// region: update
// ===========================================================================

//! 更新检查的 Tauri 入口。
//!
//! 三条 `/api/update/*` 路由只在 `serve-web` 有实现；桌面壳只实现 `update_check` 这一条。
//! `update_deployment` 与 `update_apply` 回答的是「这台机器怎么更新」（读环境变量里那条
//! 运维命令），桌面由 updater 插件装安装包，这两条在桌面没有语义，故这里不提供。
//!
//! 判断与抓取全在 [`crate::update_api::check`]，REST 侧调的是同一个函数，故线上实时
//! 发布说明在两条传输下是同一份 —— 与 `latest.json` 里那份构建期写死的 `notes` 无关。
//! 缓存按进程算：桌面进程与 `serve-web` 进程各一份，互不影响。
//!
//! **没有口令判据**：`serve-web` 的 bearer 闸在路由层，桌面壳是本机 IPC 调用，没有 bearer。

use crate::error::AppError;
use crate::update_api::UpdateCheck;

/// 查一次线上的发布说明。
///
/// `force` 供界面的「检查更新」按钮绕过 6 小时缓存；core 另有 60 秒节流，强制检查不会
/// 变成打 GitHub 的免费通道。
///
/// 返回 [`UpdateCheck`] 而不是 `Option`：抓取失败也走 `Ok`，降级信息在响应体的 `error`
/// 字段里 —— 与 `GET /api/update/check` 的 200 约定一致，界面在两条传输下读同一套判别。
#[tauri::command]
pub async fn update_check(force: Option<bool>) -> Result<UpdateCheck, AppError> {
    Ok(crate::update_api::check(force.unwrap_or(false)).await)
}
