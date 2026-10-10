//! Regression net for the `POST /api/nodes` body contract.
//!
//! The route used to deserialise `detail` straight into
//! `simple_post_core::domain::EndpointDetail`, whose `method` has no
//! `#[serde(default)]`. The tree's "new request" action sends exactly
//! what the user has typed so far — `detail: { url }` — so the *same*
//! click created an endpoint in the desktop app (whose command takes
//! `CreateEndpointDetailOpts`, every field optional) and answered
//! `400 validation: missing field 'method'` under `serve-web`.
//!
//! These tests pin the tolerant shape at the DTO boundary: no pool, no
//! router, no HTTP — the body has to parse, and a half-filled endpoint
//! has to expand into a row with a real method.

use serde_json::json;

use super::NodeCreateBody;

/// A body shaped like the one the tree sends when creating an endpoint:
/// structural node plus a `detail` carrying nothing but the url.
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
    // The whole point: a method the request can actually be sent with,
    // defaulted rather than demanded from the client.
    assert_eq!(row.method.as_str(), "GET");
}

#[test]
fn an_absent_detail_stays_absent_so_a_collection_is_not_given_one() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body.as_object_mut()
        .expect("body is an object")
        .remove("detail");

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    // `db::nodes::create` rejects a collection carrying a detail, so the
    // DTO must not manufacture one for a node that sent none.
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

/// The structural half, on its own: the renderer names a parent and
/// sends `rootId: null` for "work it out", because the stored row type
/// (`ResourceNode.root_id: String`) is not the same thing as what a
/// create call knows. Taking the body as `ResourceNode` turned that
/// into `400 invalid type: null, expected a string` — on this transport
/// only, since the Tauri command takes `root_id: Option<String>`.
#[test]
fn accepts_a_null_root_id_so_the_route_can_derive_it() {
    let mut body = endpoint_body(json!({ "url": "" }));
    body["rootId"] = serde_json::Value::Null;

    let parsed: NodeCreateBody = serde_json::from_value(body).expect("body parses");

    assert_eq!(parsed.node.root_id, None);
    assert_eq!(parsed.node.parent_id.as_deref(), Some("m1"));
    // The row the route builds once it has looked the parent up.
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

    // What the route does: derive only when the client left it out.
    let root_id = parsed.node.root_id.clone().unwrap_or_default();
    let row = parsed.node.into_node(&root_id);

    assert_eq!(row.root_id, "p_explicit");
}
