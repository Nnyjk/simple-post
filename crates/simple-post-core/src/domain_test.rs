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
fn apikey_location_field_round_trips_without_a_rename() {
    // The TS mirror reads `inLocation`. `in` is a TypeScript keyword and
    // `in_` is not a fix — camelCase maps it straight back to `in`, which
    // is how the two languages used to disagree on this one field.
    let v = json!({
        "kind": "apikey",
        "apikey": { "key": "X-API-Key", "value": "abc", "inLocation": "header" }
    });
    let auth: AuthConfig = serde_json::from_value(v).unwrap();
    let apikey = auth.apikey.clone().unwrap();
    assert_eq!(apikey.in_location, ApiKeyLocation::Header);
    // …and the emitted key is the same word the mirror declares.
    let back = serde_json::to_value(&auth).unwrap();
    assert_eq!(back["kind"], json!("apikey"));
    assert_eq!(back["apikey"]["inLocation"], json!("header"));
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
        "kind": "endpoint",
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
        "auth": { "kind": "none" },
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
        module_base_urls: [(
            "def_default".to_string(),
            [
                ("m_auth".to_string(), "http://auth".to_string()),
                ("m_order".to_string(), "http://order".to_string()),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect(),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
        is_active: true,
    };
    let s = serde_json::to_string(&env).unwrap();
    // The wire names both URL maps, and the retired one keeps its
    // historical `baseUrls` spelling so an older client still parses it.
    assert!(s.contains("\"moduleBaseUrls\""), "{s}");
    let back: Environment = serde_json::from_str(&s).unwrap();
    assert_eq!(back.base_urls.get("def_default").unwrap(), "http://x");
    assert_eq!(
        back.module_base_urls
            .get("def_default")
            .unwrap()
            .get("m_auth")
            .unwrap(),
        "http://auth"
    );
    assert_eq!(back.variables.get("token").unwrap(), "abc");
    assert!(back.is_active);
}

#[test]
fn environment_deserialises_without_module_base_urls() {
    // A payload written before the value layer moved must still load: the
    // field carries `#[serde(default)]` precisely so it arrives empty
    // rather than failing the whole read.
    let back: Environment = serde_json::from_value(json!({
        "id": "e1",
        "projectId": "p1",
        "name": "dev",
        "baseUrls": { "def_default": "http://x" },
        "variables": {},
        "isActive": false,
    }))
    .unwrap();
    assert!(back.module_base_urls.is_empty());
    assert_eq!(back.base_urls.get("def_default").unwrap(), "http://x");
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

#[test]
fn permission_serde_wire_matches_as_str() {
    // The regression this guards: `rename_all = "lowercase"` spelled
    // ReadCall `"readcall"`, a third form matching neither `as_str()` nor
    // the `agent_permissions` CHECK constraint nor the TS union. The old
    // round-trip test passed anyway — serde could read back its own
    // invention — so it is this assertion, not the round trip, that
    // would have caught it.
    for p in [Permission::Read, Permission::ReadCall, Permission::Full] {
        let wire = serde_json::to_string(&p).unwrap();
        assert_eq!(
            wire,
            format!("\"{}\"", p.as_str()),
            "serde wire form must equal as_str()"
        );
        let back: Permission = serde_json::from_str(&wire).unwrap();
        assert_eq!(p, back);
    }
}

#[test]
fn http_method_has_one_parser_for_every_entry_point() {
    // Lowercase from any caller must land on the same stored verb the
    // MCP path would produce. A derived `Deserialize` used to make
    // `"get"` into `Custom("get")`, which then round-tripped out
    // lowercase and was written to the database that way.
    for (json, want) in [
        ("\"get\"", HttpMethod::Get),
        ("\"PoSt\"", HttpMethod::Post),
        ("\"PROPFIND\"", HttpMethod::Custom("PROPFIND".into())),
        ("\"propfind\"", HttpMethod::Custom("PROPFIND".into())),
    ] {
        let m: HttpMethod = serde_json::from_str(json).unwrap();
        assert_eq!(m, want, "deserialising {json}");
        assert_eq!(m.as_str(), want.as_str());
        // Whatever comes in, what goes out is upper-case.
        assert_eq!(
            serde_json::to_string(&m).unwrap(),
            format!("\"{}\"", m.as_str())
        );
        // …and `parse_http_method` agrees with the serde path.
        assert_eq!(parse_http_method(json.trim_matches('"')).unwrap(), want);
    }
}

#[test]
fn http_method_rejects_non_token_input() {
    for bad in ["", "   ", "GET POST", "GE\tT", "你好", "GET/1"] {
        assert!(
            parse_http_method(bad).is_err(),
            "{bad:?} should not parse as a method"
        );
        // Same rejection through the serde path — one parser, one verdict.
        let err = serde_json::from_str::<HttpMethod>(&format!("\"{bad}\""));
        assert!(err.is_err(), "serde should reject {bad:?}");
    }
}

#[test]
fn project_always_emits_default_base_url_definition_id() {
    // The TS mirror declares it required-but-nullable, so "no default"
    // has to arrive as an explicit null rather than as a missing key.
    let p = Project {
        id: "p".into(),
        name: "n".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![],
        default_base_url_definition_id: None,
        color: "#3b82f6".into(),
        created_at: 0,
        updated_at: 0,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert!(
        v.get("defaultBaseUrlDefinitionId").is_some(),
        "key must be present, got {v}"
    );
    assert_eq!(v["defaultBaseUrlDefinitionId"], serde_json::Value::Null);
    // …and a set value still round-trips.
    let p = Project {
        default_base_url_definition_id: Some("def1".into()),
        ..p
    };
    let back: Project = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    assert_eq!(back.default_base_url_definition_id.as_deref(), Some("def1"));
}

#[test]
fn a_default_patch_is_empty() {
    assert!(ResourceNodePatch::default().is_empty());
    assert!(EndpointDetailPatch::default().is_empty());
    assert!(ProjectPatch::default().is_empty());
    assert!(EnvironmentPatch::default().is_empty());
}

#[test]
fn request_settings_reads_a_blob_written_by_an_older_build() {
    // The value lives in `preferences` as JSON, so whatever the last
    // build wrote is what the next one has to read. A field added to
    // `RequestSettings` tomorrow must not break every install that
    // stored a row before it existed — that is what the container-level
    // `#[serde(default)]` buys, and this is the case it protects.
    let older = json!({ "timeoutMs": 5000 });
    let s: RequestSettings = serde_json::from_value(older).unwrap();
    assert_eq!(s.timeout_ms, 5000, "stored value must win");
    assert_eq!(
        s.follow_redirects,
        RequestSettings::default().follow_redirects,
        "absent field falls back to its default"
    );
    // An empty blob is the degenerate case: all defaults, not an error.
    let s: RequestSettings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(s.timeout_ms, 30_000);
    assert!(s.follow_redirects);
    assert_eq!(s.max_response_size_kb, 5120);
}

#[test]
fn every_patch_field_alone_makes_the_patch_non_empty() {
    // This is the regression net for the footgun `is_empty` used to be.
    // It used to be a hand-written field list, so adding a field and
    // forgetting to extend the list made the MCP `endpoint_update` tool
    // silently ignore that field while the Tauri and REST paths — which
    // never called `is_empty` at all — applied it. One bug, two
    // opposite behaviours depending on the transport.
    //
    // `is_empty` now derives from serialisation, so it cannot drift.
    // This test is the second line of defence, and its real job is
    // different: adding a field makes the compiler demand a line here,
    // turning a silent skip into an edit you cannot forget. Struct
    // update syntax is deliberate — it names the field on the same line
    // as the assertion, so a reviewer can check the two at a glance.

    let n = ResourceNodePatch {
        parent_id: Some(Some("c1".into())),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::parent_id");
    let n = ResourceNodePatch {
        name: Some("renamed".into()),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::name");
    let n = ResourceNodePatch {
        description: Some(Some("d".into())),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::description");
    let n = ResourceNodePatch {
        sort_order: Some(3),
        ..Default::default()
    };
    assert!(!n.is_empty(), "ResourceNodePatch::sort_order");

    let d = EndpointDetailPatch {
        method: Some(HttpMethod::Post),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::method");
    let d = EndpointDetailPatch {
        url: Some("/x".into()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::url");
    let d = EndpointDetailPatch {
        environment_id: Some(Some("e1".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::environment_id");
    let d = EndpointDetailPatch {
        base_url_definition_id: Some(Some("def1".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::base_url_definition_id");
    let d = EndpointDetailPatch {
        base_url_override: Some(Some("https://x".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::base_url_override");
    let d = EndpointDetailPatch {
        params: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::params");
    let d = EndpointDetailPatch {
        headers: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::headers");
    let d = EndpointDetailPatch {
        body: Some(RequestBody::default()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::body");
    let d = EndpointDetailPatch {
        auth: Some(AuthConfig::default()),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::auth");
    let d = EndpointDetailPatch {
        docs: Some(Some("doc".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::docs");
    let d = EndpointDetailPatch {
        notes: Some(Some("note".into())),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::notes");
    let d = EndpointDetailPatch {
        tags: Some(vec![]),
        ..Default::default()
    };
    assert!(!d.is_empty(), "EndpointDetailPatch::tags");

    let p = ProjectPatch {
        name: Some("n".into()),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::name");
    let p = ProjectPatch {
        description: Some(Some("d".into())),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::description");
    let p = ProjectPatch {
        base_url: Some(Some("u".into())),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::base_url");
    let p = ProjectPatch {
        base_url_definitions: Some(vec![]),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::base_url_definitions");
    let p = ProjectPatch {
        default_base_url_definition_id: Some(Some("def1".into())),
        ..Default::default()
    };
    assert!(
        !p.is_empty(),
        "ProjectPatch::default_base_url_definition_id"
    );
    let p = ProjectPatch {
        color: Some("#fff".into()),
        ..Default::default()
    };
    assert!(!p.is_empty(), "ProjectPatch::color");

    let e = EnvironmentPatch {
        name: Some("n".into()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::name");
    let e = EnvironmentPatch {
        base_urls: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::base_urls");
    let e = EnvironmentPatch {
        variables: Some(HashMap::new()),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::variables");
    let e = EnvironmentPatch {
        is_active: Some(false),
        ..Default::default()
    };
    assert!(!e.is_empty(), "EnvironmentPatch::is_active");
}

#[test]
fn clearing_a_nullable_column_is_not_an_empty_patch() {
    // `Some(None)` means "write NULL", not "leave alone" — the reason
    // `double_option` exists at all. It must not read as a no-op, or
    // "clear this field" silently stops clearing.
    let n = ResourceNodePatch {
        description: Some(None),
        ..Default::default()
    };
    assert!(!n.is_empty(), "clearing description is a real write");
    // …and it really does emit the key.
    let v = serde_json::to_value(&n).unwrap();
    assert!(v.get("description").is_some(), "got {v}");
    assert_eq!(v["description"], serde_json::Value::Null);

    let d = EndpointDetailPatch {
        environment_id: Some(None),
        ..Default::default()
    };
    assert!(!d.is_empty(), "clearing environmentId is a real write");
    assert_eq!(
        serde_json::to_value(&d).unwrap()["environmentId"],
        serde_json::Value::Null
    );
}

#[test]
fn may_under_is_the_whole_parent_child_matrix() {
    use ResourceNodeType::{Collection, Endpoint, Module};

    // A module's parent is the project, which is not a node — so no node
    // type accepts one, and `db::nodes` spells "top level" as
    // `parent_id = NULL` instead.
    for parent in [Module, Collection, Endpoint] {
        assert!(
            !Module.may_under(parent),
            "a module must not sit under a {}",
            parent.as_str()
        );
    }

    assert!(Collection.may_under(Module));
    assert!(Collection.may_under(Collection));
    assert!(!Collection.may_under(Endpoint));

    // An endpoint is the one row naming *two* parents. Forcing a
    // collection made the "single endpoint, no folder" case cost a
    // throwaway collection first.
    assert!(Endpoint.may_under(Module));
    assert!(Endpoint.may_under(Collection));
    assert!(!Endpoint.may_under(Endpoint));
}
