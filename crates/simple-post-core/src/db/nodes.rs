//! `nodes` 表的 CRUD——合并后的资源树。
//!
//! 每个节点一行，由 `type` 区分、以 `parent_id` 相连；`root_id` 反范式化出所属项目，
//! 故整棵树是一次走索引的读，「这属于哪个项目」也成了一次主键命中，而不必沿父链上溯。
//!
//! ## 合并丢了什么，由什么接管
//!
//! 三表布局靠外键（如 `endpoints.collection_id REFERENCES collections(id)`）免费得到
//! 「父永远接受它的列能指名的那些类型」。单一多态 `parent_id` 做不到：SQLite 的 CHECK
//! 约束读不到别的行。
//!
//! 故父子矩阵住在 [`ResourceNodeType::may_under`](crate::domain::ResourceNodeType::may_under)，
//! 并在此处每次写入时执行，`schema.sql`
//! 另有配套触发器作为第二道防线。触发器那半不是冗余的双保险：MCP 工具是本项目主要的外部
//! 写路径，跳过 `db::nodes` 的将来调用者否则可以自由搭出一棵没有读者走得动的树。
//!
//! ## `root_id` 是推导值，推导值需要维护
//!
//! 它换来的是 `list_by_root` 一次走索引的扫描、`root_id_of` 一次主键查找——逐项目权限门的
//! 第一步。代价是跨项目边界的移动必须为整棵子树重新推导，而不只是调用方指名的那几行。
//! [`move_many`] 做了这件事，`subtree_module_id_cascade_has_a_test` 是回归网。

mod placement;
mod read;
mod tree;
mod write;

// 公开接口：定义在按职责分开的子模块里，这里重新导出，故 `nodes::create` 等旧路径不变。
pub use read::{
    get, list_all, list_by_root, list_by_root_and_type, list_children, module_id_of, root_id_of,
    search, search_paths, subtree_ids, type_of, SearchHit,
};
pub use tree::{move_many, move_to_root};
pub use write::{create, delete, duplicate, reparent, update};

// 测试。

#[cfg(test)]
mod nodes_fixtures;

#[cfg(test)]
#[path = "nodes_read_test.rs"]
mod read_tests;

#[cfg(test)]
#[path = "nodes_write_test.rs"]
mod write_tests;

#[cfg(test)]
#[path = "nodes_move_test.rs"]
mod move_tests;

#[cfg(test)]
#[path = "nodes_schema_test.rs"]
mod schema_tests;

#[cfg(test)]
#[path = "nodes_seeded_test.rs"]
mod seeded_tests;
