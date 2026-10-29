//! 规格里的模块、集合与环境怎么找到或建出来。

use crate::db::{self, Connection};
use crate::domain::ResourceNodeType;
use crate::error::AppError;
use crate::spec::{
    resolve_collection_path, resolve_environment, resolve_module, split_path, IfExists, Resolved,
};

use super::types::{EndpointSpec, EnvironmentOutcome, NamedResolved};

/// 模块：给 `module_id` 就是用它（必须属于这个项目且确实是模块），
/// 给 `module` 就是按名字找或建。
pub(super) fn resolve_module_of(
    conn: &mut Connection,
    project_id: &str,
    spec: &EndpointSpec,
) -> Result<Resolved, AppError> {
    match (spec.module_id.as_deref(), spec.module.as_deref()) {
        (Some(_), Some(_)) => Err(AppError::Validation(
            "spec_upsert: `module` 与 `module_id` 二选一 —— 名字用来新建，id 用来指认已有的".into(),
        )),
        (Some(id), None) => {
            let node = db::nodes::get(conn, id)?;
            if node.root_id != project_id {
                return Err(AppError::Validation(format!(
                    "模块 '{id}' 属于项目 '{}'，不属于这份规格指名的项目 '{project_id}'",
                    node.root_id
                )));
            }
            if node.kind != ResourceNodeType::Module {
                return Err(AppError::Validation(format!(
                    "'{id}' 是一个集合或端点，不是模块"
                )));
            }
            Ok(Resolved {
                id: node.id,
                created: false,
            })
        }
        (None, Some(name)) => resolve_module(conn, project_id, name, IfExists::Reuse),
        (None, None) => Err(AppError::Validation(
            "spec_upsert 需要 `module`（模块名，不存在就建）或 `module_id`（已有模块的 id）".into(),
        )),
    }
}

/// 集合路径与父：给了 `collection_path` 就逐级找或建，末级是端点的父；
/// 给了 `collection_id` 就用它；都没给则端点直接挂在模块下。
pub(super) fn resolve_parent_of(
    conn: &mut Connection,
    project_id: &str,
    module_id: &str,
    spec: &EndpointSpec,
) -> Result<(String, Vec<NamedResolved>), AppError> {
    match (spec.collection_id.as_deref(), spec.collection_path.as_deref()) {
        (Some(_), Some(_)) => Err(AppError::Validation(
            "spec_upsert: `collection_path` 与 `collection_id` 二选一 —— 路径用来新建，id 用来指认已有的"
                .into(),
        )),
        (Some(id), None) => {
            let node = db::nodes::get(conn, id)?;
            if node.root_id != project_id {
                return Err(AppError::Validation(format!(
                    "集合 '{id}' 属于项目 '{}'，不属于这份规格指名的项目 '{project_id}'",
                    node.root_id
                )));
            }
            if node.kind != ResourceNodeType::Collection {
                return Err(AppError::Validation(format!(
                    "'{id}' 是一个模块或端点，不是集合"
                )));
            }
            // 集合必须挂在这个模块下：base URL 住在 (环境 × 模块) 那一格，
            // 端点挂在别的模块下时那一格就写错了地方。
            let owner = db::nodes::module_id_of(conn, &node.id)?;
            if owner.as_deref() != Some(module_id) {
                return Err(AppError::Validation(format!(
                    "集合 '{id}' 挂在模块 '{}' 下，而这份规格指名的是 '{module_id}'；\
                     base URL 那一格按模块写，两者必须一致",
                    owner.as_deref().unwrap_or("（没有模块）")
                )));
            }
            Ok((node.id, Vec::new()))
        }
        (None, Some(path)) => {
            let segments = split_path(path)?;
            let resolved = resolve_collection_path(conn, module_id, &segments, IfExists::Reuse)?;
            let mut named = Vec::with_capacity(resolved.len());
            for (segment, r) in segments.iter().zip(resolved.iter()) {
                named.push(NamedResolved {
                    name: segment.clone(),
                    id: r.id.clone(),
                    created: r.created,
                });
            }
            let last = named
                .last()
                .ok_or_else(|| AppError::Validation("集合路径不能为空".into()))?;
            Ok((last.id.clone(), named))
        }
        (None, None) => Ok((module_id.to_string(), Vec::new())),
    }
}

/// 环境：找或建它、合并变量、写那一格 URL、按规格决定是否激活。
pub(super) fn resolve_environment_of(
    conn: &mut Connection,
    project_id: &str,
    module_id: &str,
    spec: &EndpointSpec,
) -> Result<Option<EnvironmentOutcome>, AppError> {
    let Some(env_spec) = spec.environment.as_ref() else {
        return Ok(None);
    };
    let resolved = match (env_spec.id.as_deref(), env_spec.name.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(AppError::Validation(
                "spec_upsert: `environment.name` 与 `environment.id` 二选一 —— 名字用来新建，id 用来指认已有的"
                    .into(),
            ))
        }
        (Some(id), None) => {
            let env = db::environments::get(conn, id)?;
            if env.project_id != project_id {
                return Err(AppError::Validation(format!(
                    "环境 '{id}' 属于项目 '{}'，不属于这份规格指名的项目 '{project_id}'",
                    env.project_id
                )));
            }
            Resolved {
                id: env.id,
                created: false,
            }
        }
        (None, Some(name)) => resolve_environment(conn, project_id, name, IfExists::Reuse)?,
        (None, None) => {
            return Err(AppError::Validation(
                "spec_upsert: `environment` 需要 `name`（环境名，不存在就建）或 `id`".into(),
            ))
        }
    };

    let mut env = db::environments::get(conn, &resolved.id)?;

    // 变量：逐个合并进环境已有的变量表，不重写整张表 —— 重写会抹掉规格没提到的变量。
    if let Some(vars) = env_spec.variables.as_ref() {
        for (key, value) in vars {
            env.variables.insert(key.clone(), value.clone());
        }
    }
    // 停用的变量：合并进已有的停用集合。被停用的变量照样存、照样显示，
    // 只是一律不参与替换。
    if let Some(disabled) = env_spec.disabled_variables.as_ref() {
        for key in disabled {
            if !env.variables.contains_key(key) {
                return Err(AppError::Validation(format!(
                    "环境 '{}' 里没有变量 '{key}'，停用一个不存在的变量没有意义；\
                     先在 `variables` 里给它一个值，再把它列进 `disabled_variables`",
                    env.name
                )));
            }
            if !env.disabled_variables.iter().any(|d| d == key) {
                env.disabled_variables.push(key.clone());
            }
        }
    }
    let mut patch = crate::domain::EnvironmentPatch::default();
    if env_spec.variables.is_some() {
        patch.variables = Some(env.variables.clone());
    }
    if env_spec.disabled_variables.is_some() {
        env.disabled_variables.sort();
        patch.disabled_variables = Some(env.disabled_variables.clone());
    }
    if !patch.is_empty() {
        db::environments::update(conn, &resolved.id, patch)?;
    }

    // 那一格地址：它住在 (环境 × 模块) 上，故模块由调用方给出。
    // `base_url` 是后端 api 地址，`base_url_proxy` 是实际访问地址；两者各自写自己的字段，
    // 规格没点名的那个不动。
    if env_spec.base_url.is_some() || env_spec.base_url_proxy.is_some() {
        db::environments::set_module_base_url(
            conn,
            &resolved.id,
            module_id,
            env_spec.base_url.as_deref(),
            env_spec.base_url_proxy.as_deref(),
        )?;
    }

    // 激活：只在规格明说时改。
    let activate = env_spec.activate.unwrap_or(false);
    if activate {
        db::environments::set_active(conn, &resolved.id)?;
    }

    Ok(Some(EnvironmentOutcome {
        id: resolved.id,
        name: env.name,
        created: resolved.created,
        activated: activate,
    }))
}
