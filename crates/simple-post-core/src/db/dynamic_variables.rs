//! `dynamic_variables` 表的 CRUD —— 一条动态变量的定义。
//!
//! 定义是**全局**的：任何项目的端点都能引用它，整个工作区里变量名唯一。数据源二选一：
//! 引用树里的一个端点，或一份内联请求。
//!
//! 变量名的判据是 `^[A-Za-z0-9_]+$`——与 `{{var}}` 替换原语的 `\w+` 一致
//! （见 `crate::template::is_word`），否则声明的名字在替换时永远不生效。这个判据住在
//! 写路径上：两条 transport 都经过这里，故不会出现「一边能存一边不能存」。
//!
//! 数据源的当前值不在本表，在 `super::dynamic_variable_cache`。
//!
//! ## 模块地图
//!
//! - `read.rs`：列表、单行读取、按名字读取、数据源端点所属的项目
//! - `write.rs`：新建、改、删
//! - `references.rs`：哪些端点引用了哪些变量名
//! - `validation.rs`：变量名与数据源绑定的校验、唯一约束冲突的判别
//! - `mapping.rs`：行到结构体的映射

mod mapping;
mod read;
mod references;
mod validation;
mod write;

// 公开接口：定义在按职责分开的子模块里，这里重新导出，故 `dynamic_variables::create` 等旧路径不变。
pub use read::{get, get_by_name, list, source_project_id_of};
pub use references::{referencing_endpoints, Reference};
pub use validation::is_valid_name;
pub use write::{create, delete, update};

// 测试。

#[cfg(test)]
#[path = "dynamic_variables/dynamic_variables_test.rs"]
mod tests;
