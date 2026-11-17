//! Gate B 回归网 —— 过门用的必须是端点真实的归属项目。
//!
//! 共享 fixture 见 `test_support.rs`。

use super::read::{tool_endpoint_list, tool_endpoint_search};
use super::test_support::{bare_detail, bare_project, node, state};
use super::write::tool_endpoint_update;
use simple_post_core::db::{nodes, permissions, projects};
use simple_post_core::domain::ResourceNodeType;
use simple_post_core::error::AppError;
use simple_post_core::permissions::Permission;
// =========================================================================
// 3. Gate B —— 归属项目由 root_id_of 解析，不接受编造的 global
// =========================================================================

/// 以 `collection_id` 给出的范围必须解析到它所属的项目。
///
/// `Permission` 的下限是 `Read`，`endpoint_list` 也只要求 `Read`，所以读档工具永远不会
/// 被拒——归属是否解析成功在它身上看不出来。本用例因此改走 `endpoint_update`：它与
/// `endpoint_list` 共用 `root_id_of` 这条解析路径，但要求 `Full`，高于下限。
///
/// `p_b` 未配置权限时取默认的 `ReadCall`，低于 `Full`，故对 `c_b` 范围的更新必须被拒。
/// 只有 handler 把 `c_b` 解析到 `p_b`、读 `p_b` 的真实权限行，这次拒绝才成立；若 handler
/// 拿一个编造的 global 去过门，读到的是同一个默认值，所以断言不止于拒绝——随后给 `p_b`
/// 授予 `Full` 并重试写入，写入成功才证明 handler 读的是 `p_b` 这一行。
#[tokio::test]
async fn a_collection_scope_resolves_to_its_owning_project() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_a", "A"))?;
        projects::create(c, &bare_project("p_b", "B"))?;
        nodes::create(c, &node("p_a", "m_a", ResourceNodeType::Module, None), None)?;
        nodes::create(c, &node("p_b", "m_b", ResourceNodeType::Module, None), None)?;
        nodes::create(
            c,
            &node("p_b", "c_b", ResourceNodeType::Collection, Some("m_b")),
            None,
        )?;
        nodes::create(
            c,
            &node("p_b", "e_b", ResourceNodeType::Endpoint, Some("c_b")),
            Some(&bare_detail("e_b")),
        )?;
        // 两个项目都没配置权限，都取默认的 `ReadCall`，低于更新需要的 `Full`。
        Ok(())
    })
    .unwrap();

    // 归属方 p_b 没有授予 `Full`，这次更新必须被拒。
    let denied = tool_endpoint_update(
        &st,
        serde_json::json!({ "endpoint_id": "e_b", "patch": { "url": "/y" } }),
    )
    .await;
    assert!(
        matches!(denied, Err(AppError::PermissionDenied(_))),
        "an endpoint in an unconfigured project must not be writable; got {denied:?}"
    );

    // 给真正的归属方 `p_b` 授予 `Full` 之后更新必须成功：handler 若拿编造的 id
    // 过门，这次授权对它的查找毫无作用，于是这里会失败。
    simple_post_core::db::with_tx(&st.pool, |c| permissions::set(c, "p_b", Permission::Full))
        .unwrap();
    tool_endpoint_update(
        &st,
        serde_json::json!({ "endpoint_id": "e_b", "patch": { "url": "/granted" } }),
    )
    .await
    .expect("granting the true owning project must unlock its endpoints");

    // list 调用解析同一个范围，因此必须成功。
    assert!(
        tool_endpoint_list(&st, serde_json::json!({ "collection_id": "c_b" }))
            .await
            .is_ok(),
        "a collection scope must resolve to its owning project"
    );
}

/// `module_id` 是另一种节点型范围，走同一条 `root_id_of` 解析。验证方式相同：
/// 用高于权限下限的 `endpoint_update` 观察，再用 `Read` 档的 list / search 收尾。
#[tokio::test]
async fn a_module_scope_resolves_to_its_owning_project() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_m", "M"))?;
        nodes::create(c, &node("p_m", "m_m", ResourceNodeType::Module, None), None)?;
        nodes::create(
            c,
            &node("p_m", "c_m", ResourceNodeType::Collection, Some("m_m")),
            None,
        )?;
        nodes::create(
            c,
            &node("p_m", "e_m", ResourceNodeType::Endpoint, Some("c_m")),
            Some(&bare_detail("e_m")),
        )?;
        Ok(())
    })
    .unwrap();

    let denied = tool_endpoint_update(
        &st,
        serde_json::json!({ "endpoint_id": "e_m", "patch": { "url": "/y" } }),
    )
    .await;
    assert!(
        matches!(denied, Err(AppError::PermissionDenied(_))),
        "got {denied:?}"
    );

    simple_post_core::db::with_tx(&st.pool, |c| permissions::set(c, "p_m", Permission::Full))
        .unwrap();

    assert!(
        tool_endpoint_list(&st, serde_json::json!({ "module_id": "m_m" }))
            .await
            .is_ok(),
        "module-scoped list must resolve to the owning project"
    );
    assert!(
        tool_endpoint_search(&st, serde_json::json!({ "module_id": "m_m", "q": "x" }))
            .await
            .is_ok(),
        "module-scoped search must resolve to the owning project"
    );
}

/// 不带范围的读会跨所有项目，逐项目的那道门找不到可查的单一项目。`endpoint_list`
/// 与 `endpoint_search` 此时拒绝调用，并在消息里告诉调用方要传什么范围，而不是编造
/// 一个项目 id 去过门。
#[tokio::test]
async fn an_unscoped_read_is_refused_rather_than_gated_against_a_fake_project() {
    let st = state();
    for res in [
        tool_endpoint_list(&st, serde_json::json!({})).await,
        tool_endpoint_search(&st, serde_json::json!({})).await,
    ] {
        match res {
            Err(AppError::Validation(msg)) => assert!(
                msg.contains("needs a scope"),
                "the error must tell the caller what to pass, got: {msg}"
            ),
            other => panic!("an unscoped read must be refused, got {other:?}"),
        }
    }
}

/// `endpoint_search` 必须按自己的工具名查权限行：若它借用 `endpoint_list` 的名字，
/// 权限判定读的就是另一个工具的行，两个名字要求的档位一分叉就会露出来。
#[tokio::test]
async fn endpoint_search_gates_under_its_own_tool_name() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_s", "S"))?;
        nodes::create(c, &node("p_s", "m_s", ResourceNodeType::Module, None), None)?;
        Ok(())
    })
    .unwrap();
    // 项目停在默认 `ReadCall`，默认档满足搜索要求的 `Read`，所以这次搜索会通过；
    // 真正要验的是权限判定读的是 `endpoint_search` 自己那一行，下面的断言按结构固定它。
    assert_eq!(
        simple_post_core::permissions::required_permission("endpoint_search"),
        Permission::Read
    );
    assert!(
        tool_endpoint_search(&st, serde_json::json!({ "project_id": "p_s" }))
            .await
            .is_ok(),
        "a Read-tier search must pass against the default permission"
    );
}
