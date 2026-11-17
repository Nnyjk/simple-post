// ===========================================================================
// region: environments
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::CreateEnvironmentOpts;

#[tauri::command]
pub fn environments_list(
    state: State<DbPool>,
    project_id: Option<String>,
) -> Result<Vec<Environment>, AppError> {
    crate::db::with_conn(&state, |c| match project_id.as_deref() {
        Some(project_id) => crate::db::environments::list_by_project(c, project_id),
        None => crate::db::environments::list_all(c),
    })
}

/// 建环境。这一层只做线格式：`opts` 缺席时的缺省值。落库编排在
/// `crate::environment_api::create_environment`（REST 路由调的是同一个函数），故两条
/// transport 建出的环境必然同形 —— id 归谁铸、创建时刻由谁补，都只有那一处说了算。
#[tauri::command]
pub fn environments_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateEnvironmentOpts>,
) -> Result<Environment, AppError> {
    crate::environment_api::create_environment(
        &state,
        // 桌面命令没有 id 参数：id 由服务端铸，与 `projects_create` 同一条规矩。
        None,
        project_id,
        name,
        opts.unwrap_or_default(),
    )
}

#[tauri::command]
pub fn environments_update(
    state: State<DbPool>,
    id: String,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn environments_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::delete(c, &id))
}

#[tauri::command]
pub fn environments_set_var(
    state: State<DbPool>,
    env_id: String,
    key: String,
    value: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_var(c, &env_id, &key, value.as_deref())
    })
}

/// 停用或启用一个环境变量。
///
/// 停用的变量**照样存、照样显示，但不参与替换**——`{{x}}` 留在原样并进缺失变量报告，用户看得见
/// 「这个变量没被替换」，而不是静默被替掉。
#[tauri::command]
pub fn environments_set_var_disabled(
    state: State<DbPool>,
    env_id: String,
    key: String,
    disabled: bool,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_var_disabled(c, &env_id, &key, disabled)
    })
}

/// 一个事务里重命名环境变量。
///
/// 必须单次调用：拆成 `set_var(old, None)` + `set_var(new, value)` 会留出一个「变量不存在」的
/// 窗口，窗口内崩溃或第二步失败就毁掉这个变量。
#[tauri::command]
pub fn environments_rename_var(
    state: State<DbPool>,
    env_id: String,
    from_key: String,
    to_key: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::rename_var(c, &env_id, &from_key, &to_key)
    })
}

/// 写（环境 × 模块）那一格：`env.moduleBaseUrls[moduleId]`。
///
/// 格子里有两个字段：`url` 是模块地址，`rewrite` 是路径前缀改写规则。两个字段各自独立：
/// 起点为空串的规则 = 清空规则。格子上没有别的层，也不向别的模块的值回落，故两个字段都清空时
/// 起点为空串的规则 = 清空规则。格子上没有别的层，也不向别的模块的值回落，故两个字段都清空时
/// 这一格就整个消失。
#[tauri::command]
pub fn environments_set_module_baseurl(
    state: State<DbPool>,
    env_id: String,
    module_id: String,
    url: Option<String>,
    rewrite: Option<RewriteRule>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_module_base_url(
            c,
            &env_id,
            &module_id,
            url.as_deref(),
            rewrite.as_ref(),
        )
    })
}
