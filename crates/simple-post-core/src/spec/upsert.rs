//! 一份接口规格的落库编排。
//!
//! ## 这一层解决什么
//!
//! 「创建一个接口规范」这件事牵涉五类实体：模块、集合、环境、服务、端点，
//! 而它们的落库顺序有依赖（端点要挂在集合下，URL 要写进环境 × 服务 × 模块
//! 那一格）。拆成若干个 CRUD 工具之后，agent 要自己记住那个顺序、自己把上一步
//! 的 id 传给下一步，于是每一份规格要花掉七八轮往返。
//!
//! 本模块把「一份规格」当作一个整体来落：调用方给出一份声明式的 JSON，
//! 本模块在一个事务里把它变成库里那些行，缺什么建什么，已有的按名字复用。
//!
//! ## 为什么编排住在 core
//!
//! 因为同一份规格的语义要在三个宿主里一致：`simple-post-server` 的 REST 路由、
//! `simple-post-mcp` 的工具、Tauri command。编排写在某一侧，另一侧就会静默少一半
//! —— 这正是本仓库反复强调的 dual-transport invariant。本模块只做落库，
//! 不发请求、不拼 HTTP 请求，故它不依赖 axum，也不依赖 Tauri。
//!
//! ## 幂等靠什么判定
//!
//! 容器类实体（模块、集合、环境、服务）一律**按名字找或建**：重复应用同一份规格
//! 不会造出第二份模块。端点例外，它按「同一个父下 name + method + url 完全相同」
//! 判定，理由见 [`super::find_endpoint_by_identity`]。
//!
//! ## `if_exists` 管的是端点
//!
//! 容器类实体永远是「找或建」，没有冲突可言 —— 反复应用同一份规格正是它们的用法。
//! 而端点可能已经存在，这时 [`EndpointIfExists`] 决定怎么办：
//! `reuse` 一个字段都不写，`error` 报冲突，`duplicate` 另建一个，
//! `update` 把规格里点名的字段覆盖到那个端点上。
//!
//! 环境与服务那两段**总是**落库，与端点的 `if_exists` 无关：它们说的是「装配」，
//! 而端点的 `if_exists` 说的是「这个端点怎么办」。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::db::{self, Connection};
use crate::domain::{
    AuthConfig, EndpointDetail, EndpointDetailPatch, EndpointResource, RequestBody, ResourceNode,
    ResourceNodePatch, ResourceNodeType,
};
use crate::error::AppError;

use super::input::{
    check_auth_payload, coerce_key_values, reject_schema_shaped_payload, KeyValueInput,
};
use super::{
    find_endpoint_by_identity, resolve_base_url_def, resolve_collection_path, resolve_environment,
    resolve_module, split_path, IfExists, Resolved,
};

// =========================================================================
// 规格的线格式
// =========================================================================

/// 一份接口规格。
///
/// 字段一律用 `Option` 表达「这份规格没有提到它」，因为「没提到」与「提到但为空」
/// 在落库时的处置不同：前者不动已有的东西，后者要覆盖。
///
/// ## 两种拼写都收
///
/// 字段名是 snake_case（MCP 那条线的惯例），但**camelCase 的别名同样收**：
/// REST 路由与桌面 command 所在的这条线通篇用 camelCase，让调用方为了一份规格
/// 改掉自己的书写习惯没有必要。两种拼写落在**同一个**类型上，故三端不会各收各的形状。
///
/// 别名只挂在多词的字段上；单词字段两种拼写本就相同。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointSpec {
    /// 规格落在哪个项目里。项目必须**已经存在** —— 本模块不建项目，也不改权限档位。
    #[serde(alias = "projectId")]
    pub project_id: String,
    /// 模块名。不存在就建出来。
    #[serde(default)]
    pub module: Option<String>,
    /// 已有的模块 id。与 `module` 二选一。
    #[serde(default, alias = "moduleId")]
    pub module_id: Option<String>,
    /// 集合路径，相对模块，段之间一个斜杠，如 `订单/明细`。路径上缺的集合会被逐级建出来。
    #[serde(default, alias = "collectionPath")]
    pub collection_path: Option<String>,
    /// 已有的集合 id。与 `collection_path` 二选一。
    #[serde(default, alias = "collectionId")]
    pub collection_id: Option<String>,
    /// 环境这一段：环境本身、它的变量、它对某个服务在这个模块下的 URL。
    #[serde(default)]
    pub environment: Option<EnvironmentSpec>,
    /// 服务这一段：服务本身，以及它是不是项目的 ★。
    #[serde(default)]
    pub service: Option<ServiceSpec>,
    /// 端点本身。
    pub endpoint: EndpointDefinition,
    /// 同一个父下已经有同名同 method 同 url 的端点时怎么办。缺省是 `reuse`。
    #[serde(default, alias = "ifExists")]
    pub if_exists: Option<String>,
}

/// 规格里的环境那一段。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSpec {
    /// 已有的环境 id。与 `name` 二选一。
    #[serde(default)]
    pub id: Option<String>,
    /// 环境名。不存在就建出来。与 `id` 二选一。
    #[serde(default)]
    pub name: Option<String>,
    /// 要写入的变量。逐个合并进环境已有的变量表 —— 规格是往环境里**加**东西，
    /// 不是把环境重写成规格里那几个变量。
    #[serde(default)]
    pub variables: Option<BTreeMap<String, String>>,
    /// 要停用的变量名。合并进已有的停用集合。
    #[serde(default, alias = "disabledVariables")]
    pub disabled_variables: Option<Vec<String>>,
    /// 这个环境 × 服务 × 模块 那一格的 URL。没有服务就没有那一格，故它与服务同生共死。
    #[serde(default, alias = "baseUrl")]
    pub base_url: Option<String>,
    /// 落库后把这个环境设为项目的激活环境。
    #[serde(default)]
    pub activate: Option<bool>,
}

/// 规格里的服务那一段。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSpec {
    /// 已有的服务 id。与 `name` 二选一。
    #[serde(default)]
    pub id: Option<String>,
    /// 服务名。不存在就建出来。与 `id` 二选一。
    #[serde(default)]
    pub name: Option<String>,
    /// 服务的说明。只在新建时写进去。
    #[serde(default)]
    pub description: Option<String>,
    /// 落库后把这个服务设为项目的 ★（默认服务）。
    #[serde(default)]
    pub default: Option<bool>,
}

/// 规格里的端点那一段。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDefinition {
    /// 接口名。
    pub name: String,
    /// HTTP 动词。
    pub method: String,
    /// 只有路径。host 在发送时从环境的服务 URL 解析。
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub params: Option<Vec<KeyValueInput>>,
    /// 路径参数（`/api/orders/{id}` 的 `id`）与它们各自的值。
    ///
    /// 与 `params` 分开存，因为两者进的地方不同：`params` 一律进查询串，路径参数要被
    /// 替换进路径本身。
    #[serde(default, alias = "pathParams")]
    pub path_params: Option<Vec<KeyValueInput>>,
    #[serde(default)]
    pub headers: Option<Vec<KeyValueInput>>,
    /// 请求体。`content` 是这一次发出去的载荷，`schema` 是文档里那份参数定义。
    #[serde(default)]
    pub body: Option<RequestBody>,
    /// 认证方案与凭据。`kind` 是方案，缺省 `none`。
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    /// 端点绑定哪个环境。规格里给了 `environment` 段而这里没给时，绑定那个环境。
    #[serde(default, alias = "environmentId")]
    pub environment_id: Option<String>,
    /// 端点据以解析的服务。规格里给了 `service` 段而这里没给时，绑定那个服务。
    #[serde(default, alias = "baseUrlDefinitionId")]
    pub base_url_definition_id: Option<String>,
    /// 这个端点自己的 base URL，胜过环境里那一格。
    #[serde(default, alias = "baseUrlOverride")]
    pub base_url_override: Option<String>,
    /// 前置脚本。
    #[serde(default, alias = "preRequestScript")]
    pub pre_request_script: Option<String>,
    /// 测试脚本。
    #[serde(default, alias = "testScript")]
    pub test_script: Option<String>,
}

// =========================================================================
// 端点的冲突策略
// =========================================================================

/// 同一个父下已经有同名同 method 同 url 的端点时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointIfExists {
    /// 复用已有的那一个，**一个字段都不写**。缺省值。
    Reuse,
    /// 报冲突，不新建。
    Error,
    /// 另建一个，与已有那个并存。
    Duplicate,
    /// 把规格里点名的字段覆盖到那个端点上；规格没点名的字段保持原样。
    Update,
}

impl EndpointIfExists {
    /// 从调用方给的名字解析。空串与缺省都当 [`EndpointIfExists::Reuse`]。
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s.trim() {
            "" | "reuse" => Ok(EndpointIfExists::Reuse),
            "error" => Ok(EndpointIfExists::Error),
            "duplicate" => Ok(EndpointIfExists::Duplicate),
            "update" => Ok(EndpointIfExists::Update),
            other => Err(AppError::Validation(format!(
                "未知的 if_exists 值 '{other}'；可取 reuse / error / duplicate / update"
            ))),
        }
    }

    /// 该策略的字符串形式，与 [`EndpointIfExists::parse`] 的取值一一对应。
    pub fn as_str(self) -> &'static str {
        match self {
            EndpointIfExists::Reuse => "reuse",
            EndpointIfExists::Error => "error",
            EndpointIfExists::Duplicate => "duplicate",
            EndpointIfExists::Update => "update",
        }
    }
}

// =========================================================================
// 结果
// =========================================================================

/// 一次落库的结果。
///
/// 三个宿主（REST 路由、桌面 command、MCP 工具）回的是同一份形状：结果类型本身
/// 可序列化，故不必各写一遍 JSON 拼装，也就不会各写错一遍。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecOutcome {
    pub project_id: String,
    /// 端点所在的那个模块。
    pub module: NamedResolved,
    /// 端点的父。模块本身也是一个合法的父。
    pub parent_id: String,
    /// 集合路径上逐级的结果，顺序与路径一致。
    pub collections: Vec<NamedResolved>,
    /// 环境那一段的结果。规格没写环境时为 `None`。
    pub environment: Option<EnvironmentOutcome>,
    /// 服务那一段的结果。规格没写服务时为 `None`。
    pub service: Option<ServiceOutcome>,
    /// 端点的结果。
    pub endpoint: EndpointOutcome,
}

/// 一个按名字找到或建出来的实体。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedResolved {
    pub name: String,
    pub id: String,
    /// 本次调用**新建**的为 `true`，复用了已有项的为 `false`。
    pub created: bool,
}

/// 环境那一段的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentOutcome {
    pub id: String,
    pub name: String,
    pub created: bool,
    /// 这次把它设成了项目的激活环境。
    pub activated: bool,
}

/// 服务那一段的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceOutcome {
    pub id: String,
    pub name: String,
    pub created: bool,
    /// 这次把它设成了项目的 ★。
    pub is_default: bool,
}

/// 端点的结果。
///
/// 端点本身**摊平**在这层里，故 `endpoint.id` 就是端点的 id，`endpoint.detail`
/// 是它的请求配置。多套一层 `resource` 会让调用方多挖两级，而那两级里没有信息。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOutcome {
    /// 本次调用**新建**了这个端点。
    pub created: bool,
    /// 本次调用覆盖了已有端点的字段。
    pub updated: bool,
    /// 端点本身：节点结构 + 请求配置。
    #[serde(flatten)]
    pub resource: EndpointResource,
}

// =========================================================================
// 落库
// =========================================================================

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
    // 项目必须存在。不存在就报错，而不是悄悄建一个 —— 建项目要改权限档位，
    // 那是一次显式的决定，不该由一份接口规格顺带做掉。
    let project = db::projects::get(conn, project_id)?;

    // 端点自绑的环境与服务必须属于这份规格指名的项目：环境里的变量与
    // (环境 × 服务 × 模块) 那一格 URL 都属于那个项目，绑一个别的项目的等于让这个
    // 项目去读另一个项目的值。校验放在最前面，不通过就整份规格不落库。
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

/// 模块：给 `module_id` 就是用它（必须属于这个项目且确实是模块），
/// 给 `module` 就是按名字找或建。
fn resolve_module_of(
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
fn resolve_parent_of(
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
            // 集合必须挂在这个模块下。base URL 住在 (环境 × 服务 × 模块) 那一格，
            // 端点挂在别的模块下时，那一格就写错了地方。
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

/// 服务：给 `id` 就是用它（必须属于这个项目），给 `name` 就是按名字找或建。
fn resolve_service_of(
    conn: &mut Connection,
    project_id: &str,
    spec: &EndpointSpec,
) -> Result<Option<ServiceOutcome>, AppError> {
    let Some(service) = spec.service.as_ref() else {
        return Ok(None);
    };
    let resolved = match (service.id.as_deref(), service.name.as_deref()) {
        (Some(_), Some(_)) => return Err(AppError::Validation(
            "spec_upsert: `service.name` 与 `service.id` 二选一 —— 名字用来新建，id 用来指认已有的"
                .into(),
        )),
        (Some(id), None) => {
            let owner = db::base_url_defs::project_id_of(conn, id)?;
            if owner != project_id {
                return Err(AppError::Validation(format!(
                    "服务 '{id}' 属于项目 '{owner}'，不属于这份规格指名的项目 '{project_id}'"
                )));
            }
            let def = db::base_url_defs::get(conn, id)?;
            Resolved {
                id: def.id,
                created: false,
            }
        }
        (None, Some(name)) => resolve_base_url_def(
            conn,
            project_id,
            name,
            service.description.as_deref(),
            IfExists::Reuse,
        )?,
        (None, None) => {
            return Err(AppError::Validation(
                "spec_upsert: `service` 需要 `name`（服务名，不存在就建）或 `id`".into(),
            ))
        }
    };
    let name = db::base_url_defs::get(conn, &resolved.id)?.name;
    // ★ 只在规格明说时改。没说就不动项目当前的 ★ —— 改它会改掉项目里**所有**
    // 没指名服务的端点的解析结果，那是一次影响面很大的改动。
    let is_default = service.default.unwrap_or(false);
    if is_default {
        db::base_url_defs::set_default(conn, project_id, Some(&resolved.id))?;
    }
    Ok(Some(ServiceOutcome {
        id: resolved.id,
        name,
        created: resolved.created,
        is_default,
    }))
}

/// 环境：找或建它、合并变量、写那一格 URL、按规格决定是否激活。
fn resolve_environment_of(
    conn: &mut Connection,
    project_id: &str,
    module_id: &str,
    service: &Option<ServiceOutcome>,
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

    // 变量：逐个合并进已有那张表。规格是往环境里**加**变量，不是把环境重写成
    // 规格里那几个 —— 后者会把环境里别的变量抹掉，而规格根本没提到它们。
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

    // 那一格 URL：它住在 (环境 × 服务 × 模块) 上，故没有服务就没有这一格。
    if let Some(url) = env_spec.base_url.as_deref() {
        let service_id = service
            .as_ref()
            .map(|s| s.id.clone())
            .or_else(|| spec.endpoint.base_url_definition_id.clone())
            .ok_or_else(|| {
                AppError::Validation(
                    "`environment.base_url` 需要一个服务：base URL 住在 \
                     (环境 × 服务 × 模块) 那一格，而那一格的第一维是服务。\
                     在 `service` 里给出服务名，或在 `endpoint.base_url_definition_id` \
                     里给出已有服务的 id"
                        .into(),
                )
            })?;
        db::environments::set_module_base_url(
            conn,
            &resolved.id,
            &service_id,
            module_id,
            Some(url),
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

    // 请求体：载荷与定义写反了就地拒绝，与 `endpoint_create` 同一道门。
    let body = match def.body.as_ref() {
        Some(b) => {
            reject_schema_shaped_payload(b)?;
            b.clone()
        }
        None => RequestBody::default(),
    };
    // 认证：凭据与方案对不上就拒。方案说 bearer 而没有 token 的配置发出去只会
    // 换来一个 401，而那要等到真的发请求才知道。
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
                // 一个字段都不写。要改字段用 `endpoint_update` 或 `if_exists=update`。
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
    // node 与 detail 一起落库：没有配置行的端点是一棵打不开的树。
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
        // 规格点名了它就写它，规格没点名就一个字段都不碰。
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
/// 绝不能用「前缀 + 毫秒时钟」：那是毫秒时钟，同一毫秒内两次创建会得到同一个
/// 主键，第二次直接撞 `nodes.id` 约束。AI 客户端成批循环创建时必然踩到。
/// [`crate::domain::uid`] 是本仓库的 id 唯一来源。
fn mint_endpoint_id() -> String {
    crate::domain::uid().replacen("sp_", "ep_", 1)
}

// =========================================================================
// 线格式的序列化（供各宿主回显）
// =========================================================================
//
// 结果类型本身带 `Serialize` 且字段名是线上形状，故 REST 路由、桌面 command 与
// MCP 工具回的是同一份 JSON，而不必各写一遍拼装 —— 那三份拼装迟早会各写错一遍。

#[cfg(test)]
#[path = "upsert_test.rs"]
mod tests;
