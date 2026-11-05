// ===========================================================================
// region: spec
// ===========================================================================
//
// 一份接口规格的落库编排住在 `crate::spec_api`。桌面端没有这一侧的调用点，故本模块
// 不导出 Tauri 命令；REST 侧的 `web_api/routes/spec.rs` 与 MCP 工具 `spec_upsert`
// 都调 `crate::spec_api::upsert`，两者的线契约由 `spec_test.rs` 守着。

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
