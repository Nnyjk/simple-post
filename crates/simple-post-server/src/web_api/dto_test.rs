//! `POST /api/nodes` body 契约的回归网。
//!
//! 这些测试把宽容的形状钉在 DTO 边界上：不碰池、不碰 router、不碰 HTTP——body 必须
//! 能解析，半填的 endpoint 必须能展开成一行带真实 method 的记录。
//!
//! 关键点：`detail` 的类型是 `CreateEndpointDetailOpts`（字段全可选），不是
//! `EndpointDetail`。树的「新建请求」动作发的正是 `detail: { url }`，而
//! `EndpointDetail` 的 `method` 没有 `#[serde(default)]`，按它解析会把同一次点击变成
//! 「桌面端建出 endpoint、`serve-web` 回 400」。同理 `rootId` 允许 `null`（"让服务端
//! 自己推"），因为存储行类型 `ResourceNode.root_id: String` 与 create 调用所知的不是
//! 同一回事。

use serde_json::json;

use super::NodeCreateBody;

/// 树上创建 endpoint 时发的 body 形状：结构节点 + 一个只带 url 的 `detail`。
fn endpoint_body(detail: serde_json::Value) -> serde_json::Value {
    json!({
        "id": "e1",
        "rootId": "p1",
        "parentId": "m1",
        "kind": "endpoint",
        "name": "未命名接口",
        "sortOrder": 0,
        "createdAt": 0,
        "updatedAt": 0,
        "expanded": true,
        "detail": detail,
    })
}

#[test]
fn accepts_an_endpoint_detail_that_names_only_a_url() {
    let parsed: NodeCreateBody =
        serde_json::from_value(endpoint_body(json!({ "url": "" }))).expect("body parses");

    let detail = parsed.detail.expect("detail survives the round trip");
    let row = detail.into_detail("e1");

    assert_eq!(row.node_id, "e1");
    assert_eq!(row.url, "");
    // 要害所在：method 由服务端给出可发送的默认值，而不是向客户端索要。
    assert_eq!(row.method.as_str(), "GET");
}

#[test]
fn an_absent_detail_stays_absent_so_a_collection_is_not_given_one() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body.as_object_mut()
        .expect("body is an object")
        .remove("detail");

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    // `db::nodes::create` 会拒绝带 detail 的 collection，故 DTO 不能替一个本来没发
    // detail 的节点凭空造一个。
    assert!(parsed.detail.is_none());
}

#[test]
fn a_method_the_caller_did_send_is_kept() {
    let parsed: NodeCreateBody = serde_json::from_value(endpoint_body(json!({
        "method": "post",
        "url": "/orders",
    })))
    .expect("body parses");

    let row = parsed.detail.expect("detail present").into_detail("e1");

    assert_eq!(row.method.as_str(), "POST");
    assert_eq!(row.url, "/orders");
}

/// 只有结构那一半：renderer 指定父节点并把 `rootId` 发成 `null` 表示「你来推」，
/// 因为存储行的类型（`ResourceNode.root_id: String`）与 create 调用所知的不是同一
/// 回事。按 `ResourceNode` 取 body 会变成 `400 invalid type: null, expected a
/// string`——且只在这条 transport 上，因为 Tauri 命令收的是 `root_id:
/// Option<String>`。
#[test]
fn accepts_a_null_root_id_so_the_route_can_derive_it() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body["rootId"] = serde_json::Value::Null;

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    assert_eq!(parsed.node.root_id, None);
    assert_eq!(parsed.node.parent_id.as_deref(), Some("m1"));
    // 路由查到父节点后构造出的那一行。
    let row = parsed.node.into_node("p1");
    assert_eq!(row.root_id, "p1");
    assert_eq!(row.id, "e1");
    assert_eq!(row.sort_order, 0);
}

#[test]
fn an_omitted_root_id_parses_the_same_as_an_explicit_null() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body.as_object_mut()
        .expect("body is an object")
        .remove("rootId");

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    assert_eq!(parsed.node.root_id, None);
}

#[test]
fn a_root_id_the_caller_did_send_is_not_derived_over() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body["rootId"] = json!("p_explicit");

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    // 路由的做法：只在客户端没给时才推导。
    let root_id = parsed.node.root_id.clone().unwrap_or_default();
    let row = parsed.node.into_node(&root_id);

    assert_eq!(row.root_id, "p_explicit");
}
