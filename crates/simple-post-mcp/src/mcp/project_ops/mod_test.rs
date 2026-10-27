use super::*;
use crate::mcp::AppState;
use simple_post_core::error::AppError;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// 每次调用自增，用来在同毫秒内构造多个 state() 时区分临时目录
/// （cargo 默认并行跑测试）。
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn state() -> std::sync::Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-project-ops-test-{}-{}-{}",
        std::process::id(),
        simple_post_core::domain::now_ms(),
        id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(crate::mcp::AppState {
        pool: simple_post_core::db::init(&dir).unwrap(),
        started_at: Instant::now(),
        metrics: std::sync::Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

#[test]
fn table_count_is_28() {
    assert_eq!(table().len(), 28);
}

#[tokio::test]
async fn project_create_rejects_empty_name() {
    let res = project::tool_project_create(&state(), serde_json::json!({ "name": "  " })).await;
    assert!(matches!(res, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn module_move_preserves_collections_and_endpoints() {
    // 回归网，防一次真实的数据丢失。`module_move` 一度是「删掉重建」，
    // 而 `collections.module_id` / `endpoints.collection_id` 都是 `ON DELETE CASCADE`，
    // 搬一个**有内容**的模块会连带悄悄销毁它下面所有集合、以及那些集合下的所有端点。
    // 改用一次真实的 `project_id` UPDATE 就不再有这个后果。
    use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
    use simple_post_core::domain::{
        EndpointDetail, Project, RequestBody, ResourceNode, ResourceNodeType,
    };
    use simple_post_core::permissions::Permission;

    let st = state();

    /// `Project` 没有 `Default`（每个字段都有意义），所以手工造这次移动所需的
    /// 两行最小数据。
    fn mk() -> Project {
        Project {
            id: String::new(),
            name: String::new(),
            description: None,
            base_url: None,
            base_url_definitions: vec![],
            default_base_url_definition_id: None,
            color: String::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn node(root: &str, id: &str, kind: ResourceNodeType, parent: Option<&str>) -> ResourceNode {
        ResourceNode {
            id: id.into(),
            root_id: root.into(),
            parent_id: parent.map(String::from),
            kind,
            name: "n".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(
            c,
            &Project {
                id: "p_move_a".into(),
                name: "A".into(),
                ..mk()
            },
        )?;
        projects::create(
            c,
            &Project {
                id: "p_move_b".into(),
                name: "B".into(),
                ..mk()
            },
        )?;
        nodes::create(
            c,
            &ResourceNode {
                sort_order: 0,
                ..node("p_move_a", "m_move", ResourceNodeType::Module, None)
            },
            None,
        )?;
        // 在源项目里留一个同胞，好让测试看清搬走之后空出来的位置。
        nodes::create(
            c,
            &ResourceNode {
                sort_order: 1,
                ..node("p_move_a", "m_stays", ResourceNodeType::Module, None)
            },
            None,
        )?;
        // 目标项目里先放两个模块，这样请求的插入位置才有意义，而不是被夹到一个空列表上。
        for (id, order) in [("m_b0", 0), ("m_b1", 1)] {
            nodes::create(
                c,
                &ResourceNode {
                    sort_order: order,
                    ..node("p_move_b", id, ResourceNodeType::Module, None)
                },
                None,
            )?;
        }
        nodes::create(
            c,
            &node(
                "p_move_a",
                "c_move",
                ResourceNodeType::Collection,
                Some("m_move"),
            ),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_move_a",
                "e_move",
                ResourceNodeType::Endpoint,
                Some("c_move"),
            ),
            Some(&EndpointDetail {
                node_id: "e_move".into(),
                method: simple_post_core::domain::HttpMethod::Get,
                url: "/x".into(),
                environment_id: None,
                base_url_definition_id: None,
                base_url_override: None,
                params: vec![],
                path_params: vec![],
                headers: vec![],
                body: RequestBody::default(),
                auth: Default::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
            }),
        )?;
        // 按项目的那扇门默认 `read_call`，低于移动要求的 `full`。必须在项目行存在
        // 之后才设 —— `agent_permissions.project_id` 是指向 projects 的外键。
        permissions::set(c, "p_move_b", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    // 把它移到目标项目模块列表的中间。
    module::tool_module_move(
        &st,
        serde_json::json!({
            "module_id": "m_move",
            "new_project_id": "p_move_b",
            "new_sort_order": 1
        }),
    )
    .await
    .expect("module_move should succeed");

    // 模块保留了 id，且现在归属目标项目。
    let after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "m_move"))
        .expect("module must still exist under its original id");
    assert_eq!(after.root_id, "p_move_b");
    assert_eq!(after.sort_order, 1);

    // 两个项目的顶层排序都保持连续：目标侧在插入处合拢，被离开的那个项目里也不会在
    // 模块原先的位置留下空洞。
    let top_level = |root: &str| {
        simple_post_core::db::with_conn(&st.pool, |c| {
            simple_post_core::db::nodes::list_by_root_and_type(c, root, ResourceNodeType::Module)
        })
        .unwrap()
        .into_iter()
        .map(|n| (n.id, n.sort_order))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        top_level("p_move_b"),
        vec![("m_b0".into(), 0), ("m_move".into(), 1), ("m_b1".into(), 2)],
        "the destination's modules must close around the inserted one"
    );
    assert_eq!(
        top_level("p_move_a"),
        vec![("m_stays".into(), 0)],
        "the project being left must not keep a gap in its sort_order"
    );

    // 关键：子行在移动中存活，且它们的 `root_id` 跟着一起重新推导。只更新模块行
    // ——`root_id` 布局之前的版本只能这么表达——会把整棵子树归到旧项目名下，让
    // `list_by_root` 看不见。
    let c_after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "c_move"))
        .expect("collection must survive the module move");
    assert_eq!(c_after.parent_id.as_deref(), Some("m_move"));
    assert_eq!(c_after.root_id, "p_move_b");

    let e_after = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, "e_move"))
        .expect("endpoint must survive the module move");
    assert_eq!(e_after.parent_id.as_deref(), Some("c_move"));
    assert_eq!(e_after.root_id, "p_move_b");

    // 端点的请求配置现在另存一张表，必须跟着一起搬过去。
    let detail = simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, "e_move"))
        .expect("endpoint detail must survive the module move");
    assert_eq!(detail.url, "/x");
}

#[tokio::test]
async fn endpoint_create_rejects_empty_url() {
    let res = endpoint::tool_endpoint_create(
        &state(),
        serde_json::json!({
            "collection_id": "c1",
            "name": "n",
            "method": "GET",
            "url": ""
        }),
    )
    .await;
    assert!(matches!(res, Err(AppError::Validation(_))));
}

#[tokio::test]
async fn endpoint_create_accepts_a_module_parent() {
    // 一个不需要自己集合的端点。约束矩阵之所以给端点列出两种父类型，正是为了不必先
    // 造一个用完即弃的集合；而 MCP 工具要能建出与 UI 一样的树，所以 `module_id` 与
    // `collection_id` 一样被接受。
    use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
    use simple_post_core::domain::{Project, ResourceNode, ResourceNodeType};
    use simple_post_core::permissions::Permission;

    let st = state();

    fn mk() -> Project {
        Project {
            id: String::new(),
            name: String::new(),
            description: None,
            base_url: None,
            base_url_definitions: vec![],
            default_base_url_definition_id: None,
            color: String::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn node(root: &str, id: &str, kind: ResourceNodeType, parent: Option<&str>) -> ResourceNode {
        ResourceNode {
            id: id.into(),
            root_id: root.into(),
            parent_id: parent.map(String::from),
            kind,
            name: "n".into(),
            description: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(
            c,
            &Project {
                id: "p_ep".into(),
                name: "P".into(),
                ..mk()
            },
        )?;
        nodes::create(
            c,
            &node("p_ep", "m_ep", ResourceNodeType::Module, None),
            None,
        )?;
        // 按项目的那扇门默认 `read_call`，低于创建要求的 `full`。
        permissions::set(c, "p_ep", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = endpoint::tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ep",
            "name": "root endpoint",
            "method": "GET",
            "url": "/x"
        }),
    )
    .await
    .expect("creating an endpoint under a module must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    // `EndpointResource` 把 node 摊平，所以它的字段与 `detail` 平级。
    assert_eq!(created["kind"], "endpoint");
    assert_eq!(created["parentId"], "m_ep");
    assert_eq!(created["rootId"], "p_ep");

    // detail 行必须与 node 一起落库，否则树上会留一行打不开的东西。
    let id = created["id"].as_str().expect("node id").to_string();
    assert_eq!(
        simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, &id))
            .expect("detail row")
            .url,
        "/x"
    );
}

#[tokio::test]
async fn endpoint_create_requires_some_parent() {
    // 两个父 id 都没给：没有父级的端点不构成一棵树，所以这是输入错误 —— 不是权限
    // 拒绝。
    let res = endpoint::tool_endpoint_create(
        &state(),
        serde_json::json!({
            "name": "n",
            "method": "GET",
            "url": "/x"
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "expected Validation rejection, got {res:?}"
    );
}

#[tokio::test]
async fn endpoint_create_rejects_unknown_method_gracefully() {
    // method 为空是畸形 HTTP —— 应当报 `Validation` 错误（LLM / UI 拿到
    // `is_error=true` 文本），而不是 panic 或把垃圾写进库。`PROPFIND` /
    // `FROBNICATE` 这类自定义形状的动词仍会以 `HttpMethod::Custom` 放行。
    let res = endpoint::tool_endpoint_create(
        &state(),
        serde_json::json!({
            "collection_id": "c1",
            "name": "n",
            "method": "",
            "url": "/x"
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "expected Validation rejection, got {res:?}"
    );
}

/// `parse_body_mode` 住在 `endpoint.rs`（只被 `endpoint_create` 用）。
/// 经由 `endpoint` 子模块的可见性来测 —— 其实它是文件私有的，所以用一个测试专用的薄
/// 包装重新暴露出来。
#[test]
fn parse_body_mode_known_values() {
    // 经 `super::endpoint::__test_parse_body_mode`（下方定义的仅测试再导出）访问该
    // 辅助函数，免得为了测试把 `parse_body_mode` 提成 `pub(super)` 给生产代码看。
    assert!(super::endpoint::__test_parse_body_mode("json").is_ok());
    assert!(super::endpoint::__test_parse_body_mode("form").is_ok());
    assert!(super::endpoint::__test_parse_body_mode("raw").is_ok());
    assert!(super::endpoint::__test_parse_body_mode("multipart").is_ok());
    assert!(super::endpoint::__test_parse_body_mode("none").is_ok());
    assert!(super::endpoint::__test_parse_body_mode("wat").is_err());
}

#[test]
fn build_auth_handles_each_kind() {
    let none = super::endpoint::__test_build_auth(Some("none"), None).unwrap();
    assert_eq!(none.kind, simple_post_core::domain::AuthType::None);

    let bearer = super::endpoint::__test_build_auth(
        Some("bearer"),
        Some(serde_json::json!({"bearer": "tok"})),
    )
    .unwrap();
    assert_eq!(bearer.kind, simple_post_core::domain::AuthType::Bearer);

    let unknown = super::endpoint::__test_build_auth(Some("magic"), None);
    assert!(unknown.is_err());
}
