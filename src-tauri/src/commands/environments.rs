// ===========================================================================
// region: environments
// ===========================================================================

use std::collections::HashMap;

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

#[tauri::command]
pub fn environments_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateEnvironmentOpts>,
) -> Result<Environment, AppError> {
    let opts = opts.unwrap_or_default();
    let env = Environment {
        id: uid(),
        project_id,
        name,
        base_urls: opts.base_urls.unwrap_or_default(),
        module_base_urls: HashMap::new(),
        variables: opts.variables.unwrap_or_default(),
        disabled_variables: Vec::new(),
        is_active: opts.make_active.unwrap_or(false),
    };
    crate::db::with_tx(&state, |c| {
        crate::db::environments::create(c, &env)?;
        Ok(env.clone())
    })
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
/// 停用的变量**照样存、照样显示，但不参与替换**——`{{x}}` 留在原样并进缺失变量报告，
/// 用户看得见「这个变量没被替换」，而不是静默被替掉。
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
/// 必须单次调用：拆成 `set_var(old, None)` + `set_var(new, value)` 会留出一个
/// 「变量不存在」的窗口，窗口内崩溃或第二步失败就毁掉这个变量。
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

/// 写唯一一层 URL 的一格：`env.moduleBaseUrls[defId][moduleId]`。
///
/// `defId` 是项目的一个服务，`moduleId` 是正在给它配地址的模块——同一服务可在两个
/// 模块指向两个主机。这一层之上没有第二层可写、也没有可回落的东西，所以在此清空一格
/// 就意味着请求不带 base URL 发出。`url = None` 才是清空；传空串会存下一格「看着是空」
/// 的值，与清空无法区分。
#[tauri::command]
pub fn environments_set_module_baseurl(
    state: State<DbPool>,
    env_id: String,
    def_id: String,
    module_id: String,
    url: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_module_base_url(
            c,
            &env_id,
            &def_id,
            &module_id,
            url.as_deref(),
        )
    })
}

#[tauri::command]
pub fn environments_set_active(
    state: State<DbPool>,
    env_id: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::set_active(c, &env_id))
}
