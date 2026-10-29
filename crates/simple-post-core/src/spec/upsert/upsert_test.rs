//! `core::spec::upsert` 的测试：一份规格落成库里那些行。
//!
//! 这一组守的是「一次调用把一整份接口规范落进库里」这件事的各个侧面：
//! 幂等、冲突策略、变量合并、URL 那一格的落点，以及几处**必须报错**的形状
//! （载荷与定义写反、跨项目的 id、两个互斥的寻址方式同时给）。

use crate::db::test_helpers::fresh_pool;
use crate::db::{self, DbPool};
use crate::domain::{AuthType, BodyMode, ResourceNodeType};
use crate::spec::upsert::{upsert_endpoint_spec, EndpointSpec};
use serde_json::json;

/// 一份最小可用的规格：只指名项目、模块与端点。
fn minimal_spec() -> serde_json::Value {
    json!({
        "project_id": "p_demo",
        "module": "营销",
        "endpoint": {
            "name": "查询活动",
            "method": "GET",
            "url": "/api/campaigns"
        }
    })
}

/// 把一份 JSON 规格落进库里。**反序列化失败也算失败** —— 它同样是一份 agent
/// 拼错的规格，报错信息里要点名那个不认识的字段。
fn apply(pool: &DbPool, v: serde_json::Value) -> crate::error::AppError {
    let spec: EndpointSpec = match serde_json::from_value(v) {
        Ok(s) => s,
        Err(e) => {
            return crate::error::AppError::Validation(format!("这份规格反序列化失败：{e}"));
        }
    };
    match db::with_tx(pool, |c| upsert_endpoint_spec(c, &spec)) {
        Ok(_) => unreachable!("这一次调用本该失败"),
        Err(e) => e,
    }
}

fn apply_ok(pool: &DbPool, v: serde_json::Value) -> crate::spec::upsert::SpecOutcome {
    let spec: EndpointSpec = serde_json::from_value(v).expect("这份规格应当能反序列化");
    db::with_tx(pool, |c| upsert_endpoint_spec(c, &spec)).expect("这一次调用本该成功")
}

// =========================================================================
// 路径参数
// =========================================================================

/// 路径参数与查询参数分开存：查询参数进查询串，路径参数进路径本身。
///
/// 两者混在一份列表里，「这个参数进哪儿」就取决于 URL 文本 —— 改一个占位符的名字
/// 会静默改变一个参数的归属。故规格把它们分成两段，落到 detail 的两处。
#[test]
fn path_parameters_land_in_their_own_list() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查订单",
                "method": "GET",
                "url": "/api/orders/{id}",
                "params": [{ "key": "page", "value": "1" }],
                "path_params": [{ "key": "id", "value": "42" }]
            }
        }),
    );

    let detail = &outcome.endpoint.resource.detail;
    assert_eq!(
        detail.path_params.len(),
        1,
        "路径参数要落进自己的那一份，不能混进查询参数"
    );
    assert_eq!(detail.path_params[0].key, "id");
    assert_eq!(detail.path_params[0].value, "42");
    assert!(
        detail.path_params[0].enabled,
        "路径参数缺省是启用的 —— 存成禁用就是一条永远发不出去的路径"
    );
    assert_eq!(detail.params.len(), 1, "查询参数那一份不受影响");
    assert_eq!(detail.params[0].key, "page");
}

// =========================================================================
// 一份完整的规格
// =========================================================================

#[test]
fn a_full_spec_lands_the_module_the_collections_the_environment_and_the_endpoint() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "collection_path": "活动/详情",
            "environment": {
                "name": "gray",
                "variables": { "token": "gray-token", "region": "cn-east", "legacy": "old" },
                "disabled_variables": ["legacy"],
                "base_url": "https://gray.example.com",
                "activate": true
            },
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "params": [{ "key": "page", "value": "1" }],
                "headers": [{ "key": "X-Trace", "value": "1" }],
                "tags": ["营销"],
                "pre_request_script": "pm.environment.set('a', 'b');",
                "test_script": "pm.test('ok', () => pm.response.code === 200);"
            }
        }),
    );

    // 模块：新建。
    assert!(outcome.module.created);
    assert_eq!(outcome.module.name, "营销");

    // 集合：路径上两级都建出来了。
    assert_eq!(outcome.collections.len(), 2);
    assert!(outcome.collections.iter().all(|c| c.created));
    assert_eq!(outcome.collections[0].name, "活动");
    assert_eq!(outcome.collections[1].name, "详情");

    // 环境：新建并激活。
    let env = outcome.environment.expect("规格里有环境");
    assert!(env.created);
    assert!(env.activated);

    // 端点：新建。
    assert!(outcome.endpoint.created);
    assert!(!outcome.endpoint.updated);

    db::with_conn(&pool, |c| {
        // 端点挂在末级集合下。
        let node = db::nodes::get(c, &outcome.endpoint.resource.node.id)?;
        assert_eq!(node.kind, ResourceNodeType::Endpoint);
        assert_eq!(
            node.parent_id.as_deref(),
            Some(outcome.collections[1].id.as_str())
        );
        assert_eq!(node.root_id, "p_demo");

        // 请求配置真的落库了。
        let detail = db::endpoint_details::get(c, &outcome.endpoint.resource.node.id)?;
        assert_eq!(detail.method.as_str(), "GET");
        assert_eq!(detail.url, "/api/campaigns");
        assert_eq!(detail.params.len(), 1);
        assert_eq!(detail.params[0].key, "page");
        assert!(detail.params[0].enabled, "enabled 缺省要是 true");
        assert_eq!(detail.headers.len(), 1);
        assert_eq!(detail.tags, vec!["营销".to_string()]);
        assert_eq!(
            detail.pre_request_script.as_deref(),
            Some("pm.environment.set('a', 'b');")
        );
        assert!(detail.test_script.is_some());
        // 端点绑定到规格里的环境。
        assert_eq!(detail.environment_id.as_deref(), Some(env.id.as_str()));

        // 那一格地址落在 (环境 × 模块) 上。
        let stored_env = db::environments::get(c, &env.id)?;
        let cell = stored_env.module_base_urls.get(&outcome.module.id);
        assert_eq!(
            cell.map(|c| c.url.as_str()),
            Some("https://gray.example.com")
        );

        // 环境里的变量。
        assert_eq!(
            stored_env.variables.get("token").map(String::as_str),
            Some("gray-token")
        );
        assert_eq!(
            stored_env.variables.get("region").map(String::as_str),
            Some("cn-east")
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_environment_base_url_lands_in_the_cell_of_the_specified_module() {
    // 地址按模块分格，故规格指名哪个模块，地址就写进哪一格。
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "environment": { "name": "gray", "base_url": "https://gray.example.com" },
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );

    let env_id = outcome.environment.expect("规格里有环境").id;
    db::with_conn(&pool, |c| {
        let stored_env = db::environments::get(c, &env_id)?;
        assert_eq!(stored_env.module_base_urls.len(), 1, "只有指名的那一格");
        let cell = stored_env.module_base_urls.get(&outcome.module.id);
        assert_eq!(
            cell.map(|c| c.url.as_str()),
            Some("https://gray.example.com")
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_environment_base_url_proxy_lands_in_the_same_cell() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "environment": {
                "name": "gray",
                "base_url": "https://gray.example.com",
                "base_url_proxy": "https://gw.example.com/gray"
            },
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );

    let env_id = outcome.environment.expect("规格里有环境").id;
    db::with_conn(&pool, |c| {
        let stored_env = db::environments::get(c, &env_id)?;
        let cell = stored_env.module_base_urls.get(&outcome.module.id);
        assert_eq!(
            cell.map(|c| c.url.as_str()),
            Some("https://gray.example.com")
        );
        assert_eq!(
            cell.and_then(|c| c.proxy.as_deref()),
            Some("https://gw.example.com/gray")
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_endpoint_without_a_collection_sits_directly_under_the_module() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(&pool, minimal_spec());
    assert!(outcome.collections.is_empty());
    assert_eq!(outcome.parent_id, outcome.module.id);
}

// =========================================================================
// 幂等
// =========================================================================

#[test]
fn applying_the_same_spec_twice_creates_nothing_the_second_time() {
    let (pool, _dir) = fresh_pool();
    let spec = json!({
        "project_id": "p_demo",
        "module": "营销",
        "collection_path": "活动",
        "environment": { "name": "gray", "variables": { "token": "t" } },
        "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
    });

    let first = apply_ok(&pool, spec.clone());
    let second = apply_ok(&pool, spec);

    assert!(first.module.created);
    assert!(!second.module.created, "第二次不该重建模块");
    assert!(!second.collections[0].created, "第二次不该重建集合");
    assert!(first.endpoint.created);
    assert!(!second.endpoint.created, "第二次不该重建端点");
    assert_eq!(
        first.endpoint.resource.node.id,
        second.endpoint.resource.node.id
    );
    assert!(
        !second.environment.expect("规格里有环境").created,
        "第二次不该重建环境"
    );
}

#[test]
fn applying_the_same_spec_twice_does_not_grow_the_tree() {
    let (pool, _dir) = fresh_pool();
    let spec = json!({
        "project_id": "p_demo",
        "module": "营销",
        "collection_path": "活动/详情",
        "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
    });
    let count = |pool: &DbPool| -> usize {
        db::with_conn(pool, |c| {
            Ok(db::nodes::list_by_root(c, "p_demo").unwrap().len())
        })
        .unwrap()
    };
    let before = count(&pool);
    apply_ok(&pool, spec.clone());
    let after_first = count(&pool);
    apply_ok(&pool, spec);
    assert_eq!(
        count(&pool),
        after_first,
        "第二次应用规格之后，树上不该多出任何节点"
    );
    assert_eq!(after_first, before + 4, "模块 + 两级集合 + 端点");
}

// =========================================================================
// 端点的冲突策略
// =========================================================================

#[test]
fn reuse_returns_the_existing_endpoint_without_writing_a_single_field() {
    let (pool, _dir) = fresh_pool();
    let first = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "docs": "第一版说明"
            }
        }),
    );
    let second = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "docs": "第二版说明"
            }
        }),
    );

    assert!(!second.endpoint.created);
    assert!(!second.endpoint.updated);
    assert_eq!(
        first.endpoint.resource.node.id,
        second.endpoint.resource.node.id
    );

    db::with_conn(&pool, |c| {
        let detail = db::endpoint_details::get(c, &second.endpoint.resource.node.id)?;
        assert_eq!(
            detail.docs.as_deref(),
            Some("第一版说明"),
            "reuse 模式下规格里的字段一个都不该写进去"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn error_mode_refuses_when_the_same_endpoint_already_exists() {
    let (pool, _dir) = fresh_pool();
    apply_ok(&pool, minimal_spec());
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "if_exists": "error",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("查询活动"), "{e}");
    assert!(e.to_string().contains("if_exists=update"), "{e}");
}

#[test]
fn duplicate_mode_builds_a_second_endpoint_with_the_same_identity() {
    let (pool, _dir) = fresh_pool();
    let first = apply_ok(&pool, minimal_spec());
    let second = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "if_exists": "duplicate",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(second.endpoint.created);
    assert_ne!(
        first.endpoint.resource.node.id,
        second.endpoint.resource.node.id
    );
}

#[test]
fn update_mode_overwrites_the_fields_the_spec_names_and_leaves_the_others_alone() {
    let (pool, _dir) = fresh_pool();
    apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "docs": "第一版说明",
                "notes": "原来的备注",
                "tags": ["旧标签"]
            }
        }),
    );
    let second = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "if_exists": "update",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "docs": "第二版说明"
            }
        }),
    );
    assert!(!second.endpoint.created);
    assert!(second.endpoint.updated);

    db::with_conn(&pool, |c| {
        let detail = db::endpoint_details::get(c, &second.endpoint.resource.node.id)?;
        assert_eq!(
            detail.docs.as_deref(),
            Some("第二版说明"),
            "点名的字段要被覆盖"
        );
        assert_eq!(
            detail.notes.as_deref(),
            Some("原来的备注"),
            "规格没点名的字段保持原样"
        );
        assert_eq!(detail.tags, vec!["旧标签".to_string()]);
        Ok(())
    })
    .unwrap();
}

#[test]
fn update_mode_creates_the_endpoint_when_it_does_not_exist_yet() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "if_exists": "update",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(outcome.endpoint.created);
    assert!(!outcome.endpoint.updated);
}

// =========================================================================
// 环境与变量
// =========================================================================

#[test]
fn the_spec_variables_merge_into_the_environment_instead_of_replacing_it() {
    let (pool, _dir) = fresh_pool();
    // fixture 里 env_dev 有 token 与 env 两个变量。
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "environment": { "name": "dev", "variables": { "campaign": "spring" } },
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    let env_id = outcome.environment.expect("规格里有环境").id;
    db::with_conn(&pool, |c| {
        let env = db::environments::get(c, &env_id)?;
        assert_eq!(
            env.variables.get("campaign").map(String::as_str),
            Some("spring")
        );
        assert_eq!(
            env.variables.get("token").map(String::as_str),
            Some("eyJhbGciOiJIUzI1NiJ9.dev"),
            "规格没提到的变量不能被抹掉"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn disabling_a_variable_that_the_environment_does_not_have_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "environment": { "name": "dev", "disabled_variables": ["并不存在的变量"] },
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("并不存在的变量"), "{e}");
}

#[test]
fn a_spec_without_an_environment_section_leaves_the_environment_untouched() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(&pool, minimal_spec());
    assert!(outcome.environment.is_none());
    db::with_conn(&pool, |c| {
        let envs = db::environments::list_by_project(c, "p_demo")?;
        assert_eq!(envs.len(), 3, "fixture 里有三个环境，不该多出第四个");
        Ok(())
    })
    .unwrap();
}

// =========================================================================
// 认证与载荷
// =========================================================================

#[test]
fn the_spec_auth_carries_the_scheme_and_the_credential() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "auth": { "kind": "bearer", "bearer": "abc" }
            }
        }),
    );
    db::with_conn(&pool, |c| {
        let detail = db::endpoint_details::get(c, &outcome.endpoint.resource.node.id)?;
        assert_eq!(detail.auth.kind, AuthType::Bearer);
        assert_eq!(detail.auth.bearer.as_deref(), Some("abc"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_auth_scheme_without_its_credential_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "auth": { "kind": "bearer" }
            }
        }),
    );
    assert!(e.to_string().contains("bearer"), "{e}");
}

#[test]
fn a_payload_that_looks_like_a_schema_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "POST",
                "url": "/api/campaigns",
                "body": {
                    "mode": "json",
                    "content": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}"
                }
            }
        }),
    );
    assert!(e.to_string().contains("body_schema"), "{e}");
}

#[test]
fn a_body_with_both_a_payload_and_a_schema_is_accepted() {
    let (pool, _dir) = fresh_pool();
    let outcome = apply_ok(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "POST",
                "url": "/api/campaigns",
                "body": {
                    "mode": "json",
                    "content": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}",
                    "schema": "{\"type\":\"object\",\"properties\":{\"id\":{\"type\":\"string\"}}}"
                }
            }
        }),
    );
    db::with_conn(&pool, |c| {
        let detail = db::endpoint_details::get(c, &outcome.endpoint.resource.node.id)?;
        assert_eq!(detail.body.mode, BodyMode::Json);
        assert!(detail.body.schema.is_some());
        Ok(())
    })
    .unwrap();
}

// =========================================================================
// 必须报错的形状
// =========================================================================

#[test]
fn a_spec_for_a_project_that_does_not_exist_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_nope",
            "module": "营销",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("p_nope"), "{e}");
}

#[test]
fn giving_both_a_module_name_and_a_module_id_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "module_id": "m_order",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("二选一"), "{e}");
}

#[test]
fn giving_both_a_collection_path_and_a_collection_id_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "collection_path": "活动",
            "collection_id": "c_order_crud",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("二选一"), "{e}");
}

#[test]
fn a_module_that_belongs_to_another_project_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module_id": "m_title",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("m_title"), "{e}");
    assert!(e.to_string().contains("p_invoice"), "{e}");
}

#[test]
fn a_collection_that_belongs_to_another_project_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "collection_id": "c_title_crud",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("c_title_crud"), "{e}");
}

#[test]
fn an_environment_that_belongs_to_another_project_is_refused() {
    let (pool, _dir) = fresh_pool();
    // fixture 里的环境都属于 p_demo，所以先在另一个项目里造一个。
    let foreign_env_id = db::with_conn(&pool, |c| {
        let env = crate::domain::Environment {
            id: "env_invoice".to_string(),
            project_id: "p_invoice".to_string(),
            name: "dev".to_string(),
            module_base_urls: Default::default(),
            variables: Default::default(),
            disabled_variables: Vec::new(),
            is_active: false,
        };
        db::environments::create(c, &env)?;
        Ok(env.id)
    })
    .unwrap();

    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "environment": { "id": foreign_env_id },
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("env_invoice"), "{e}");
    assert!(e.to_string().contains("p_invoice"), "{e}");
}

#[test]
fn a_spec_without_a_module_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("module"), "{e}");
}

#[test]
fn an_unknown_field_in_the_spec_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "typo_field": "打错了字段名",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("typo_field"), "{e}");
}

#[test]
fn an_unknown_verb_is_refused() {
    let (pool, _dir) = fresh_pool();
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": { "name": "查询活动", "method": "不是动词", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("不是动词"), "{e}");
}

// =========================================================================
// 两种拼写
// =========================================================================

/// 多词字段两种拼写都收：`ifExists` 与 `pathParams` 与 snake_case 落在同一个类型上。
///
/// REST 路由与桌面 command 那条线通篇用 camelCase，MCP 那条线用 snake_case，而
/// `deny_unknown_fields` 会把少一个别名变成整份规格被拒。
#[test]
fn the_camel_case_spelling_of_multi_word_fields_is_accepted() {
    let (pool, _dir) = fresh_pool();
    let spec = json!({
        "projectId": "p_demo",
        "module": "营销",
        "ifExists": "duplicate",
        "endpoint": {
            "name": "查订单",
            "method": "GET",
            "url": "/api/orders/{id}",
            "pathParams": [{ "key": "id", "value": "42" }]
        }
    });

    let first = apply_ok(&pool, spec.clone());
    let second = apply_ok(&pool, spec);

    assert_eq!(
        first.endpoint.resource.detail.path_params.len(),
        1,
        "pathParams 必须落进路径参数那一份"
    );
    assert_eq!(first.endpoint.resource.detail.path_params[0].key, "id");
    assert_ne!(
        first.endpoint.resource.node.id, second.endpoint.resource.node.id,
        "ifExists=duplicate 每次都另建一个端点"
    );
}

// =========================================================================
// 跨模块与跨项目
// =========================================================================

/// 集合必须挂在规格指名的那个模块下。
///
/// base URL 住在 (环境 × 模块) 那一格：集合在别的模块下时，规格把那一格写进一个模块，
/// 端点却挂在另一个模块下，发送时读的是另一个格子。
#[test]
fn a_collection_under_another_module_is_refused() {
    let (pool, _dir) = fresh_pool();
    // fixture 里 `c_order_crud` 挂在 `m_order` 之下，而这份规格指名的是「支付」模块。
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "支付",
            "collection_id": "c_order_crud",
            "endpoint": { "name": "查询活动", "method": "GET", "url": "/api/campaigns" }
        }),
    );
    assert!(e.to_string().contains("c_order_crud"), "{e}");
    assert!(e.to_string().contains("m_order"), "{e}");
}

/// 端点自绑的环境必须属于规格指名的项目。
///
/// 环境里的变量属于那个项目，绑一个别的项目的环境等于让这个项目去读那个项目的值。
#[test]
fn an_endpoint_bound_to_another_projects_environment_is_refused() {
    let (pool, _dir) = fresh_pool();
    // fixture 里的环境都属于 p_demo，所以先在另一个项目里造一个。
    let foreign_env_id = db::with_conn(&pool, |c| {
        let env = crate::domain::Environment {
            id: "env_invoice".to_string(),
            project_id: "p_invoice".to_string(),
            name: "dev".to_string(),
            module_base_urls: Default::default(),
            variables: Default::default(),
            disabled_variables: Vec::new(),
            is_active: false,
        };
        db::environments::create(c, &env)?;
        Ok(env.id)
    })
    .unwrap();

    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "environment_id": foreign_env_id
            }
        }),
    );
    assert!(e.to_string().contains("env_invoice"), "{e}");
    assert!(e.to_string().contains("p_invoice"), "{e}");
}

// =========================================================================
// 落库的原子性
// =========================================================================

#[test]
fn a_spec_that_fails_halfway_leaves_nothing_behind() {
    let (pool, _dir) = fresh_pool();
    let count = |pool: &DbPool| -> usize {
        db::with_conn(pool, |c| {
            Ok(db::nodes::list_by_root(c, "p_demo").unwrap().len())
        })
        .unwrap()
    };
    let before = count(&pool);

    // 这份规格会先建模块与集合，再在端点那一段失败（认证方案没有凭据）。
    let e = apply(
        &pool,
        json!({
            "project_id": "p_demo",
            "module": "营销",
            "collection_path": "活动",
            "endpoint": {
                "name": "查询活动",
                "method": "GET",
                "url": "/api/campaigns",
                "auth": { "kind": "bearer" }
            }
        }),
    );
    assert!(e.to_string().contains("bearer"), "{e}");
    assert_eq!(
        count(&pool),
        before,
        "整份规格是一个事务：中途失败时，模块与集合都不该留在库里"
    );
}
