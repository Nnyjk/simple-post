//! 按项目的权限门，以及「这次请求按哪个环境解析」。

use simple_post_core::db::DbPool;
use simple_post_core::error::AppError;

/// 查项目存下来的权限档位，与 `tool_name` 需要的档位比较；不通过则返回
/// `AppError::PermissionDenied`。调用处**必须**用 `?` 传播 —— 吞掉它就等于
/// 门形同虚设。
pub async fn check_permission(
    pool: &DbPool,
    project_id: &str,
    tool_name: &str,
) -> Result<(), AppError> {
    let required = simple_post_core::permissions::required_permission(tool_name);
    simple_post_core::db::with_conn(pool, |conn| {
        simple_post_core::permissions::require(conn, project_id, required)
    })
}

/// 这次发送用哪个环境。
///
/// 端点自己绑定的环境优先；没绑定就取项目当前激活的那个 —— 与界面那侧一致：界面用的
/// 是工作区当前环境，而顶栏的环境切换会写 `environments.is_active`，故两处读到的是
/// 同一个环境。
///
/// 端点绑定的环境必须属于端点所在的项目：环境里的变量属于那个项目，拿别的项目的
/// 环境来替换，等于把那个项目的值读进这个项目的请求。
///
/// 两者都取不到时没有环境：变量不替换，base URL 的格子也无从读。
pub(crate) fn resolve_env(
    pool: &DbPool,
    env_id: Option<&str>,
    project_id: &str,
) -> Result<Option<simple_post_core::domain::Environment>, AppError> {
    if let Some(env_id) = env_id {
        // 悬空的 env id（例如环境已被删）不至于让调用失败，只是这次请求不做变量替换。
        let found = simple_post_core::db::with_conn(pool, |c| {
            simple_post_core::db::environments::get(c, env_id)
        });
        let env = found.ok();
        // `project_id` 为空表示这次请求没有项目归属（如临时的 `api_run_request`），
        // 那时无从判断，故照旧返回那个环境。
        let belongs_to_another_project = env
            .as_ref()
            .is_some_and(|e| !project_id.is_empty() && e.project_id != project_id);
        return Ok(if belongs_to_another_project {
            None
        } else {
            env
        });
    }
    if project_id.is_empty() {
        return Ok(None);
    }
    Ok(simple_post_core::db::with_conn(pool, |c| {
        simple_post_core::db::environments::get_active(c, project_id)
    })
    .unwrap_or(None))
}
