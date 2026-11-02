//! 一条定义取数时用的上下文：环境与模块都由定义自己决定。

use crate::db;
use crate::domain::{DynamicVariable, Environment, Project};
use crate::error::AppError;

/// 一条定义取数时用的上下文。
///
/// 它只由定义自己决定：数据源端点落在哪个项目就用哪个项目的模块，定义点名的环境就是
/// 那个环境，没点名就用该项目的第一个可用环境。故同一个 `{{name}}` 在任何项目里取到的
/// 值都一样——这正是定义做成全局作用域的前提。
pub struct SourceContext {
    /// 数据源端点所属的项目。内联请求没有项目。
    pub project_id: Option<String>,
    /// 取数用的环境。`None` = 没有环境（内联请求，或该项目一个环境都没有）。
    pub environment_id: Option<String>,
    /// 取数用的环境的内容。数据源端点用它解析 base URL、鉴权与变量替换。
    pub env: Option<Environment>,
    /// 数据源端点所属的项目。
    pub project: Project,
    /// 数据源端点所在的模块。base URL 的取值住在 (环境 × 模块) 的格子里。
    pub module_id: Option<String>,
}

/// 算出这条定义的取数上下文。
///
/// 规则：
///
/// - 数据源是内联请求：没有项目、没有环境——内联请求自带完整地址。
/// - 环境：定义点名的那个；没点名就用数据源端点所属项目的**第一个可用环境**
///   （按名字排序，与选择器里看到的顺序一致）。
/// - 模块：数据源端点所在的那个模块。base URL 挂在模块上，故端点的模块决定取哪一格。
pub fn source_context(pool: &db::DbPool, def: &DynamicVariable) -> Result<SourceContext, AppError> {
    let Some(endpoint_id) = def.source_endpoint_id.as_deref() else {
        return Ok(SourceContext {
            project_id: None,
            environment_id: None,
            env: None,
            project: Project::empty(),
            module_id: None,
        });
    };

    let project_id = db::with_conn(pool, |c| db::nodes::root_id_of(c, endpoint_id))?;
    let module_id = db::with_conn(pool, |c| db::nodes::module_id_of(c, endpoint_id))
        .ok()
        .flatten();
    let project = db::with_conn(pool, |c| db::projects::get(c, &project_id))
        .unwrap_or_else(|_| Project::empty());

    // 环境：点名的那个优先。没点名就用项目里的第一个——环境按名字排序，故「第一个」
    // 与用户在选择器里看到的头一个一致。
    let env = match def.source_environment_id.as_deref() {
        Some(env_id) => db::with_conn(pool, |c| db::environments::get(c, env_id)).ok(),
        None => db::with_conn(pool, |c| db::environments::list_by_project(c, &project_id))
            .ok()
            .and_then(|list| list.into_iter().next()),
    };

    Ok(SourceContext {
        environment_id: env.as_ref().map(|e| e.id.clone()),
        project_id: Some(project_id),
        env,
        project,
        module_id,
    })
}
