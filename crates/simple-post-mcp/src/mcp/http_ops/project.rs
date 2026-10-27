//! 临时请求要用的项目行 —— 查不到就用空壳。

use simple_post_core::db::DbPool;
use simple_post_core::domain::Project;

/// 临时请求要用的项目行。
///
/// base URL 的取值住在 (环境 × 服务 × 模块) 交叉的格子里，而服务属于项目，故要知道
/// 项目才能读那张表。项目 id 的来源（服务 id / 环境 id）在调用处定好了，这里只负责
/// 取那一行；取不到就用空壳（解析出空 base URL，不猜）。
pub(super) fn project_for(pool: &DbPool, project_id: Option<&str>) -> Project {
    let Some(project_id) = project_id.filter(|id| !id.is_empty()) else {
        return EMPTY_PROJECT.clone();
    };
    simple_post_core::db::with_conn(pool, |c| simple_post_core::db::projects::get(c, project_id))
        .unwrap_or_else(|_| EMPTY_PROJECT.clone())
}

/// `resolve_endpoint` 要一个 `&Project`，而项目不一定查得到；查不到就用这个空壳，
/// 免得解析这一步去 borrow 一个 `None`。
// 必须是 `static` 而非 `const`：`&EMPTY_PROJECT` 要拿到的是 `&'static Project`，
// 引用 `const` 会产生语句结束即释放的临时值，存活的 `&Project` 会触发 E0716。
pub(crate) static EMPTY_PROJECT: Project = Project {
    id: String::new(),
    name: String::new(),
    description: None,
    base_url: None,
    base_url_definitions: Vec::new(),
    default_base_url_definition_id: None,
    color: String::new(),
    created_at: 0,
    updated_at: 0,
};
