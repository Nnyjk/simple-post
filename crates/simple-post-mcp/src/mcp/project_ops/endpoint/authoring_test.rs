//! 「一次调用把接口规范建出来」这条路上的四件事：
//!
//! 1. 两段脚本能在**创建**时一次写进去。
//! 2. `collection_path` 能把路径上的集合逐级建出来，末级就是端点的父。
//! 3. `if_exists=reuse` 让重复调用不再造出第二份。
//! 4. 响应里的 `missing` 说清这个端点还差什么。
//!
//! 另有一条：`endpoint_create` 遇到不认识的字段必须**报错**，而不是静默丢掉。

use super::test_support::{detail_of, fixture, state};
use super::write::tool_endpoint_create;
use simple_post_core::db::{environments, nodes, permissions, projects};
use simple_post_core::domain::{Environment, ResourceNodeType};
use simple_post_core::error::AppError;
use simple_post_core::permissions::Permission;

/// 从工具响应里取出 JSON 主体。
fn body(res: crate::mcp::CallToolResult) -> serde_json::Value {
    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    serde_json::from_str(text).expect("json result")
}

// =========================================================================
// 1. 脚本
// =========================================================================

/// 两段脚本必须在**创建**时就能写进去。
///
/// `tool_endpoint_create` 一次就把 `pre_request_script` 与 `test_script` 写进请求
/// 配置，创建带脚本的接口不必再调一次 `endpoint_update`。
#[tokio::test]
async fn endpoint_create_stores_both_scripts_in_one_call() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "带脚本的接口",
            "method": "POST",
            "url": "/api/orders",
            "pre_request_script": "pm.environment.set('staged', '1');",
            "test_script": "pm.test('ok', pm.response.code === 200);"
        }),
    )
    .await
    .expect("a create with scripts must be accepted");

    let created = body(res);
    let id = created["id"].as_str().expect("id").to_string();
    let d = detail_of(&st, &id);
    assert_eq!(
        d.pre_request_script.as_deref(),
        Some("pm.environment.set('staged', '1');")
    );
    assert_eq!(
        d.test_script.as_deref(),
        Some("pm.test('ok', pm.response.code === 200);")
    );
}

// =========================================================================
// 2. 路径寻址
// =========================================================================

/// `collection_path` 一次就把路径上的集合建出来，端点落在末级。
///
/// 这是「先查一遍再建一下」那十几步里最费的一段：agent 原来要先
/// `collection_list` 才知道集合在不在，不在就 `collection_create`，再拿 id 去建端点。
#[tokio::test]
async fn endpoint_create_builds_the_collections_on_the_path() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "collection_path": "订单/明细",
            "name": "创建订单",
            "method": "POST",
            "url": "/api/orders"
        }),
    )
    .await
    .expect("a create with a collection path must be accepted");

    let created = body(res);
    let id = created["id"].as_str().expect("id").to_string();

    // 路径上的两级都被建了出来。
    let levels = created["created_collections"]
        .as_array()
        .expect("created_collections");
    assert_eq!(levels.len(), 2, "{levels:?}");
    assert!(levels
        .iter()
        .all(|l| l["created"] == serde_json::json!(true)));

    // 端点落在末级集合下。
    let endpoint = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, &id)).unwrap();
    let leaf = levels[1]["id"].as_str().expect("leaf id");
    assert_eq!(endpoint.parent_id.as_deref(), Some(leaf));

    let leaf_node = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, leaf)).unwrap();
    assert_eq!(leaf_node.kind, ResourceNodeType::Collection);
    assert_eq!(leaf_node.name, "明细");
    let middle = levels[0]["id"].as_str().expect("middle id");
    assert_eq!(leaf_node.parent_id.as_deref(), Some(middle));
}

/// 再调一次同样的调用，集合不重复建，端点也不重复建。
#[tokio::test]
async fn repeating_the_same_call_does_not_duplicate_the_tree() {
    let (st, _) = fixture();
    let args = serde_json::json!({
        "module_id": "m_ep_fix",
        "collection_path": "订单/明细",
        "name": "创建订单",
        "method": "POST",
        "url": "/api/orders"
    });

    let first = body(tool_endpoint_create(&st, args.clone()).await.unwrap());
    let second = body(tool_endpoint_create(&st, args).await.unwrap());

    assert_eq!(first["id"], second["id"], "同一个接口不该建出第二份");
    // `created` 说的是「这一次是不是新建的」：第一次建出来是 true，第二次拿到的是
    // 已有的那一个，是 false —— 它一个字段都没写。
    assert_eq!(first["created"], serde_json::json!(true));
    assert_eq!(second["created"], serde_json::json!(false));

    // 路径上的集合也只建过一次。
    let levels = second["created_collections"]
        .as_array()
        .expect("created_collections");
    assert!(
        levels
            .iter()
            .all(|l| l["created"] == serde_json::json!(false)),
        "第二次调用不该再建集合：{levels:?}"
    );
}

/// `if_exists=error` 遇到已有的同名同 method 同 url 时拒绝，而不是默默建第二份。
#[tokio::test]
async fn the_error_mode_refuses_to_build_a_second_copy() {
    let (st, _) = fixture();
    let args = serde_json::json!({
        "module_id": "m_ep_fix",
        "name": "重复的接口",
        "method": "GET",
        "url": "/api/dup"
    });
    tool_endpoint_create(&st, args.clone()).await.unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "重复的接口",
            "method": "GET",
            "url": "/api/dup",
            "if_exists": "error"
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "if_exists=error 必须拒绝：{res:?}"
    );
}

/// 名字相同但 method 或 url 不同的，是两个接口 —— 不能被认成一个。
#[tokio::test]
async fn a_different_method_is_a_different_endpoint() {
    let (st, _) = fixture();
    tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "同一个名字",
            "method": "GET",
            "url": "/api/same"
        }),
    )
    .await
    .unwrap();

    let res = body(
        tool_endpoint_create(
            &st,
            serde_json::json!({
                "module_id": "m_ep_fix",
                "name": "同一个名字",
                "method": "POST",
                "url": "/api/same"
            }),
        )
        .await
        .unwrap(),
    );
    assert_eq!(res["created"], serde_json::json!(true), "{res}");
}

// =========================================================================
// 3. 不认识的字段
// =========================================================================

/// 不认识的字段必须报错。
///
/// `EndpointCreateArgs` 带 `deny_unknown_fields`，后端不读的字段不会被静默丢掉，
/// 工具会把它当拼错的字段名报回来。
#[tokio::test]
async fn an_unknown_field_is_rejected_instead_of_being_dropped() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "name": "n",
            "method": "GET",
            "url": "/api/x",
            "preRequestScript": "console.log('oops')"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("an unknown field must be rejected, got {res:?}");
    };
    assert!(
        message.contains("preRequestScript"),
        "错误信息要点名那个不认识的字段：{message}"
    );
}

// =========================================================================
// 4. 「还缺什么」
// =========================================================================

/// 响应里的 `missing` 要说清缺的是哪个变量。
///
/// 变量不在环境里，请求就会把 `{{token}}` 原样发出去 —— 服务端收到的是字面量，
/// 而建端点那一步不会失败，所以只能靠提示告诉 agent。
#[tokio::test]
async fn the_response_names_the_variable_the_environment_is_missing() {
    let (st, _) = fixture();
    simple_post_core::db::with_tx(&st.pool, |c| {
        environments::create(
            c,
            &Environment {
                id: "env_gap".into(),
                project_id: "p_ep_fix".into(),
                name: "dev".into(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .unwrap();

    let res = body(
        tool_endpoint_create(
            &st,
            serde_json::json!({
                "module_id": "m_ep_fix",
                "name": "要 token 的接口",
                "method": "GET",
                "url": "/api/me",
                "auth_type": "bearer",
                "auth_config": { "bearer": "{{token}}" }
            }),
        )
        .await
        .unwrap(),
    );

    let missing = res["missing"].as_array().expect("missing");
    assert!(
        missing
            .iter()
            .any(|m| m.as_str().unwrap_or_default().contains("token")),
        "缺的那个变量要点名：{missing:?}"
    );
}

/// 环境里有那个变量时，就不该报它缺。
#[tokio::test]
async fn a_variable_the_environment_has_is_not_reported_as_missing() {
    let (st, _) = fixture();
    simple_post_core::db::with_tx(&st.pool, |c| {
        environments::create(
            c,
            &Environment {
                id: "env_ok".into(),
                project_id: "p_ep_fix".into(),
                name: "dev".into(),
                module_base_urls: Default::default(),
                variables: [("token".to_string(), "t-1".to_string())]
                    .into_iter()
                    .collect(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .unwrap();

    let res = body(
        tool_endpoint_create(
            &st,
            serde_json::json!({
                "module_id": "m_ep_fix",
                "name": "有 token 的接口",
                "method": "GET",
                "url": "/api/me",
                "auth_type": "bearer",
                "auth_config": { "bearer": "{{token}}" }
            }),
        )
        .await
        .unwrap(),
    );

    let missing = res["missing"].as_array().expect("missing");
    assert!(
        !missing
            .iter()
            .any(|m| m.as_str().unwrap_or_default().contains("token")),
        "环境里有的变量不该被报成缺：{missing:?}"
    );
}

/// 模块 id 与项目 id 不匹配时要报错，不是建到别的地方去。
#[tokio::test]
async fn a_path_without_a_module_is_rejected() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "collection_path": "订单/明细",
            "name": "n",
            "method": "GET",
            "url": "/api/x"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("collection_path without module_id must be rejected, got {res:?}");
    };
    assert!(message.contains("module_id"), "{message}");
}

/// 路径里有一级是空的，报错说明是哪一段空着。
#[tokio::test]
async fn a_path_with_an_empty_segment_is_rejected() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "collection_path": "订单//明细",
            "name": "n",
            "method": "GET",
            "url": "/api/x"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("a path with an empty segment must be rejected, got {res:?}");
    };
    assert!(message.contains("空的一段"), "{message}");
}

/// `collection_id` 与 `collection_path` 都在说「挂在哪」，两个一起给就拒 ——
/// 悄悄挑一个会让调用方以为建在它说的那个父下。
#[tokio::test]
async fn giving_both_a_collection_id_and_a_path_is_rejected() {
    let (st, _) = fixture();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "collection_id": "c_ep_fix",
            "collection_path": "订单/明细",
            "name": "n",
            "method": "GET",
            "url": "/api/x"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("two conflicting ways of naming the parent must be rejected, got {res:?}");
    };
    assert!(message.contains("二选一"), "{message}");
}

/// `collection_create` 走路径时，缺的那几级会被建出来，重复调用不会重建。
#[tokio::test]
async fn collection_create_builds_the_path_and_is_idempotent() {
    let (st, _) = fixture();

    let args = serde_json::json!({
        "module_id": "m_ep_fix",
        "path": "订单/明细"
    });
    let first = body(
        crate::mcp::project_ops::collection::tool_collection_create(&st, args.clone())
            .await
            .expect("a create with a path must be accepted"),
    );
    let second = body(
        crate::mcp::project_ops::collection::tool_collection_create(&st, args)
            .await
            .expect("a repeated create must be accepted"),
    );

    assert_eq!(first["id"], second["id"], "同一条路径不该建出第二份");
    assert_eq!(first["created"], serde_json::json!(true));
    assert_eq!(second["created"], serde_json::json!(false));

    let levels = second["created_collections"]
        .as_array()
        .expect("created_collections");
    assert_eq!(levels.len(), 2, "{levels:?}");
    assert!(
        levels
            .iter()
            .all(|l| l["created"] == serde_json::json!(false)),
        "第二次调用不该再建集合：{levels:?}"
    );
}

/// `collection_create` 的 `path` 与 `parent_collection_id` 也一样是两种说法打架。
#[tokio::test]
async fn collection_create_rejects_a_path_together_with_a_parent() {
    let (st, _) = fixture();

    let res = crate::mcp::project_ops::collection::tool_collection_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep_fix",
            "parent_collection_id": "c_ep_fix",
            "path": "订单/明细"
        }),
    )
    .await;

    let Err(AppError::Validation(message)) = res else {
        panic!("two conflicting ways of naming the parent must be rejected, got {res:?}");
    };
    assert!(message.contains("二选一"), "{message}");
}

/// 项目没给权限时，创建要过不去 —— 路径上的集合也不能被建出来。
#[tokio::test]
async fn the_permission_gate_runs_before_the_collections_are_created() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &super::test_support::bare_project("p_gate", "G"))?;
        nodes::create(
            c,
            &super::test_support::node("p_gate", "m_gate", ResourceNodeType::Module, None),
            None,
        )?;
        // 默认是 ReadCall，低于创建所需的 Full。
        permissions::set(c, "p_gate", Permission::ReadCall)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_gate",
            "collection_path": "订单/明细",
            "name": "n",
            "method": "GET",
            "url": "/api/x"
        }),
    )
    .await;
    assert!(res.is_err(), "权限不足时必须拒绝：{res:?}");

    // 路径上的集合一个也不该被建出来。
    let collections = simple_post_core::db::with_conn(&st.pool, |c| {
        Ok(nodes::list_by_root_and_type(c, "p_gate", ResourceNodeType::Collection)?.len())
    })
    .unwrap();
    assert_eq!(collections, 0, "权限门必须先于建集合生效");
}
