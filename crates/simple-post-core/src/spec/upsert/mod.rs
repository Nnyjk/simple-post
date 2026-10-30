//! 一份接口规格的落库编排。
//!
//! ## 这一层解决什么
//!
//! 「创建一个接口规范」这件事牵涉四类实体：模块、集合、环境、端点，
//! 而它们的落库顺序有依赖（端点要挂在集合下，URL 要写进环境 × 模块那一格）。拆成若干个 CRUD 工具之后，agent 要自己记住那个顺序、自己把上一步
//! 的 id 传给下一步，于是每一份规格要花掉七八轮往返。
//!
//! 本模块把「一份规格」当作一个整体来落：调用方给出一份声明式的 JSON，
//! 本模块在一个事务里把它变成库里那些行，缺什么建什么，已有的按名字复用。
//!
//! ## 为什么编排住在 core
//!
//! 因为同一份规格的语义要在三个宿主里一致：`simple-post-server` 的 REST 路由、
//! `simple-post-mcp` 的工具、Tauri command。编排写在某一侧，另一侧就会静默少一半
//! —— 这正是本仓库反复强调的 dual-transport invariant。本模块只做落库，
//! 不发请求、不拼 HTTP 请求，故它不依赖 axum，也不依赖 Tauri。
//!
//! ## 幂等靠什么判定
//!
//! 容器类实体（模块、集合、环境）一律**按名字找或建**：重复应用同一份规格
//! 不会造出第二份模块。端点例外，它按「同一个父下 name + method + url 完全相同」
//! 判定，理由见 [`crate::spec::find_endpoint_by_identity`]。
//!
//! ## `if_exists` 管的是端点
//!
//! 容器类实体永远是「找或建」，没有冲突可言 —— 反复应用同一份规格正是它们的用法。
//! 而端点可能已经存在，这时 [`EndpointIfExists`] 决定怎么办：
//! `reuse` 一个字段都不写，`error` 报冲突，`duplicate` 另建一个，
//! `update` 把规格里点名的字段覆盖到那个端点上。
//!
//! 环境那一段**总是**落库，与端点的 `if_exists` 无关：它说的是「装配」，
//! 而端点的 `if_exists` 说的是「这个端点怎么办」。
//!
//! ## 模块地图
//!
//! | 文件 | 内容 |
//! |---|---|
//! | `types.rs` | 规格的线格式与落库后的结果形状 |
//! | `resolve.rs` | 模块、集合、环境怎么找到或建出来 |
//! | `write.rs` | 端点怎么写，以及整份规格怎么落成一个事务 |

mod resolve;
mod types;
mod write;

pub use types::{
    EndpointDefinition, EndpointIfExists, EndpointOutcome, EndpointSpec, EnvironmentOutcome,
    EnvironmentSpec, NamedResolved, SpecOutcome,
};
pub use write::upsert_endpoint_spec;

#[cfg(test)]
#[path = "upsert_test.rs"]
mod tests;
