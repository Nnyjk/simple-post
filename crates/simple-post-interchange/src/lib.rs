//! 导入导出的转换层：外来格式 ↔ 本仓库的标准格式。
//!
//! **标准格式是 [`simple_post_core::domain`]，本 crate 不持有任何自己的持久化模型。**
//! 它只做标准格式两侧的适配。三条边性质各不相同，这是本 crate 存在的理由：
//!
//! - `format → ir` —— **有损**。外来格式含标准格式表达不了的构造，所以这一侧**必须**
//!   产出 [`Diagnostic`]，不能只产出数据。
//! - `ir → domain` —— **无损、全函数**。补 id / `root_id` / 时间戳，任何 IR 都能落库。
//! - `domain → ir` —— **投影**。导出要自己决定取哪个环境，因为 host 不在 endpoint 上。
//!
//! `ir` 不是 `domain` 的别名：IR 里的节点还没有 id（导入预览阶段它们根本不存在），
//! 且诊断与丢失集在 `domain` 里无处安放。完整契约见 `README.md`。

pub mod commit;
pub mod diagnostics;
pub mod fingerprint;
pub mod import;
pub mod ir;
pub mod openapi;
pub mod project;

pub use diagnostics::{Diagnostic, Severity};
pub use ir::Exchange;

// 导出产物被同一个 crate 的导入器读回。两侧各有自己的测试，但那些各自对着自己的
// 期望值自洽——互相不认得时没有一个会红。
#[cfg(test)]
#[path = "roundtrip_test.rs"]
mod roundtrip;

// 导出产物过 OpenAPI 3.0 的官方模型。自家导入器永远宽容，故那一条不算数。
#[cfg(test)]
#[path = "openapi_model_test.rs"]
mod openapi_model;
