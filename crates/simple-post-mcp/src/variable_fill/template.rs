//! `{{var}}` 替换原语 —— 实现住在 `simple_post_core::template`。
//!
//! 只做转发，好让本 crate 的 `use super::template::resolve_template` 与外部的
//! `crate::variable_fill::resolve_template` 原样可用：interchange 要读同一套 `{{var}}`
//! 语法，而依赖方向是 `interchange → core`，不能反过来。

pub use simple_post_core::template::*;
