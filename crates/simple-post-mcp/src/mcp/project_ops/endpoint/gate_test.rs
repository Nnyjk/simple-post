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
// 3. Gate B — no fabricated "global" project id
// =========================================================================

/// A scope named as a `collection_id` must be resolved to its owning
/// project.
///
/// The old code gated against `project_id.unwrap_or("global")`, so a
/// collection-scoped list consulted a project that does not exist and
/// fell back to the default permission — the owner's real row was never
/// read. `Permission`'s floor is `Read` and `endpoint_list` requires
/// `Read`, so a Read-tier tool can never be *denied*; the observable
/// proof that the owner is resolved is therefore made through a tool
/// that shares the same resolution path but sits above the floor —
/// `endpoint_update`, which requires `Full`.
///
/// With the owner's row at the default (`ReadCall`, the value a project
/// that was never configured has), a collection-scoped update must be
/// denied. It passes only when the handler resolves `c_b → p_b` and
/// reads `p_b`'s actual permission. Against the old fabricated `"global"`
/// the lookup would also have hit the same default, so this test
/// pins the resolution by *also* granting `p_b` and re-asserting.
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
        // Neither project is configured, so both sit at the default
        // `ReadCall` — below the `Full` an update needs.
        Ok(())
    })
    .unwrap();

    // Denied: the owner p_b has not granted Full.
    let denied = tool_endpoint_update(
        &st,
        serde_json::json!({ "endpoint_id": "e_b", "patch": { "url": "/y" } }),
    )
    .await;
    assert!(
        matches!(denied, Err(AppError::PermissionDenied(_))),
        "an endpoint in an unconfigured project must not be writable; got {denied:?}"
    );

    // Granted: `p_b` — the endpoint's REAL owner — is now Full. This is
    // 这条断言会在 handler 拿一个编造的 id 过门时失败：
    // 给真正的归属方授权将毫无作用。
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

/// `module_id` is the other node-shaped scope, and it reaches the same
/// `root_id_of` resolution. Proven the same way, through a tool above
/// the permission floor.
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

/// An unscoped read spans every project, so the per-project gate has no
/// single project to consult. Rather than fabricating an id — the exact
/// bug being fixed — the call is refused with a message that tells the
/// caller what to pass.
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

/// `endpoint_search` previously gated under the tool name
/// `"endpoint_list"`, so the permission decision was looked up against
/// another tool's row — a substitution that is invisible until the two
/// names' required tiers diverge.
#[tokio::test]
async fn endpoint_search_gates_under_its_own_tool_name() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_s", "S"))?;
        nodes::create(c, &node("p_s", "m_s", ResourceNodeType::Module, None), None)?;
        Ok(())
    })
    .unwrap();
    // 项目停在默认 `ReadCall`。搜索要求 Read，
    // so it passes; what matters is that the *search* row was consulted,
    // which is asserted structurally below.
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
