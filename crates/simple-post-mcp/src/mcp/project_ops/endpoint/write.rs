//! 写侧 handler：`endpoint_create` / `_update` / `_delete` / `_duplicate` / `_move`。
//!
//! 过门一律由父节点或端点自身经 `nodes::root_id_of` 解析归属项目，不接受调用方
//! 自称的项目 id。

use serde_json::{json, Value};

use super::args::{
    mint_endpoint_id, EndpointCreateArgs, EndpointDeleteArgs, EndpointDuplicateArgs,
    EndpointMoveArgs,
};
use super::input::{build_auth, coerce_key_values, parse_body_mode, KeyValueInput};
use super::patch::{patch_field, patch_value_field};
use super::resource::endpoint_resource;
use crate::mcp::{arg_str, AppState, CallToolResult};
use simple_post_core::domain::{
    EndpointDetail, EndpointDetailPatch, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;
pub(in crate::mcp::project_ops) async fn tool_endpoint_create(
    state: &AppState,
    args: Value,
) -> Result<CallToolResult, AppError> {
    let a: EndpointCreateArgs = serde_json::from_value(args)
        .map_err(|e| AppError::Validation(format!("endpoint_create args: {e}")))?;
    if a.name.trim().is_empty() {
        return Err(AppError::Validation("name must not be empty".into()));
    }
    if a.url.trim().is_empty() {
        return Err(AppError::Validation("url must not be empty".into()));
    }
    // 先验 HTTP 动词再碰权限表：输入级错误很便宜，不该被误报成权限拒绝。
    let method = simple_post_core::domain::parse_http_method(&a.method)?;
    // 端点的父级是集合，或者调用方没建文件夹时直接是模块。`collection_id` 与
    // `module_id` 是同一关系的两个名字（`collection_create` 用的也是同一形状），
    // 取客户端给的那个，不成对的组合交给约束矩阵去拒。
    let parent_id = a
        .collection_id
        .clone()
        .or_else(|| a.module_id.clone())
        .ok_or_else(|| {
            AppError::Validation(
                "endpoint_create needs a parent: pass collection_id to put the \
                 endpoint inside a collection, or module_id to put it directly \
                 under a module"
                    .into(),
            )
        })?;
    // 按父节点的真实项目过门。写死字面量 "global" 不是项目 id，过门时只能拿默认的
    // `ReadCall` 去比创建工具要求的 `Full`，于是每次调用都被拒。
    let project_id = simple_post_core::db::with_conn(&state.pool, |c| {
        simple_post_core::db::nodes::root_id_of(c, &parent_id)
    })?;
    crate::mcp::check_permission(&state.pool, &project_id, "endpoint_create").await?;

    let body_mode_str = a.body_mode.clone().unwrap_or_else(|| "none".into());
    let body = RequestBody {
        mode: parse_body_mode(&body_mode_str)?,
        content: a.body_content.clone().unwrap_or_default(),
        schema: a.body_schema.clone(),
    };
    let auth = build_auth(a.auth_type.as_deref(), a.auth_config.clone())?;

    let now = simple_post_core::domain::now_ms();
    let id = mint_endpoint_id();
    let node = ResourceNode {
        id: id.clone(),
        root_id: project_id,
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
        headers: coerce_key_values(a.headers.unwrap_or_default()),
        body,
        auth,
        docs: a.docs,
        notes: a.notes,
        tags: a.tags.unwrap_or_default(),
        pre_request_script: None,
        test_script: None,
    };
    // node 与 detail 一起落库：没有配置行的端点是一棵打不开的树。
    simple_post_core::db::with_tx(&state.pool, |c| {
        simple_post_core::db::nodes::create(c, &node, Some(&detail))
    })?;
    let stored = endpoint_resource(&state.pool, &node.id)?;
    Ok(CallToolResult::json(stored))
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
    };
    if node_patch.is_empty() && detail_patch.is_empty() {
        return Err(AppError::Validation(
            "endpoint_update was given an empty patch: name at least one \
             field to change, e.g. {\"patch\":{\"url\":\"/v2/users\"}}. \
             Accepted fields: method, url, params, headers, body, auth, \
             tags, environment_id, base_url_definition_id, \
             base_url_override, docs, notes, pre_request_script, \
             test_script, name, description, sort_order"
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
    let updated = endpoint_resource(&state.pool, &endpoint_id)?;
    Ok(CallToolResult::json(updated))
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
