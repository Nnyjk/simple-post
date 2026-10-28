//! 写侧 handler：`endpoint_create` / `_update` / `_delete` / `_duplicate` / `_move`。
//!
//! 过门一律由父节点或端点自身经 `nodes::root_id_of` 解析归属项目，不接受调用方
//! 自称的项目 id。

use serde_json::{json, Value};

use super::args::{
    mint_endpoint_id, EndpointCreateArgs, EndpointDeleteArgs, EndpointDuplicateArgs,
    EndpointMoveArgs, TlsSettingsInput,
};
use super::input::{build_auth, parse_body_mode, reject_schema_shaped_payload};
use super::missing::{missing_pieces, project_of};
use super::patch::{patch_field, patch_value_field};
use super::resource::endpoint_resource;
use crate::mcp::key_value::{coerce_key_values, KeyValueInput};
use crate::mcp::{arg_str, AppState, CallToolResult};
use simple_post_core::domain::{
    EndpointDetail, EndpointDetailPatch, RequestBody, ResourceNode, ResourceNodeType, TlsSettings,
};
use simple_post_core::error::AppError;
use simple_post_core::spec::{self, IfExists};

/// 端点的父节点从哪儿来 —— **只读**地看一眼，用来定归属项目。
///
/// 三种写法给出同一个答案：
///
/// - `collection_id`：直接建在那个集合下。
/// - `collection_path` + `module_id`：路径相对模块，中途缺的集合会被逐级建出来。
/// - `module_id`：直接建在模块下（端点允许不占一个集合）。
///
/// 这一步**不建任何东西**：建出来之前要先过权限门，顺序反了就是权限门形同虚设 ——
/// 没权限的调用方照样能在别人的项目里建出集合。
fn anchor_of(a: &EndpointCreateArgs) -> Result<String, AppError> {
    // `collection_id` 与 `collection_path` 都在说「挂在哪」，两个一起给就是两种说法
    // 打架。悄悄挑一个会让调用方以为建在它说的那个父下 —— 所以直接拒。
    if a.collection_id.is_some()
        && a.collection_path
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
    {
        return Err(AppError::Validation(
            "endpoint_create: `collection_id` 与 `collection_path` 二选一 —— \
             path 自己就说明了挂在哪"
                .into(),
        ));
    }
    if let Some(cid) = a.collection_id.as_deref().filter(|s| !s.is_empty()) {
        return Ok(cid.to_string());
    }
    if let Some(path) = a.collection_path.as_deref() {
        if !path.trim().is_empty() {
            let module_id = a
                .module_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AppError::Validation(
                        "collection_path is relative to a module: pass module_id together with it"
                            .into(),
                    )
                })?;
            return Ok(module_id.to_string());
        }
    }
    a.module_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AppError::Validation(
                "endpoint_create needs a parent: pass collection_id to put the \
                 endpoint inside a collection, collection_path (with module_id) to \
                 create the collections on the way, or module_id to put it directly \
                 under a module"
                    .into(),
            )
        })
}

/// 把路径上的集合逐级建出来，返回末级那个集合的 id。
///
/// 路径上的每一级都**复用**已有的同名集合 —— 路径是「去哪儿」的定位，不是「建什么」
/// 的声明，所以中途遇到已存在的层级是常态而不是冲突。
fn materialize_parent(
    state: &AppState,
    a: &EndpointCreateArgs,
) -> Result<(String, Vec<spec::Resolved>), AppError> {
    if let Some(cid) = a.collection_id.as_deref().filter(|s| !s.is_empty()) {
        return Ok((cid.to_string(), Vec::new()));
    }
    if let Some(path) = a
        .collection_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let module_id = a
            .module_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .expect("anchor_of 已经检查过 module_id 在场");
        let segments = spec::split_path(path)?;
        let resolved = simple_post_core::db::with_tx(&state.pool, |c| {
            spec::resolve_collection_path(c, module_id, &segments, IfExists::Reuse)
        })?;
        let last = resolved
            .last()
            .ok_or_else(|| AppError::Validation("集合路径不能为空".into()))?;
        return Ok((last.id.clone(), resolved));
    }
    let module_id = a
        .module_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .expect("anchor_of 已经检查过 module_id 在场");
    Ok((module_id.to_string(), Vec::new()))
}

pub(in crate::mcp::project_ops) async fn tool_endpoint_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let mut a: EndpointCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }
    // 先验 HTTP 动词再碰权限表：输入级错误很便宜，不该被误报成权限拒绝。
    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    let if_exists = IfExists::parse(a.if_exists.as_deref().unwrap_or(""))?;

    // 端点自己那份 HTTPS 出站设置在这里转成域类型并校验，**在建集合之前**。
    //
    // 路径上的集合是逐级建出来的，每一级都单独提交。若这项校验留到写
    // `endpoint_details` 那一行才做（那里也校验），那么「集合建好了、TLS 被拒、
    // 端点没建成」就会留下一串没人要的集合。输入级的错误要在任何副作用之前报出来。
    let tls: Option<TlsSettings> = a.tls.take().map(Into::into);
    if let Some(t) = &tls {
        t.validate()?;
    }

    // 归属项目从「锚点」读 —— 锚点是只读的，过门之后才允许建路径上的集合。
    let anchor = anchor_of(&a)?;
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &anchor)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_create").await?;

    // 过门之后才建路径上的集合。
    let (parent_id, created_collections) = materialize_parent(state, &a)?;

    // 同一个父下已经有「同名 + 同 method + 同 url」的端点时按 if_exists 处置。
    let existing = simple_post_core::db::with_conn(&state.pool, |c| {
        spec::find_endpoint_by_identity(c, &parent_id, &a.name, &a.method, &a.url)
    })?;
    if let Some(existing_id) = existing {
        match if_exists {
            IfExists::Reuse => {
                // 直接返回它，**一个字段都不写**。要改字段用 `endpoint_update` ——
                // 那个工具只改它点名的字段，不会抹掉别的。
                return endpoint_response(
                    state,
                    &existing_id,
                    &project_id,
                    false,
                    Some(created_collections),
                );
            }
            IfExists::Error => {
                return Err(AppError::Validation(format!(
                    "an endpoint named '{}' with {} {} already exists under this parent; \
                     use endpoint_update to change it, or if_exists=duplicate to create a second one",
                    a.name,
                    method.as_str(),
                    a.url
                )));
            }
            IfExists::Duplicate => {}
        }
    }

    let body_mode_str = a.body_mode.clone().unwrap_or_else(|| "none".into());
    let body = RequestBody {
        mode: parse_body_mode(&body_mode_str)?,
        content: a.body_content.clone().unwrap_or_default(),
        schema: a.body_schema.clone(),
    };
    // 载荷与定义写反了就地拒绝：两份内容都是 JSON 对象，凭内容分不出来，但写错的
    // 后果是发出去的 body 是一份 Schema，而接口的文档里什么都没有。
    reject_schema_shaped_payload(&body)?;
    let auth = build_auth(a.auth_type.as_deref(), a.auth_config.clone())?;

    let now = simple_post_core::domain::now_ms();
    let id = mint_endpoint_id();
    let node = ResourceNode {
        id: id.clone(),
        root_id: project_id.clone(),
        parent_id: Some(parent_id),
        kind: ResourceNodeType::Endpoint,
        name: a.name,
        description: a.description,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    let detail = EndpointDetail {
        node_id: id,
        method,
        url: a.url,
        environment_id: a.environment_id,
        base_url_definition_id: a.base_url_definition_id,
        base_url_override: a.base_url_override,
        params: coerce_key_values(a.params.unwrap_or_default()),
        path_params: coerce_key_values(a.path_params.unwrap_or_default()),
        headers: coerce_key_values(a.headers.unwrap_or_default()),
        body,
        auth,
        docs: a.docs,
        notes: a.notes,
        tags: a.tags.unwrap_or_default(),
        // 两段脚本随请求配置一次写入，创建后不需要再调 `endpoint_update`。
        pre_request_script: a.pre_request_script,
        test_script: a.test_script,
        // 端点自己的 HTTPS 出站设置。`None` = 继承全局默认。
        // 上面已经转成域类型并校验过，这里直接用那一份。
        tls,
    };
    // node 与 detail 一起落库：没有配置行的端点是一棵打不开的树。
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::create(c, &node, Some(&detail))
    })?;
    endpoint_response(
        state,
        &node.id,
        &project_id,
        true,
        Some(created_collections),
    )
}

/// 组装写工具的响应：端点本体 + 「这次新建了哪些集合」 + 「它还缺什么」。
///
/// 三个字段都是**加**上去的，端点本体的形状不变，所以已经在读这个响应的客户端
/// 不受影响。
fn endpoint_response(
    state: &AppState,
    endpoint_id: &str,
    project_id: &str,
    created: bool,
    created_collections: Option<Vec<spec::Resolved>>,
) -> Result<CallToolResult, AppError> {
    let stored = endpoint_resource(&state.pool, endpoint_id)?;
    let mut out = serde_json::to_value(&stored)
        .map_err(|e| AppError::Validation(format!("endpoint response: {e}")))?;
    out["created"] = json!(created);
    if let Some(collections) = created_collections {
        out["created_collections"] = json!(collections
            .into_iter()
            .map(|r| json!({ "id": r.id, "created": r.created }))
            .collect::<Vec<_>>());
    }
    // 「还缺什么」让 agent 一次就知道要补什么，而不用先发一次请求、看报错、再回头补。
    if let Ok(project) = project_of(&state.pool, project_id) {
        let detail = simple_post_core::db::with_conn(&state.pool, |c| {
            simple_post_core::db::endpoint_details::get(c, endpoint_id)
        })?;
        out["missing"] = json!(missing_pieces(&state.pool, endpoint_id, &detail, &project));
    }
    Ok(CallToolResult::json(out))
}

pub(super) async fn tool_endpoint_update(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let endpoint_id = arg_str(&args, "endpoint_id")?.to_string();
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_update").await?;

    let patch_obj = args.get("patch").cloned().unwrap_or(Value::Null);
    if !patch_obj.is_object() {
        return Err(AppError::Validation(format!(
            "endpoint_update expects `patch` to be an object of fields to \
             change, got {patch_obj}"
        )));
    }

    // `name` 是结构、其余是配置，所以一次调用要写两张表。拆分在入口做而不是交给
    // 调用方，`endpoint_update` 才仍然是「改你点名的那些」—— 现有客户端都这么假设。
    //
    // 下面每个字段都用真 `Result`（`patch_field` / `patch_value_field`）读，绝不用
    // `.ok()`。字段有问题会在开事务**之前**中止，于是 patch 是全有或全无：agent 要
    // 么拿到点名了出错字段的 `Validation`，要么点名的字段全写进去。
    let node_patch = simple_post_core::domain::ResourceNodePatch {
        parent_id: None,
        // `name` / `sort_order` 在 patch 类型上是普通 `Option<T>`，没有「清空名字」
        // 这档状态，所以用单层读取器。
        name: patch_value_field::<String>(&patch_obj, "name")?,
        description: patch_field::<String>(&patch_obj, "description")?,
        sort_order: patch_value_field::<i64>(&patch_obj, "sort_order")?,
    };
    let detail_patch = EndpointDetailPatch {
        method: patch_value_field::<simple_post_core::domain::HttpMethod>(&patch_obj, "method")?,
        url: patch_value_field::<String>(&patch_obj, "url")?,
        environment_id: patch_field::<String>(&patch_obj, "environment_id")?,
        base_url_definition_id: patch_field::<String>(&patch_obj, "base_url_definition_id")?,
        base_url_override: patch_field::<String>(&patch_obj, "base_url_override")?,
        params: patch_value_field::<Vec<KeyValueInput>>(&patch_obj, "params")?
            .map(coerce_key_values),
        // 路径参数是域类型里的 `double_option` 字段（缺席 = 不动，`null` = 清空），
        // 故这里走 `patch_field` 而不是 `patch_value_field`：同一个字段在 REST / Tauri
        // 与 MCP 两条路上必须是同一个意思，否则调用方会一边清得掉、一边报 Validation。
        path_params: patch_field::<Vec<KeyValueInput>>(&patch_obj, "path_params")?
            .map(|rows| rows.map(coerce_key_values)),
        headers: patch_value_field::<Vec<KeyValueInput>>(&patch_obj, "headers")?
            .map(coerce_key_values),
        body: patch_value_field(&patch_obj, "body")?,
        auth: patch_value_field(&patch_obj, "auth")?,
        // `docs` / `notes` 必须能从这里读进来：它们只能经由本工具写入，写死 `None`
        // 等于让 agent 永远改不了自己刚读到的备注。
        docs: patch_field::<String>(&patch_obj, "docs")?,
        notes: patch_field::<String>(&patch_obj, "notes")?,
        tags: patch_value_field::<Vec<String>>(&patch_obj, "tags")?,
        // 两段脚本同样要能从这里读进来：写死 `None` 等于让 agent 永远配不了脚本。
        pre_request_script: patch_field::<String>(&patch_obj, "pre_request_script")?,
        test_script: patch_field::<String>(&patch_obj, "test_script")?,
        // HTTPS 出站设置走 `double_option`：缺席 = 不动，显式 `null` = 清除端点自带
        // 那份（回到继承全局），设值 = 写入。`patch_field` 的三档语义与域类型上的
        // `Option<Option<TlsSettings>>` 一一对应。
        //
        // 线格式用 `TlsSettingsInput` 而不是域类型：拼错的键名要报错，不能静默丢掉
        // 一个键后写进一份「看起来设好了、其实没有」的设置。
        tls: patch_field::<TlsSettingsInput>(&patch_obj, "tls")?.map(|inner| inner.map(Into::into)),
    };
    // 载荷与定义写反了就地拒绝 —— 与 `endpoint_create` 同一道门。只查这次 patch
    // 点名的 body：没点名 body 的 patch 不该因为端点早就存着的那份载荷被拒。
    if let Some(body) = detail_patch.body.as_ref() {
        reject_schema_shaped_payload(body)?;
    }
    if node_patch.is_empty() && detail_patch.is_empty() {
        return Err(AppError::Validation(
            "endpoint_update was given an empty patch: name at least one \
             field to change, e.g. {\"patch\":{\"url\":\"/v2/users\"}}. \
             Accepted fields: method, url, params, headers, body, auth, \
             tags, environment_id, base_url_definition_id, \
             base_url_override, docs, notes, pre_request_script, \
             test_script, tls, name, description, sort_order"
                .into(),
        ));
    }
    // 单事务：改名成功而 url 修改失败，是调用方察觉不到的半截 patch。
    simple_post_core::db::with_tx(&state.pool, |c| {
        if !node_patch.is_empty() {
            simple_post_core::db::nodes::update(c, &endpoint_id, node_patch.clone())?;
        }
        if !detail_patch.is_empty() {
            simple_post_core::db::endpoint_details::update(c, &endpoint_id, detail_patch.clone())?;
        }
        Ok(())
    })?;
    // 改完照样回报「还缺什么」：改 url 常常改出新的缺口（新的路径参数没有对应的
    // 变量、新的服务没有配 base URL）。
    endpoint_response(state, &endpoint_id, &node.root_id, false, None)
}

pub(super) async fn tool_endpoint_delete(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDeleteArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_delete args: {e}")))?;
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_delete").await?;
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::delete(c, &a.endpoint_id)
    })?;
    Ok(CallToolResult::json(json!({ "deleted": a.endpoint_id })))
}

pub(super) async fn tool_endpoint_duplicate(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointDuplicateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_duplicate args: {e}")))?;
    let ep = endpoint_resource(&state.pool, &a.endpoint_id)?;
    crate::mcp::check_permission(&state.pool, &ep.node.root_id, "endpoint_create").await?;

    // 副本紧挨着原件放，而不是扔到集合末尾 —— 看不出挨着谁的东西很容易被漏掉。
    // 第二个参数给 `None`：MCP 客户端没有乐观行要对齐，副本的名字由服务端起。
    let created = simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::duplicate(c, &a.endpoint_id, None)
    })?;
    let created = match a.name {
        Some(name) => simple_post_core::db::with_tx(&state.pool, |c| {
            simple_post_core::db::nodes::update(
                c,
                &created.id,
                simple_post_core::domain::ResourceNodePatch {
                    name: Some(name.clone()),
                    ..Default::default()
                },
            )
        })?,
        None => created,
    };
    let stored = endpoint_resource(&state.pool, &created.id)?;
    Ok(CallToolResult::json(stored))
}

pub(super) async fn tool_endpoint_move(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointMoveArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_move args: {e}")))?;
    let node = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::get(c, &a.endpoint_id)
    })?;
    crate::mcp::check_permission(&state.pool, &node.root_id, "endpoint_update").await?;

    // 单事务、单行、id 保持不变。
    //
    // 「删掉重建」既会丢数据（中途崩溃 = 端点没了），也会让调用方的 `endpoint_id`
    // 悬空 —— 响应还得告诉它新 id 是什么。`nodes::move_many` 是原地换父级。
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::move_many(
            c,
            std::slice::from_ref(&a.endpoint_id),
            Some(&a.new_collection_id),
            a.new_sort_order.unwrap_or(0) as i32,
        )
    })?;
    Ok(CallToolResult::json(json!({
        "endpoint_id": a.endpoint_id,
        "new_collection_id": a.new_collection_id,
    })))
}
