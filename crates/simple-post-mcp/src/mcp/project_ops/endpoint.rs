//! `endpoint_*` MCP 工具 —— 端点 CRUD、搜索、复制、移动，共 8 个。
//!
//! 本文件只是门面：登记与 JSON Schema（`registry.rs`）、Args（`args.rs`）、输入强制
//! 转换（`input.rs` / `patch.rs`）、读写 handler（`read.rs` / `write.rs`）与
//! node+detail 的合成读取（`resource.rs`）各归一个文件，挂在同名子目录 `endpoint/`
//! 下。`project_ops` 经本文件取 `register()` 与 `tool_endpoint_create`。

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use simple_post_core::error::AppError;

mod args;
mod input;
mod missing;
mod patch;
mod read;
mod registry;
mod resource;
mod write;

pub(super) use registry::register;
#[cfg(test)]
pub(super) use write::tool_endpoint_create;

// `spec_upsert` 也要用这两样：「它还缺什么」的判据，以及 OAuth2 载荷的 JSON Schema。
// 它们本来只服务 `endpoint_*` 那 8 个工具，现在同一个 `project_ops` 下的另一个
// 工具也要用，故在这里再导出一次。
//
// `missing_pieces` 另有一处调用方：`crate::spec::ops` —— REST 路由与桌面 command
// 落同一份规格时也要算「还缺什么」，而那条判据住在 mcp crate 里（它要用
// `variable_fill::resolve_base_url_with`），故它得能走出 `project_ops`。
pub(in crate::mcp::project_ops) use input::oauth2_config_schema;
pub(crate) use missing::{missing_pieces, project_of};

// =========================================================================
// 仅测试用的再导出
// =========================================================================
//
// `project_ops/mod_test.rs` 需要直接调 `parse_body_mode` 与 `build_auth`。它们是
// `input.rs` 私有的辅助函数，用 `#[cfg(test)]` 暴露，生产调用方看不到。

#[cfg(test)]
pub(super) fn __test_parse_body_mode(
    s: &str,
) -> Result<simple_post_core::domain::BodyMode, AppError> {
    input::parse_body_mode(s)
}

#[cfg(test)]
pub(super) fn __test_build_auth(
    auth_type: Option<&str>,
    auth_config: Option<Value>,
) -> Result<simple_post_core::domain::AuthConfig, AppError> {
    input::build_auth(auth_type, auth_config)
}

// =========================================================================
// 测试
// =========================================================================
//
// 兄弟 `#[path]` 模块，遵循本 crate 的惯例：Rust 按模块路径找文件，所以测试专用
// *目录* 是找不到的；内联 `mod tests` 在这里也是少数派。五块各守一类「以**成功**
// 响应收场」的缺陷：
//
// 1. **畸形 patch 被吞掉**（`patch_test.rs`）。patch 字段一旦用 `.ok()` 读，反序列化
//    失败就变成「这一列别动」——agent 拿到 `is_error: false` 且什么都没写，且无法与
//    真正生效的 patch 区分。这些测试断言相反的行为：字段畸形必须是点名该字段与期望
//    形状的 `Validation` 错误。
//
// 2. **搭建接口够不到或写不进**（`create_test.rs`）。patch 里写死成 `None` 的字段任何
//    工具都写不了；这些测试把整张配置面走一遍，逐个断言字段真的落库。
//
// 3. **过门用了不属于任何项目的 id**（`gate_test.rs`）。以 `collection_id` /
//    `module_id` 给出的范围若没被解析成它的归属，真实项目的权限就一次也没被查过。
//
// 共享的 fixture 在 `test_support.rs`；它不以 `_test.rs` 结尾，所以会被
// `mcp/mod_test.rs` 的源码扫描读到，但里面既没有过门调用也没有被丢弃的检查结果。

#[cfg(test)]
#[path = "endpoint/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "endpoint/patch_test.rs"]
mod patch_tests;

#[cfg(test)]
#[path = "endpoint/create_test.rs"]
mod create_tests;

#[cfg(test)]
#[path = "endpoint/authoring_test.rs"]
mod authoring_tests;

#[cfg(test)]
#[path = "endpoint/gate_test.rs"]
mod gate_tests;

#[cfg(test)]
#[path = "endpoint/schema_test.rs"]
mod schema_tests;
