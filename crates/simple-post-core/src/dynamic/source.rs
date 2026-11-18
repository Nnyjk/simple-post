//! 一条定义取数时用的上下文：环境与模块。
//!
//! 模块仍由定义决定（数据源端点坐在哪个模块，就取哪个模块的那一格 base URL）；
//! 环境则有两种形态：**继承**调用方当前选中的环境，或**指定**某一个。

use crate::db;
use crate::domain::{DynamicVariable, Environment, Project};
use crate::error::AppError;

/// 一条定义取数时用的上下文。
///
/// 环境按三档定：定义点名的那个 → 调用方当前选中的那个 → 数据源端点所属项目的默认环境。
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
/// - 数据源是内联请求：没有项目、没有环境——内联请求自带完整地址。
/// - 环境：`def.source_environment_id` 有名就是**指定**，就是那个环境；为空就是**继承**
///   ——用 `caller_environment_id`（调用方此刻选中的环境），但只在它属于数据源端点所属
///   项目时；调用方没有环境、或那个环境不属于这个项目时，回落到该项目的**默认环境**
///   （`db::environments::get_default`，最早创建的那个）；再不行为没有环境。
/// - 模块：数据源端点所在的那个模块。base URL 挂在模块上，故端点的模块决定取哪一格。
///
/// 继承的那一档让「同一个 `{{name}}` 跟着用户切环境换值」成立，而缓存键取的是这里算出的
/// `environment_id`，故切环境一定命中另一个键——不会读到上一个环境的值。
pub fn source_context(
    pool: &db::DbPool,
    def: &DynamicVariable,
    caller_environment_id: Option<&str>,
) -> Result<SourceContext, AppError> {
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
    let module_id = db::with_conn(pool, |c| db::nodes::module_id_of(c, endpoint_id))?;
    let project = db::with_conn(pool, |c| db::projects::get(c, &project_id))?;

    let env = match def.source_environment_id.as_deref() {
        Some(env_id) => environment_by_id(pool, env_id)?,
        None => inherited_environment(pool, &project_id, caller_environment_id)?,
    };

    Ok(SourceContext {
        environment_id: env.as_ref().map(|e| e.id.clone()),
        project_id: Some(project_id),
        env,
        project,
        module_id,
    })
}

/// 按 id 取环境。环境行被删掉了就是没有环境——外键那条路本来也会把定义里的 id 置空，
/// 故这不是异常。
fn environment_by_id(
    pool: &db::DbPool,
    environment_id: &str,
) -> Result<Option<Environment>, AppError> {
    match db::with_conn(pool, |c| db::environments::get(c, environment_id)) {
        Ok(env) => Ok(Some(env)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 继承模式下实际生效的环境。
///
/// 调用方选中的环境只在它属于数据源端点所属项目时才算「调用方给了环境」：环境决定 base URL
/// 与变量，而取数打的是那个项目的接口，拿另一个项目的环境去凑会取到错的地址或凭据。
fn inherited_environment(
    pool: &db::DbPool,
    project_id: &str,
    caller_environment_id: Option<&str>,
) -> Result<Option<Environment>, AppError> {
    if let Some(env_id) = caller_environment_id {
        if let Some(env) = environment_by_id(pool, env_id)? {
            if env.project_id == project_id {
                return Ok(Some(env));
            }
        }
    }
    db::with_conn(pool, |c| db::environments::get_default(c, project_id))
}
