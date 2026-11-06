// ===========================================================================
// region: spec
// ===========================================================================
//
// 一份接口规格的落库编排住在 `simple_post_mcp::spec::ops::upsert`，server crate 的
// `spec_api` 是转发到它的那层门面。桌面端没有这一侧的调用点，故本模块不导出 Tauri 命令。
//
// 两个入口各有一层薄壳，但落在同一个实现上：REST 侧的 `web_api/routes/spec.rs` 调
// `crate::spec_api::upsert`，MCP 工具 `spec_upsert` 直接调 `crate::spec::ops::upsert`。
// 两者的线契约由 `spec_test.rs` 守着。

#[cfg(test)]
#[path = "spec_test.rs"]
mod tests;
