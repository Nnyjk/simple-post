//! 临时请求要用的项目行 —— 查不到就用空壳。

use simple_post_core::db::DbPool;
use simple_post_core::domain::Project;

/// 临时请求要用的项目行。
///
/// 动态变量的定义挂在项目里，故要知道项目才能在它里面找定义。项目 id 的来源
/// （环境 id）在调用处定好了，这里只负责取那一行；取不到就用空壳（不猜）。
pub(super) fn project_for(pool: &DbPool, project_id: Option<&str>) -> Project {
    let Some(project_id) = project_id.filter(|id| !id.is_empty()) else {
        return EMPTY_PROJECT.clone();
    };
    simple_post_core::db::with_conn(pool, |c| simple_post_core::db::projects::get(c, project_id))
        .unwrap_or_else(|_| EMPTY_PROJECT.clone())
}

/// 项目查不到时的空壳，让调用方不必处理悬垂引用。
// 必须是 `static` 而非 `const`：`&EMPTY_PROJECT` 要拿到的是 `&'static Project`，
// 引用 `const` 会产生语句结束即释放的临时值，存活的 `&Project` 会触发 E0716。
pub(crate) static EMPTY_PROJECT: Project = Project {
    id: String::new(),
    name: String::new(),
    description: None,
    color: String::new(),
    created_at: 0,
    updated_at: 0,
};
