//! `endpoint_*` 回归测试共用的 fixture。
//!
//! 文件名不以 `_test.rs` 结尾，所以 `mcp/mod_test.rs` 的两个源码扫描也会读到它。
//! 这里既不过门，也不丢弃任何 `Result`，因此不会成为它们的违规者。

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use crate::mcp::AppState;
use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
use simple_post_core::domain::{
    EndpointDetail, HttpMethod, Project, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::permissions::Permission;
/// Per-call counter that disambiguates temp dirs when several tests
/// construct their state() in the same millisecond (cargo's default
/// runs tests in parallel).
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

pub(super) fn state() -> std::sync::Arc<AppState> {
    let id = STATE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "simple-post-mcp-endpoint-test-{}-{}-{}",
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

/// `Project` 没有 `Default`（每个字段都有意义），所以手工造出 fixture 所需的最小行。
pub(super) fn bare_project(id: &str, name: &str) -> Project {
    Project {
        id: id.into(),
        name: name.into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![],
        default_base_url_definition_id: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    }
}

pub(super) fn node(
    root: &str,
    id: &str,
    kind: ResourceNodeType,
    parent: Option<&str>,
) -> ResourceNode {
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

pub(super) fn bare_detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Get,
        url: "/x".into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: Default::default(),
        docs: None,
        notes: None,
        tags: vec![],
    }
}

/// 一个项目 + 模块 + 集合 + 端点，并且已授予 `Full` —— 好让改写类工具不会在到达被测
/// 行为之前就被 Gate B 拦下。
///
/// 返回端点 id。
pub(super) fn fixture() -> (std::sync::Arc<AppState>, String) {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_ep_fix", "P"))?;
        nodes::create(
            c,
            &node("p_ep_fix", "m_ep_fix", ResourceNodeType::Module, None),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_ep_fix",
                "c_ep_fix",
                ResourceNodeType::Collection,
                Some("m_ep_fix"),
            ),
            None,
        )?;
        nodes::create(
            c,
            &node(
                "p_ep_fix",
                "e_ep_fix",
                ResourceNodeType::Endpoint,
                Some("c_ep_fix"),
            ),
            Some(&bare_detail("e_ep_fix")),
        )?;
        // Gate B 默认是 ReadCall，低于更新所需的 Full —— 只能在项目行存在之后设置。
        // requires. `agent_permissions.project_id` is an FK to projects,
        // so this can only be set once the project row exists.
        permissions::set(c, "p_ep_fix", Permission::Full)?;
        Ok(())
    })
    .unwrap();
    (st, "e_ep_fix".to_string())
}

pub(super) fn detail_of(st: &AppState, id: &str) -> EndpointDetail {
    simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, id)).expect("detail")
}
