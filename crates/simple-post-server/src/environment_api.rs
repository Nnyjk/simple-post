//! 建环境的编排：id 归谁铸、创建时刻由谁补、其余字段的缺省值是什么 —— 都在这一处。
//!
//! 放在 server crate 而不是 `project_api.rs` 里，是因为这一层只管环境：`project_api` 管的是
//! 项目与资源树节点（含 `root_id` 的推导），把环境的落库塞进去会让那两个不相干的问题
//! 共用一个模块名。同族的证据：`dynamic_api` / `spec_api` 也都是按聚合各开一个模块。
//!
//! 两条 transport 都只做线格式（见 `project_api_shells_test.rs`）。这不是洁癖：桌面命令行
//! `Environment { … }` 字面构造时，`core` 里新增一个字段会让编译器在桌面侧报错、而 REST 侧
//! 走 `serde` 静默取默认值 —— 同一个新字段，两条线一个立刻红、一个悄悄不一样。结构体的
//! 缺省值只留在这里拼，这条分叉就不可能发生。

use simple_post_core::db::{self, DbPool};
use simple_post_core::domain::{now_ms, Environment};
use simple_post_core::error::AppError;

use crate::dto::CreateEnvironmentOpts;

/// 新建一个环境并返回**落库之后**的那一行。
///
/// - `id`：REST 的 `ensure_server_id`、以及打平 v1 形状的客户端会点名 id，此时照用；
///   缺席或空串则由 `project_api::adopt_non_empty` 铸一个 —— 「id 归谁铸、空 id 算不算
///   给了」在这两条资源上必须是同一个答案，故只有那一处实现。
/// - `created_at`：由 `core::db::environments::create` 在落库前补 `now_ms()` —— 它在那里
///   补是为了让「最早创建的那个 = 默认环境」这条规则有一个出处（`get_default` 按
///   `created_at` 排序）。这里不必也不该自己再算一遍。
/// - `opts.variables`：桌面与 web 都表达得了的只有变量这一项；其余字段一律空着，
///   模块的 base URL 只在（环境 × 模块）那一格里写（`db::environments::set_module_base_url`）。
pub fn create_environment(
    pool: &DbPool,
    id: Option<String>,
    project_id: String,
    name: String,
    opts: CreateEnvironmentOpts,
) -> Result<Environment, AppError> {
    let env = Environment {
        id: crate::project_api::adopt_non_empty(id),
        project_id,
        name,
        module_base_urls: std::collections::HashMap::new(),
        variables: opts.variables.unwrap_or_default(),
        disabled_variables: Vec::new(),
        created_at: now_ms(),
    };
    db::with_tx(pool, |c| db::environments::create(c, &env))
}

#[cfg(test)]
#[path = "environment_api_test.rs"]
mod tests;
