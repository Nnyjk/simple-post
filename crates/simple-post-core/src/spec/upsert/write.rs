//! 端点怎么写，以及整份规格怎么落成一个事务。

use crate::db::{self, Connection};
use crate::domain::{
    AuthConfig, EndpointDetail, EndpointDetailPatch, EndpointResource, RequestBody, ResourceNode,
    ResourceNodePatch, ResourceNodeType,
};
use crate::error::AppError;
use crate::spec::find_endpoint_by_identity;
use crate::spec::input::{check_auth_payload, coerce_key_values, reject_schema_shaped_payload};

use super::resolve::{
    resolve_environment_of, resolve_module_of, resolve_parent_of, resolve_service_of,
};
use super::types::{
    EndpointDefinition, EndpointIfExists, EndpointOutcome, EndpointSpec, EnvironmentOutcome,
    NamedResolved, ServiceOutcome, SpecOutcome,
};

/// 把一份规格落进库里。
///
/// **整个过程在一个事务里**：规格写到一半失败时，库里不留半截状态 ——
/// 一个建好模块却没有端点的规格，比一次彻底失败更难收拾。
///
/// 调用方**必须先过权限门**再调本函数：规格指名了 `project_id`，而本函数会往
/// 那个项目里写东西。门在本函数之外，是因为读权限档位与写库不该在同一把锁里。
pub fn upsert_endpoint_spec(
    conn: &mut Connection,
    spec: &EndpointSpec,
) -> Result<SpecOutcome, AppError> {
    let project_id = spec.project_id.trim();
    if project_id.is_empty() {
        return Err(AppError::Validation("spec_upsert 需要 project_id".into()));
    }
    // 项目必须存在：不存在就报错，不建。建项目要改权限档位，那是一次显式的决定，
    // 不该由一份接口规格顺带做掉。
    let project = db::projects::get(conn, project_id)?;

    // 端点自绑的环境与服务必须属于规格指名的项目：环境变量与 (环境 × 服务 × 模块)
    // 那一格 URL 都属于那个项目，绑别的项目的等于让本项目读别的项目的值。
    // 校验放在最前面，不通过就整份规格不落库。
    check_endpoint_bindings(
        conn,
        &project.id,
        spec.endpoint.environment_id.as_deref(),
        spec.endpoint.base_url_definition_id.as_deref(),
    )?;

    let if_exists = EndpointIfExists::parse(spec.if_exists.as_deref().unwrap_or(""))?;

    // ---- 模块 ----
    let module = resolve_module_of(conn, &project.id, spec)?;

    // ---- 集合与父 ----
    let (parent_id, collections) = resolve_parent_of(conn, &project.id, &module.id, spec)?;

    // ---- 服务 ----
    let service = resolve_service_of(conn, &project.id, spec)?;

    // ---- 环境 ----
    let environment = resolve_environment_of(conn, &project.id, &module.id, &service, spec)?;

    // ---- 端点 ----
    let endpoint = upsert_endpoint_of(
        conn,
        &project.id,
        &parent_id,
        &environment,
        &service,
        spec,
        if_exists,
    )?;

    Ok(SpecOutcome {
        project_id: project.id,
        module: NamedResolved {
            name: module_name_of(conn, &module.id)?,
            id: module.id,
            created: module.created,
        },
        parent_id,
        collections,
        environment,
        service,
        endpoint,
    })
}

/// 端点自绑的环境与服务必须属于这份规格指名的项目。
///
/// 环境里的变量与 (环境 × 服务 × 模块) 那一格 URL 都属于那个项目，所以绑一个别的
/// 项目的环境或服务，等于让这个项目读另一个项目的值。
fn check_endpoint_bindings(
    conn: &mut Connection,
    project_id: &str,
    environment_id: Option<&str>,
    base_url_definition_id: Option<&str>,
) -> Result<(), AppError> {
    if let Some(env_id) = environment_id {
        let owner = db::environments::get(conn, env_id)?.project_id;
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "环境 '{env_id}' 属于项目 '{owner}'，不属于这份规格指名的项目 '{project_id}'"
            )));
        }
    }
    if let Some(def_id) = base_url_definition_id {
        let owner = db::base_url_defs::project_id_of(conn, def_id)?;
        if owner != project_id {
            return Err(AppError::Validation(format!(
                "服务 '{def_id}' 属于项目 '{owner}'，不属于这份规格指名的项目 '{project_id}'"
            )));
        }
    }
    Ok(())
}

/// 端点：找同一个父下同名同 method 同 url 的那个，按策略处置。
#[allow(clippy::too_many_arguments)]
fn upsert_endpoint_of(
    conn: &mut Connection,
    project_id: &str,
    parent_id: &str,
    environment: &Option<EnvironmentOutcome>,
    service: &Option<ServiceOutcome>,
    spec: &EndpointSpec,
    if_exists: EndpointIfExists,
) -> Result<EndpointOutcome, AppError> {
    let def = &spec.endpoint;
    if def.name.trim().is_empty() {
        return Err(AppError::Validation("端点名不能为空".into()));
    }
    if def.url.trim().is_empty() {
        return Err(AppError::Validation("端点路径不能为空".into()));
    }
    let method = crate::domain::parse_http_method(&def.method)?;

    // 请求体：载荷与定义写反时由 `reject_schema_shaped_payload` 拒绝，
    // 与 `endpoint_create` 同一道门。
    let body = match def.body.as_ref() {
        Some(b) => {
            reject_schema_shaped_payload(b)?;
            b.clone()
        }
        None => RequestBody::default(),
    };
    // 认证：`check_auth_payload` 拒掉凭据与方案对不上的配置 —— 方案说 bearer 而没有
    // token 的配置发出去只会换一个 401，而那要等到真的发请求才知道。
    let auth = match def.auth.as_ref() {
        Some(a) => {
            check_auth_payload(a)?;
            a.clone()
        }
        None => AuthConfig::default(),
    };

    // 端点绑定的环境与服务：规格里点名了就用它，规格里有对应的那一段就用那一段。
    let environment_id = def
        .environment_id
        .clone()
        .or_else(|| environment.as_ref().map(|e| e.id.clone()));
    let base_url_definition_id = def
        .base_url_definition_id
        .clone()
        .or_else(|| service.as_ref().map(|s| s.id.clone()));

    let existing =
        find_endpoint_by_identity(conn, parent_id, &def.name, method.as_str(), &def.url)?;

    if let Some(existing_id) = existing.as_ref() {
        match if_exists {
            EndpointIfExists::Reuse => {
                // `reuse` 一个字段都不写；要改字段用 `endpoint_update` 或 `if_exists=update`。
                return Ok(EndpointOutcome {
                    created: false,
                    updated: false,
                    resource: load_endpoint_resource(conn, existing_id)?,
                });
            }
            EndpointIfExists::Error => {
                return Err(AppError::Validation(format!(
                    "同一个父下已经有一个叫 '{}' 的 {} {} 端点；\
                     用 if_exists=update 覆盖它的字段，用 if_exists=duplicate 另建一个，\
                     或用 endpoint_update 单独改某几个字段",
                    def.name,
                    method.as_str(),
                    def.url
                )));
            }
            EndpointIfExists::Duplicate => {}
            EndpointIfExists::Update => {
                apply_endpoint_patch(
                    conn,
                    existing_id,
                    def,
                    &body,
                    &auth,
                    environment_id,
                    base_url_definition_id,
                )?;
                return Ok(EndpointOutcome {
                    created: false,
                    updated: true,
                    resource: load_endpoint_resource(conn, existing_id)?,
                });
            }
        }
    }

    let now = crate::domain::now_ms();
    let id = mint_endpoint_id();
    let node = ResourceNode {
        id: id.clone(),
        root_id: project_id.to_string(),
        parent_id: Some(parent_id.to_string()),
        kind: ResourceNodeType::Endpoint,
        name: def.name.clone(),
        description: def.description.clone(),
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let detail = EndpointDetail {
        node_id: id.clone(),
        method,
        url: def.url.clone(),
        environment_id,
        base_url_definition_id,
        base_url_override: def.base_url_override.clone(),
        params: coerce_key_values(def.params.clone().unwrap_or_default()),
        path_params: coerce_key_values(def.path_params.clone().unwrap_or_default()),
        headers: coerce_key_values(def.headers.clone().unwrap_or_default()),
        body,
        auth,
        docs: def.docs.clone(),
        notes: def.notes.clone(),
        tags: def.tags.clone().unwrap_or_default(),
        pre_request_script: def.pre_request_script.clone(),
        test_script: def.test_script.clone(),
    };
    // `db::nodes::create` 一次写入节点与请求配置：只建节点会留下一个没有配置行的端点。
    db::nodes::create(conn, &node, Some(&detail))?;
    Ok(EndpointOutcome {
        created: true,
        updated: false,
        resource: load_endpoint_resource(conn, &id)?,
    })
}

/// 读出一个端点的节点结构与请求配置。
fn load_endpoint_resource(
    conn: &mut Connection,
    node_id: &str,
) -> Result<EndpointResource, AppError> {
    let node = db::nodes::get(conn, node_id)?;
    let detail = db::endpoint_details::get(conn, node_id)?;
    Ok(EndpointResource { node, detail })
}

/// 把规格里点名的字段覆盖到一个已有端点上。规格没点名的字段保持原样。
///
/// 名字、动词与路径**不在**这份 patch 里：它们是端点的身份，而能被这一档找到的
/// 那个端点，名字、动词与路径本来就与规格相同。
fn apply_endpoint_patch(
    conn: &mut Connection,
    endpoint_id: &str,
    def: &EndpointDefinition,
    body: &RequestBody,
    auth: &AuthConfig,
    environment_id: Option<String>,
    base_url_definition_id: Option<String>,
) -> Result<(), AppError> {
    let node_patch = ResourceNodePatch {
        parent_id: None,
        name: None,
        description: def.description.clone().map(Some),
        sort_order: None,
    };
    let detail_patch = EndpointDetailPatch {
        method: None,
        url: None,
        environment_id: environment_id.clone().map(Some),
        base_url_definition_id: base_url_definition_id.clone().map(Some),
        base_url_override: def.base_url_override.clone().map(Some),
        params: def.params.clone().map(coerce_key_values),
        // 路径参数走 `double_option`：缺席 = 不动，`null` = 清空，设值 = 写入。
        // 规格点名了它就写它，没点名就一个字段都不碰。
        path_params: def
            .path_params
            .clone()
            .map(|rows| Some(coerce_key_values(rows))),
        headers: def.headers.clone().map(coerce_key_values),
        body: def.body.as_ref().map(|_| body.clone()),
        auth: def.auth.as_ref().map(|_| auth.clone()),
        docs: def.docs.clone().map(Some),
        notes: def.notes.clone().map(Some),
        tags: def.tags.clone(),
        pre_request_script: def.pre_request_script.clone().map(Some),
        test_script: def.test_script.clone().map(Some),
    };
    if !node_patch.is_empty() {
        db::nodes::update(conn, endpoint_id, node_patch)?;
    }
    if !detail_patch.is_empty() {
        db::endpoint_details::update(conn, endpoint_id, detail_patch)?;
    }
    Ok(())
}

/// 读出模块的名字，供结果回显。
fn module_name_of(conn: &mut Connection, module_id: &str) -> Result<String, AppError> {
    Ok(db::nodes::get(conn, module_id)?.name)
}

/// 铸一个端点 id。
///
/// id 不能用「前缀 + 毫秒时钟」：同一毫秒内两次创建会撞 `nodes.id` 约束，
/// AI 客户端成批循环创建时必然踩到。[`crate::domain::uid`] 是本仓库的 id 唯一来源。
fn mint_endpoint_id() -> String {
    crate::domain::uid().replacen("sp_", "ep_", 1)
}
