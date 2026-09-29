use super::*;
use serde_json::json;

#[test]
fn http_method_round_trips_known_verbs() {
    for v in [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Head,
        HttpMethod::Options,
    ] {
        let s = serde_json::to_string(&v).unwrap();
        let back: HttpMethod = serde_json::from_str(&s).unwrap();
        assert_eq!(v, back);
    }
}

#[test]
fn http_method_serializes_uppercase_strings() {
    assert_eq!(serde_json::to_string(&HttpMethod::Get).unwrap(), "\"GET\"");
    assert_eq!(
        serde_json::to_string(&HttpMethod::Post).unwrap(),
        "\"POST\""
    );
}

#[test]
fn http_method_custom_verb_passes_through() {
    let raw = "\"PROPFIND\"";
    let m: HttpMethod = serde_json::from_str(raw).unwrap();
    assert_eq!(m.as_str(), "PROPFIND");
}

#[test]
fn apikey_in_field_uses_unquoted_name() {
    let v = json!({
        "type": "apikey",
        "apikey": { "key": "X-API-Key", "value": "abc", "in": "header" }
    });
    let auth: AuthConfig = serde_json::from_value(v).unwrap();
    let apikey = auth.apikey.unwrap();
    assert_eq!(apikey.in_, ApiKeyLocation::Header);
}

#[test]
fn endpoint_camel_case_field_names() {
    // The structural half and the configuration half are separate types
    // now, so this checks both deserialize from the camelCase wire shape
    // their TS mirrors expect.
    let v = json!({
        "id": "e1",
        "rootId": "p1",
        "parentId": "c1",
        "type": "endpoint",
        "name": "foo",
        "sortOrder": 0,
        "createdAt": 1,
        "updatedAt": 2,
    });
    let n: ResourceNode = serde_json::from_value(v).unwrap();
    assert_eq!(n.parent_id.as_deref(), Some("c1"));
    assert_eq!(n.root_id, "p1");
    assert_eq!(n.kind, ResourceNodeType::Endpoint);
    assert_eq!(n.created_at, 1);

    let v = json!({
        "nodeId": "e1",
        "method": "GET",
        "url": "/x",
        "params": [],
        "headers": [],
        "body": { "mode": "none", "content": "" },
        "auth": { "type": "none" },
        "tags": [],
    });
    let d: EndpointDetail = serde_json::from_value(v).unwrap();
    assert_eq!(d.node_id, "e1");
    assert_eq!(d.method, HttpMethod::Get);
    assert_eq!(d.url, "/x");
}

#[test]
fn endpoint_patch_distinguishes_absent_from_null() {
    // `description` field absent → None
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "url": "/renamed"
    }))
    .unwrap();
    assert_eq!(p.url.as_deref(), Some("/renamed"));
    assert!(p.environment_id.is_none());

    // `environmentId: null` → Some(None) (clear the column)
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "environmentId": null
    }))
    .unwrap();
    assert_eq!(p.environment_id, Some(None));

    // `environmentId: "e1"` → Some(Some("e1"))
    let p: EndpointDetailPatch = serde_json::from_value(json!({
        "environmentId": "e1"
    }))
    .unwrap();
    assert_eq!(p.environment_id, Some(Some("e1".into())));
}

#[test]
fn node_patch_distinguishes_absent_from_null() {
    // `parentId` absent → leave the node where it is
    let p: ResourceNodePatch = serde_json::from_value(json!({ "name": "renamed" })).unwrap();
    assert_eq!(p.name.as_deref(), Some("renamed"));
    assert!(p.parent_id.is_none());

    // `parentId: null` → promote to the top of the project. Only legal
    // for a module, but the domain layer carries the intent and the DB
    // layer is what enforces the type matrix.
    let p: ResourceNodePatch = serde_json::from_value(json!({ "parentId": null })).unwrap();
    assert_eq!(p.parent_id, Some(None));
}

#[test]
fn environment_base_urls_round_trip() {
    let env = Environment {
        id: "e1".into(),
        project_id: "p1".into(),
        name: "dev".into(),
        base_urls: [("def_default".to_string(), "http://x".to_string())]
            .into_iter()
            .collect(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        is_active: true,
    };
    let s = serde_json::to_string(&env).unwrap();
    let back: Environment = serde_json::from_str(&s).unwrap();
    assert_eq!(back.base_urls.get("def_default").unwrap(), "http://x");
    assert_eq!(back.variables.get("token").unwrap(), "abc");
    assert!(back.is_active);
}

#[test]
fn permission_round_trip() {
    for p in [Permission::Read, Permission::ReadCall, Permission::Full] {
        let s = serde_json::to_string(&p).unwrap();
        let back: Permission = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }
    assert_eq!(Permission::parse("read"), Some(Permission::Read));
    assert_eq!(Permission::parse("read_call"), Some(Permission::ReadCall));
    assert_eq!(Permission::parse("full"), Some(Permission::Full));
    assert_eq!(Permission::parse("nope"), None);
}
