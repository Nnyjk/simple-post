//! `endpoint_*` 工具回归测试 —— 由 `endpoint.rs` 以
//! `#[cfg(test)] #[path = "endpoint_test.rs"] mod tests;` 挂载。
//!
//! 下面三组各自守住一类「以**成功**响应收场」的缺陷：
//!
//! 1. **畸形 patch 被吞掉。** 每个 patch 字段都用
//!    `.and_then(|v| serde_json::from_value(v).ok())` 读，于是反序列化失败变成
//!    「这一列别动」。agent 发来 `{"params":[{"k":"a","v":"1"}]}`（字段名写错）会拿到
//!    `is_error: false` 且什么都没写，且无法与真正生效的 patch 区分。这些测试断言的是
//!    相反的行为：字段畸形必须是点名该字段与期望形状的 `Validation` 错误。
//!
//! 2. **搭建接口够不到或写不进。** patch 里 `docs` / `notes` 被写死成 `None`，任何工具
//!    都写不了它们；`environment_id` / `base_url_definition_id` / `base_url_override`
//!    只能经同一个有损读取器设置。这些测试把整张配置面走一遍，逐个断言字段真的落库。
//!
//! 3. **`endpoint_list` / `endpoint_search` 曾拿字面量 `"global"` 过门。** `"global"`
//!    不是项目 id，按项目的那次查询找不到行、退回默认档位；而以 `collection_id` /
//!    `module_id` 给出的范围根本没被解析成它的归属，真实项目的权限因此一次也没被查过。

use super::*;
use crate::mcp::AppState;
use simple_post_core::db::{endpoint_details, nodes, permissions, projects};
use simple_post_core::domain::{
    BodyMode, EndpointDetail, HttpMethod, OAuth2ClientAuth, OAuth2GrantType, OAuth2TokenLocation,
    Project, RequestBody, ResourceNode, ResourceNodeType,
};
use simple_post_core::error::AppError;
use simple_post_core::permissions::Permission;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Per-call counter that disambiguates temp dirs when several tests
/// construct their state() in the same millisecond (cargo's default
/// runs tests in parallel).
static STATE_COUNTER: AtomicU32 = AtomicU32::new(0);

fn state() -> std::sync::Arc<AppState> {
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
fn bare_project(id: &str, name: &str) -> Project {
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

fn bare_detail(node_id: &str) -> EndpointDetail {
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
fn fixture() -> (std::sync::Arc<AppState>, String) {
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

fn detail_of(st: &AppState, id: &str) -> EndpointDetail {
    simple_post_core::db::with_conn(&st.pool, |c| endpoint_details::get(c, id)).expect("detail")
}

// =========================================================================
// 1. 畸形 patch 必须是响亮的错误，而不是静默的无操作
// =========================================================================

/// The headline defect. `{"k":"a","v":"1"}` is what an LLM writes first;
/// under the old `.ok()` reader it deserialised to `None`, the patch
/// serialised to an empty object, and the call returned a **success**
/// with nothing written.
#[tokio::test]
async fn endpoint_update_rejects_malformed_params_instead_of_ignoring_it() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "params": [{ "k": "a", "v": "1" }] }
        }),
    )
    .await;

    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    // 消息必须点名字段，否则 agent 分不清十五个 patch 字段里错的是哪一个。
    // of its fifteen patch fields was wrong.
    assert!(
        msg.contains("'params'"),
        "error must name the offending field, got: {msg}"
    );
    // ……并且说清期望形状，这样不必去读工具的 Rust 源码就能重试。
    // reading the tool's Rust source.
    assert!(
        msg.contains("key") && msg.contains("value"),
        "error must state the expected item shape, got: {msg}"
    );
}

/// A wrong-typed field (a string where an array belongs) must fail the
/// same way. Both were `.ok()`-swallowed before.
#[tokio::test]
async fn endpoint_update_rejects_wrong_typed_params() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "params": "a=1" }
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("'params'"), "got: {msg}");
}

/// `method` used `.and_then(|m| parse_http_method(m).ok())` — a second
/// independent swallow on the same patch object.
#[tokio::test]
async fn endpoint_update_rejects_a_non_verb_method() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "method": "GET POST" }
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("'method'"), "got: {msg}");
}

/// An unknown `body.mode` and an unknown `auth.kind` must both be
/// errors. These go through serde, which reports the variant list.
#[tokio::test]
async fn endpoint_update_rejects_an_unknown_body_mode() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "body": { "mode": "xml", "content": "<a/>" } }
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("'body'"), "got: {msg}");
}

/// An unknown `auth.kind` must be an error. These go through serde,
/// which reports the variant list.
///
/// The payload is a name that never existed rather than `oauth2`: that
/// one is a supported kind since #2, so reusing it here would leave the
/// assertion testing nothing.
#[tokio::test]
async fn endpoint_update_rejects_an_unknown_auth_kind() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "auth": { "kind": "bearerish" } }
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("'auth'"), "got: {msg}");
}

/// An `oauth2` payload with a malformed field aborts the **whole**
/// patch. `grantType` goes through serde, so an invented flow must be
/// reported rather than quietly defaulting to `authorization_code` and
/// then failing against a real IdP much later.
#[tokio::test]
async fn endpoint_update_rejects_a_malformed_oauth2_payload() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "auth": {
                    "kind": "oauth2",
                    "oauth2": { "grantType": "teleport", "tokenUrl": "https://idp.example.com/token" }
                }
            }
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("teleport"), "got: {msg}");
    assert_eq!(
        detail_of(&st, &id).auth.kind,
        simple_post_core::domain::AuthType::None,
        "a rejected auth must leave the stored one untouched"
    );
}

/// The acceptance path: an agent writes a full oauth2 config and it
/// lands in the stored row, not merely in the echoed response.
#[tokio::test]
async fn an_agent_can_store_a_complete_oauth2_config() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "auth": {
                    "kind": "oauth2",
                    "oauth2": {
                        "grantType": "client_credentials",
                        "tokenUrl": "https://idp.example.com/oauth/token",
                        "clientId": "cid",
                        "clientSecret": "csecret",
                        "clientAuth": "body",
                        "scope": "read write",
                        "extraParams": [
                            { "id": "kv1", "key": "audience", "value": "https://api.example.com", "enabled": true }
                        ],
                        "tokenLocation": "query",
                        "tokenParam": "jwt"
                    }
                }
            }
        }),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let stored = detail_of(&st, &id).auth;
    assert_eq!(stored.kind, simple_post_core::domain::AuthType::Oauth2);
    let o = stored.oauth2.expect("oauth2 payload");
    assert_eq!(o.grant_type, OAuth2GrantType::ClientCredentials);
    assert_eq!(o.token_url, "https://idp.example.com/oauth/token");
    assert_eq!(o.client_id, "cid");
    assert_eq!(o.client_secret, "csecret");
    assert_eq!(o.client_auth, OAuth2ClientAuth::Body);
    assert_eq!(o.scope, "read write");
    assert_eq!(o.token_location, OAuth2TokenLocation::Query);
    assert_eq!(o.token_param, "jwt");
    assert_eq!(o.extra_params.len(), 1);
    assert_eq!(o.extra_params[0].key, "audience");
}

/// `endpoint_create` 的 `auth_type` 也得认识 oauth2 —— 它走的是另一条解析路径
/// (`build_auth` 的字符串匹配)，漏掉它会让 create 与 update 对同一个鉴权方案
/// 给出不同的答案。
#[tokio::test]
async fn endpoint_create_accepts_the_oauth2_auth_type() {
    let (st, _existing) = fixture();
    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "project_id": "p_ep_fix",
            "module_id": "m_ep_fix",
            "collection_id": "c_ep_fix",
            "name": "oauth ep",
            "url": "/x",
            "method": "GET",
            "auth_type": "oauth2",
            "auth_config": {
                "oauth2": {
                    "grantType": "client_credentials",
                    "tokenUrl": "https://idp.example.com/oauth/token",
                    "clientId": "cid"
                }
            }
        }),
    )
    .await;
    let created = res.expect("create 应成功");
    // `CallToolResult` 的载荷包在 text content 里，故从序列化后的形状取回 id。
    let envelope = serde_json::to_value(&created).unwrap();
    let text = envelope["content"][0]["text"]
        .as_str()
        .expect("text content");
    let body: serde_json::Value = serde_json::from_str(text).expect("content 是 JSON");
    let id = body["id"].as_str().expect("create 回显里带 id").to_string();
    assert_eq!(
        detail_of(&st, &id).auth.kind,
        simple_post_core::domain::AuthType::Oauth2,
        "auth_type=oauth2 必须真的落到存储行，不能只出现在回显里"
    );
}

/// 一个 `auth_type` 未知的请求，错误信息要列出全部合法取值 —— agent 据此自我纠正，
/// 而不是去猜大小写或拼写。
#[tokio::test]
async fn endpoint_create_rejects_an_unknown_auth_type_and_lists_the_known_ones() {
    let (st, _existing) = fixture();
    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "project_id": "p_ep_fix",
            "module_id": "m_ep_fix",
            "collection_id": "c_ep_fix",
            "name": "bad ep",
            "url": "/x",
            "method": "GET",
            "auth_type": "oauth-2"
        }),
    )
    .await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(msg.contains("oauth2"), "got: {msg}");
}

/// A malformed field must abort the **whole** patch, including the
/// well-formed fields alongside it. Otherwise the agent sees a success,
/// the valid half is written, and the invalid half is silently dropped —
/// the same invisibility, one level down.
#[tokio::test]
async fn a_malformed_field_aborts_the_whole_patch() {
    let (st, id) = fixture();
    let res = tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "url": "/should-not-be-written",
                "params": [{ "k": "a", "v": "1" }]
            }
        }),
    )
    .await;
    assert!(res.is_err(), "a malformed field must fail the call");
    let detail = detail_of(&st, &id);
    assert_eq!(
        detail.url, "/x",
        "the well-formed `url` in the same patch must not have been \
         written — a partial patch is exactly the silent failure this \
         fix removes"
    );
}

/// An empty patch used to be a silent success that wrote nothing and
/// returned the unchanged endpoint, which reads as "it worked".
#[tokio::test]
async fn endpoint_update_rejects_an_empty_patch() {
    let (st, id) = fixture();
    let res =
        tool_endpoint_update(&st, serde_json::json!({ "endpoint_id": id, "patch": {} })).await;
    let Err(AppError::Validation(msg)) = res else {
        panic!("expected a Validation error, got {res:?}");
    };
    assert!(
        msg.contains("empty patch"),
        "the error should say the patch was empty, got: {msg}"
    );
}

// =========================================================================
// 2. The "接口参数" build surface must be complete and writable
// =========================================================================

/// The acceptance test for the whole point of the change: one agent can
/// build an endpoint's complete request configuration through
/// `endpoint_create` + `endpoint_update`, and every field is verified in
/// the stored row rather than in the echoed response.
#[tokio::test]
async fn an_agent_can_build_every_piece_of_endpoint_request_config() {
    let (st, id) = fixture();

    // --- via endpoint_update: the full configuration surface ---------
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "method": "POST",
                "url": "/api/users/{id}",
                "params": [
                    { "key": "page", "value": "1" },
                    { "key": "size", "value": "20", "enabled": false }
                ],
                "headers": [
                    { "key": "Content-Type", "value": "application/json" },
                    { "key": "X-Trace", "value": "abc" }
                ],
                "body": {
                    "mode": "json",
                    "content": "{\"name\":\"ada\"}",
                    "schema": "{\"name\":\"string\"}"
                },
                "auth": {
                    "kind": "bearer",
                    "bearer": "tok-123"
                },
                "tags": ["smoke", "user"],
                "environment_id": "env_dev",
                "base_url_definition_id": "svc_api",
                "base_url_override": "https://staging.example.com",
                "docs": "Creates a user.",
                "notes": "checked by hand",
                "name": "create user"
            }
        }),
    )
    .await
    .expect("a well-formed full patch must be accepted");

    let d = detail_of(&st, &id);

    assert_eq!(d.method, HttpMethod::Post);
    assert_eq!(d.url, "/api/users/{id}");

    // Query 参数。这些行没写 `id`/`enabled`，正是 agent 会写出的形状 ——
    // two fields `KeyValue` requires and an agent has no reason to know
    // 转换器补铸前者、把后者默认成
    // `true`（存成禁用的请求头就是一个永远发不出去的请求头）。
    assert_eq!(d.params.len(), 2);
    let page = d.params.iter().find(|kv| kv.key == "page").expect("page");
    assert_eq!(page.value, "1");
    assert!(page.enabled, "an omitted `enabled` must default to true");
    assert!(!page.id.is_empty(), "a row id must be minted");
    let size = d.params.iter().find(|kv| kv.key == "size").expect("size");
    assert!(
        !size.enabled,
        "an explicit `enabled: false` must still be honoured"
    );

    assert_eq!(d.headers.len(), 2);
    assert!(d.headers.iter().all(|kv| kv.enabled));
    assert!(d
        .headers
        .iter()
        .any(|kv| kv.key == "Content-Type" && kv.value == "application/json"));

    // Body：mode、content、schema 三项都要写，不能只前两个。
    // `endpoint_create` could reach.
    assert_eq!(d.body.mode, BodyMode::Json);
    assert_eq!(d.body.content, "{\"name\":\"ada\"}");
    assert_eq!(d.body.schema.as_deref(), Some("{\"name\":\"string\"}"));

    // Auth: type + config.
    assert_eq!(
        d.auth.kind,
        simple_post_core::domain::AuthType::Bearer,
        "auth type must be settable"
    );
    assert_eq!(d.auth.bearer.as_deref(), Some("tok-123"));

    assert_eq!(d.tags, vec!["smoke".to_string(), "user".to_string()]);

    // Environment binding + base-url definition binding.
    assert_eq!(d.environment_id.as_deref(), Some("env_dev"));
    assert_eq!(d.base_url_definition_id.as_deref(), Some("svc_api"));
    assert_eq!(
        d.base_url_override.as_deref(),
        Some("https://staging.example.com")
    );

    // docs / notes 在旧 patch 构造器里被写死成 `None`，于是没有任何工具写得动它们。

    assert_eq!(d.docs.as_deref(), Some("Creates a user."));
    assert_eq!(d.notes.as_deref(), Some("checked by hand"));

    let n = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, &id)).expect("node");
    assert_eq!(n.name, "create user");
}

/// `Some(None)` must keep meaning "clear" for the `double_option`
/// fields. A patch that could only ever *set* would make
/// `environment_id` / `base_url_override` / `notes` one-way.
#[tokio::test]
async fn an_explicit_null_clears_a_nullable_patch_field() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "environment_id": "env_dev", "notes": "first" }
        }),
    )
    .await
    .expect("setting the fields");
    assert_eq!(
        detail_of(&st, &id).environment_id.as_deref(),
        Some("env_dev")
    );

    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "environment_id": null, "notes": null }
        }),
    )
    .await
    .expect("clearing the fields must be a supported write, not an error");

    let d = detail_of(&st, &id);
    assert_eq!(d.environment_id, None, "null must clear, not be rejected");
    assert_eq!(d.notes, None);
}

/// An omitted field must still mean "leave unchanged" — the property
/// that made `.ok()` so tempting in the first place.
#[tokio::test]
async fn an_omitted_field_is_left_unchanged() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": { "url": "/v2/only-the-url" }
        }),
    )
    .await
    .expect("patch must apply");
    let d = detail_of(&st, &id);
    assert_eq!(d.url, "/v2/only-the-url");
    assert_eq!(d.method, HttpMethod::Get, "method must be untouched");
    assert!(d.params.is_empty(), "params must be untouched");
}

/// A round-trip through `endpoint_get` must preserve the client-supplied
/// `id`, so an agent can read-modify-write a header list without
/// orphaning rows.
#[tokio::test]
async fn a_supplied_row_id_is_preserved() {
    let (st, id) = fixture();
    tool_endpoint_update(
        &st,
        serde_json::json!({
            "endpoint_id": id,
            "patch": {
                "headers": [{ "key": "A", "value": "1", "id": "row-stable-id" }]
            }
        }),
    )
    .await
    .expect("patch must apply");
    assert_eq!(detail_of(&st, &id).headers[0].id, "row-stable-id");
}

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

// =========================================================================
// 4. Endpoint id uniqueness
// =========================================================================

/// `endpoint_create` minted `format!("ep_{now}")` from a **millisecond**
/// clock, so two creates in the same millisecond collided on the primary
/// key. Minting a batch back-to-back is the reliable way to hit it.
#[tokio::test]
async fn two_endpoint_creates_in_the_same_millisecond_get_distinct_ids() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_id", "I"))?;
        nodes::create(
            c,
            &node("p_id", "m_id", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_id", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let mut ids = Vec::new();
    for i in 0..8 {
        let res = tool_endpoint_create(
            &st,
            serde_json::json!({
                "module_id": "m_id",
                "name": format!("e{i}"),
                "method": "GET",
                "url": "/x"
            }),
        )
        .await
        .unwrap_or_else(|e| panic!("create #{i} must succeed, got {e}"));
        let crate::mcp::CallToolContent::Text { text } = &res.content[0];
        let created: serde_json::Value = serde_json::from_str(text).expect("json result");
        ids.push(created["id"].as_str().expect("id").to_string());
    }

    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "every create must get its own id; got {ids:?}"
    );
    // `ep_` 前缀是同级创建工具遵循的约定的一部分，所以放宽 id 也不能丢掉它。

    assert!(
        ids.iter().all(|id| id.starts_with("ep_")),
        "endpoint ids must keep the ep_ prefix; got {ids:?}"
    );
}

// =========================================================================
// 5. `endpoint_create` 一次调用即可接受完整配置
// =========================================================================

/// `endpoint_create` previously hardcoded `environment_id`,
/// `base_url_definition_id`, `base_url_override`, `body_schema`, `docs`,
/// `notes` and `description` to their defaults, so building a fully
/// configured endpoint took a mandatory second `endpoint_update` call.
#[tokio::test]
async fn endpoint_create_accepts_the_full_configuration_in_one_call() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_c", "C"))?;
        nodes::create(c, &node("p_c", "m_c", ResourceNodeType::Module, None), None)?;
        permissions::set(c, "p_c", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_c",
            "name": "list orders",
            "description": "Orders for the current user",
            "method": "POST",
            "url": "/api/orders",
            "params": [{ "key": "page", "value": "1" }],
            "headers": [{ "key": "Accept", "value": "application/json" }],
            "body_mode": "json",
            "body_content": "{\"limit\":10}",
            "body_schema": "{\"limit\":\"number\"}",
            "auth_type": "bearer",
            "auth_config": { "bearer": "tok" },
            "tags": ["orders"],
            "environment_id": "env_dev",
            "base_url_definition_id": "svc_api",
            "base_url_override": "https://api.example.com",
            "docs": "Lists orders.",
            "notes": "n"
        }),
    )
    .await
    .expect("a fully specified create must succeed");

    let crate::mcp::CallToolContent::Text { text } = &res.content[0];
    let created: serde_json::Value = serde_json::from_str(text).expect("json result");
    let id = created["id"].as_str().expect("id").to_string();

    let d = detail_of(&st, &id);
    assert_eq!(d.method, HttpMethod::Post);
    assert_eq!(d.params.len(), 1);
    assert!(d.params[0].enabled);
    assert_eq!(d.headers.len(), 1);
    assert_eq!(d.body.mode, BodyMode::Json);
    assert_eq!(d.body.schema.as_deref(), Some("{\"limit\":\"number\"}"));
    assert_eq!(d.auth.kind, simple_post_core::domain::AuthType::Bearer);
    assert_eq!(d.auth.bearer.as_deref(), Some("tok"));
    assert_eq!(d.environment_id.as_deref(), Some("env_dev"));
    assert_eq!(d.base_url_definition_id.as_deref(), Some("svc_api"));
    assert_eq!(
        d.base_url_override.as_deref(),
        Some("https://api.example.com")
    );
    assert_eq!(d.docs.as_deref(), Some("Lists orders."));
    assert_eq!(d.notes.as_deref(), Some("n"));

    let n = simple_post_core::db::with_conn(&st.pool, |c| nodes::get(c, &id)).expect("node");
    assert_eq!(
        n.description.as_deref(),
        Some("Orders for the current user")
    );
}

/// A typo'd `auth_config` used to be swallowed into an all-default
/// `AuthConfig` via `unwrap_or_default()`, so the create succeeded and
/// described an endpoint whose bearer token did not exist.
#[tokio::test]
async fn endpoint_create_rejects_a_malformed_auth_config() {
    let st = state();
    simple_post_core::db::with_tx(&st.pool, |c| {
        projects::create(c, &bare_project("p_ac", "AC"))?;
        nodes::create(
            c,
            &node("p_ac", "m_ac", ResourceNodeType::Module, None),
            None,
        )?;
        permissions::set(c, "p_ac", Permission::Full)?;
        Ok(())
    })
    .unwrap();

    let res = tool_endpoint_create(
        &st,
        serde_json::json!({
            "module_id": "m_ac",
            "name": "n",
            "method": "GET",
            "url": "/x",
            "auth_type": "bearer",
            "auth_config": { "bearer": { "tok": 1 } }
        }),
    )
    .await;
    assert!(
        matches!(res, Err(AppError::Validation(_))),
        "a malformed auth_config must be rejected, got {res:?}"
    );
}

// =========================================================================
// 6. The JSON Schema is self-describing
// =========================================================================

/// An agent reading only `tools/list` cannot guess a nested shape. The
/// schema is the contract it has, so the item shape and the enum values
/// have to be in it.
#[test]
fn endpoint_create_schema_documents_the_item_shape_and_enums() {
    let descriptor = crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == "endpoint_create")
        .map(|e| e.schema.clone())
        .expect("endpoint_create must be registered");
    let props = &descriptor["properties"];

    // `params` / `headers` share one `$defs` entry, so the item shape is
    // 完整写出再被引用。关键在于它**可解析** —— 跟着 `$ref` 走的 agent 必须落到
    // 一个真实携带字段名的定义上，而不是一个光秃秃的
    // `{"type":"object"}`。
    let params_ref = props["params"]["items"]["$ref"]
        .as_str()
        .expect("params items must $ref the keyValue definition");
    assert_eq!(params_ref, "#/$defs/keyValue");
    assert_eq!(
        props["headers"]["items"]["$ref"].as_str(),
        Some(params_ref),
        "headers must share the params item definition"
    );

    let defs = &descriptor["$defs"];
    let item_props = &defs["keyValue"]["properties"];
    assert_eq!(
        item_props["key"]["type"], "string",
        "the KeyValue item shape must be spelled out, not a bare object"
    );
    assert_eq!(item_props["value"]["type"], "string");
    assert_eq!(item_props["enabled"]["type"], "boolean");
    assert_eq!(
        defs["keyValue"]["required"],
        serde_json::json!(["key", "value"])
    );

    // 枚举必须与解析器实际接受的完全一致。
    // derived from `domain::BodyMode` / `domain::AuthType`.
    assert_eq!(
        props["body_mode"]["enum"],
        serde_json::json!(["none", "json", "form", "raw"])
    );
    // 与解析器实际接受的那一份常量比对，而不是在这里再抄一遍：抄一份就多一个
    // 「加了变体却忘了改这里」的地方，而那种不一致对 agent 是静默的。
    assert_eq!(
        props["auth_type"]["enum"],
        serde_json::json!(AUTH_TYPE_VALUES)
    );
    // schema 声明的每个取值都必须能解析。
    for mode in props["body_mode"]["enum"].as_array().expect("enum") {
        let s = mode.as_str().expect("string");
        assert!(
            parse_body_mode(s).is_ok(),
            "schema advertises body_mode {s:?} but the parser rejects it"
        );
    }
}

/// `endpoint_update` is the tool an agent uses to build interface
/// parameters, so its patch object must not be an untyped `{"type":
/// "object"}` either.
#[test]
fn endpoint_update_schema_documents_every_patchable_field() {
    let descriptor = crate::mcp::tool_table()
        .iter()
        .find(|e| e.name == "endpoint_update")
        .map(|e| e.schema.clone())
        .expect("endpoint_update must be registered");
    let patch_props = &descriptor["properties"]["patch"]["properties"];

    // Every field `tool_endpoint_update` accepts, and no field it does
    // —— 两份列表一旦漂移，schema 就是在撒谎。
    let mut documented: Vec<&str> = patch_props
        .as_object()
        .expect("patch properties")
        .keys()
        .map(String::as_str)
        .collect();
    documented.sort_unstable();
    assert_eq!(
        documented,
        vec![
            "auth",
            "base_url_definition_id",
            "base_url_override",
            "body",
            "description",
            "docs",
            "environment_id",
            "headers",
            "method",
            "name",
            "notes",
            "params",
            "sort_order",
            "tags",
            "url",
        ],
        "the documented patch fields must match the fields the handler reads"
    );

    assert_eq!(
        patch_props["body"]["properties"]["mode"]["enum"],
        serde_json::json!(["none", "json", "form", "raw"])
    );
    assert_eq!(
        patch_props["auth"]["properties"]["kind"]["enum"],
        serde_json::json!(AUTH_TYPE_VALUES)
    );
    // oauth2 的载荷必须登记进 schema。`auth` 上有 `additionalProperties: false`，
    // 漏掉它不是「宽松一点」，而是 agent 根本传不进来——一个看着正常却永远收不到
    // 配置的端点。
    let oauth2 = &patch_props["auth"]["properties"]["oauth2"];
    assert_eq!(oauth2["type"], "object");
    assert_eq!(
        oauth2["properties"]["grantType"]["enum"],
        serde_json::json!([
            "authorization_code",
            "client_credentials",
            "password",
            "implicit"
        ])
    );
    for field in [
        "tokenUrl",
        "clientId",
        "clientSecret",
        "clientAuth",
        "scope",
        "authorizationUrl",
        "redirectUri",
        "codeVerifier",
        "authCode",
        "username",
        "password",
        "extraParams",
        "tokenLocation",
        "tokenParam",
    ] {
        assert!(
            oauth2["properties"].get(field).is_some(),
            "oauth2 载荷漏登记了 `{field}`"
        );
    }
    assert_eq!(oauth2["additionalProperties"], false);
    // 可空字段必须写明 `null` 表示清空 ——
    // three-state meaning is invisible on the wire otherwise.
    assert_eq!(patch_props["environment_id"]["type"][1], "null");
    assert_eq!(patch_props["base_url_override"]["type"][1], "null");
}
