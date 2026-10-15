//! 畸形 patch 必须是响亮的错误，而不是静默的无操作。
//!
//! 与「接口参数」那一组共用同一套 fixture，见 `test_support.rs`。

use super::test_support::{detail_of, fixture};
use super::write::{tool_endpoint_create, tool_endpoint_update};
use simple_post_core::domain::{OAuth2ClientAuth, OAuth2GrantType, OAuth2TokenLocation};
use simple_post_core::error::AppError;
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
