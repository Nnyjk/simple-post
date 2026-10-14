// ===========================================================================
// region: base_url_defs
// ===========================================================================

use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

use super::dto::{adopt_or_mint_id, BaseUrlDefPatch, CreateBaseUrlDefOpts};

/// 项目全部服务，扁平。
///
/// 「服务应该绑定到项目，这样每一个模块都会全部包含这些服务」——模块不持有服务，
/// 一个项目一份列表，其每个模块都能解析每一行。之所以扁平：endpoint 的
/// `baseUrlDefinitionId` 也是扁平的，且它是从 `env.moduleBaseUrls[defId][moduleId]`
/// 里挑一个**值**，而不是挑服务的一个子集。
#[tauri::command]
pub fn base_url_defs_list(
    state: State<DbPool>,
    project_id: String,
) -> Result<Vec<BaseUrlDefinition>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::base_url_defs::list_by_project(c, &project_id)
    })
}

/// 往**项目**的服务列表里加一条。
///
/// 项目即作用域。此前没有 ★ 就仍是 `None`：建服务不等于选一个，用户没挑过默认的项目
/// 就该在没有 base URL 的状态下发出去，而不是带上一个他从未选过的地址。
///
/// 调用方给了 id 就用它，见 `simple_post_server::dto::adopt_or_mint_id`；本命令与
/// REST handler 共用它，两个传输才对「服务 id 归谁」达成一致。
#[tauri::command]
pub fn base_url_defs_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateBaseUrlDefOpts>,
) -> Result<BaseUrlDefinition, AppError> {
    let opts = opts.unwrap_or_default();
    let def = BaseUrlDefinition {
        id: adopt_or_mint_id(opts.id),
        name,
        description: opts.description,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::create(c, &def, &project_id)?;
        // 返回库里那一行而非入参：渲染侧要把它换进自己的列表，只有重读才与
        // 数据库此刻持有的内容一致。
        crate::db::base_url_defs::get(c, &def.id)
    })
}

#[tauri::command]
pub fn base_url_defs_update(
    state: State<DbPool>,
    id: String,
    patch: BaseUrlDefPatch,
) -> Result<BaseUrlDefinition, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::update(c, &id, patch.name.clone(), patch.description.clone())
    })
}

/// 删一个服务。项目的 ★ 若指着它则一并清空——一个项目只有一个 ★，没有选替补的规则。
#[tauri::command]
pub fn base_url_defs_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::base_url_defs::delete(c, &id))
}

/// 把项目的 ★ 指到它自己的某个服务上，或收回（`def_id = None`）。
///
/// 返回 `Project` 而非空：`defaultBaseUrlDefinitionId` 是活的，没指名服务的 endpoint
/// 按它解析，返回空会让调用方从「无」猜出新值，再重读一次确认写入。
#[tauri::command]
pub fn base_url_defs_set_default(
    state: State<DbPool>,
    project_id: String,
    def_id: Option<String>,
) -> Result<Project, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::set_default(c, &project_id, def_id.as_deref())?;
        crate::db::projects::get(c, &project_id)
    })
}
