use super::*;
use crate::db;
use crate::db::nodes;
use crate::domain::{AuthType, HttpMethod, Project, RequestBody, ResourceNode, ResourceNodeType};
use std::path::PathBuf;

fn node(id: &str, kind: ResourceNodeType, parent: Option<&str>, sort: i64) -> ResourceNode {
    ResourceNode {
        id: id.into(),
        root_id: "p1".into(),
        parent_id: parent.map(str::to_string),
        kind,
        name: id.into(),
        description: None,
        sort_order: sort,
        created_at: 1,
        updated_at: 1,
    }
}

/// A database with one project, one module, one collection and three
/// endpoints.
///
/// Built through the public CRUD API: the fixtures describe the old
/// three-table layout and populate none of `nodes`.
fn fresh_pool() -> (db::DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-endpoint-details-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        nodes::create(c, &node("m1", ResourceNodeType::Module, None, 0), None)?;
        nodes::create(
            c,
            &node("c1", ResourceNodeType::Collection, Some("m1"), 0),
            None,
        )?;
        for (i, id) in ["e1", "e2", "e3"].iter().enumerate() {
            nodes::create(
                c,
                &node(id, ResourceNodeType::Endpoint, Some("c1"), i as i64),
                Some(&detail(id)),
            )?;
        }
        Ok(())
    })
    .expect("seed tree");
    (pool, dir)
}

fn detail(node_id: &str) -> EndpointDetail {
    EndpointDetail {
        node_id: node_id.into(),
        method: HttpMethod::Get,
        url: format!("/{node_id}"),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
    }
}

// -------------------------------------------------------------------------
// Round-tripping the JSON columns.
// -------------------------------------------------------------------------

#[test]
fn json_columns_round_trip() {
    let (pool, _dir) = fresh_pool();
    // `r##` because the payload contains `"#`, which would close an
    // `r#` literal at the markdown heading below.
    let patch = r##"{
        "method": "POST",
        "url": "/orders",
        "environmentId": "env1",
        "baseUrlDefinitionId": "def1",
        "baseUrlOverride": "https://mirror.example",
        "params": [{"id":"k1","key":"page","value":"1","enabled":true,"description":"分页"}],
        "headers": [{"id":"k2","key":"X-Trace","value":"on","enabled":true}],
        "body": {"mode":"json","content":"{\"a\":1}","schema":"{}"},
        "auth": {"kind":"apikey","apikey":{"key":"X-API-Key","value":"{{apiKey}}","inLocation":"query"}},
        "docs": "# orders",
        "notes": "careful",
        "tags": ["订单","crud"]
    }"##;
    db::with_tx(&pool, |c| {
        update(c, "e1", serde_json::from_str(patch).unwrap())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.method, HttpMethod::Post);
    assert_eq!(got.url, "/orders");
    assert_eq!(got.environment_id.as_deref(), Some("env1"));
    assert_eq!(got.base_url_definition_id.as_deref(), Some("def1"));
    assert_eq!(
        got.base_url_override.as_deref(),
        Some("https://mirror.example")
    );
    assert_eq!(got.params.len(), 1);
    assert_eq!(got.params[0].key, "page");
    assert_eq!(got.params[0].description.as_deref(), Some("分页"));
    assert_eq!(got.headers[0].key, "X-Trace");
    assert_eq!(got.body.content, r#"{"a":1}"#);
    assert_eq!(got.body.schema.as_deref(), Some("{}"));
    assert_eq!(got.auth.kind, AuthType::Apikey);
    assert_eq!(got.auth.apikey.as_ref().unwrap().value, "{{apiKey}}");
    assert_eq!(got.docs.as_deref(), Some("# orders"));
    assert_eq!(got.notes.as_deref(), Some("careful"));
    assert_eq!(got.tags, vec!["订单".to_string(), "crud".to_string()]);
}

#[test]
fn get_missing_reports_not_found() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_conn(&pool, |c| get(c, "nope")).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
}

// -------------------------------------------------------------------------
// Partial update, including the absent / explicit-null distinction.
// -------------------------------------------------------------------------

#[test]
fn update_touches_only_what_it_is_given() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            serde_json::from_str(r#"{"method": "DELETE", "docs": null}"#).unwrap(),
        )
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.method, HttpMethod::Delete, "the patch applies");
    assert!(got.docs.is_none(), "explicit null clears");
    assert_eq!(got.url, "/e1", "an absent key leaves the column alone");
}

#[test]
fn absent_and_explicit_null_differ() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            serde_json::from_str(r#"{"docs": "keep me"}"#).unwrap(),
        )
    })
    .unwrap();
    // An empty patch must be a no-op, not a clear.
    db::with_tx(&pool, |c| {
        update(c, "e1", serde_json::from_str("{}").unwrap())
    })
    .unwrap();
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .docs
            .as_deref(),
        Some("keep me")
    );
}

#[test]
fn empty_patch_returns_the_row_unchanged() {
    let (pool, _dir) = fresh_pool();
    let before = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    let after = db::with_tx(&pool, |c| {
        update(c, "e1", serde_json::from_str("{}").unwrap())
    })
    .unwrap();
    assert_eq!(before.url, after.url);
    assert_eq!(before.method, after.method);
}

#[test]
fn update_missing_reports_not_found() {
    let (pool, _dir) = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        update(c, "nope", serde_json::from_str(r#"{"url": "/x"}"#).unwrap())
    })
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
}

// -------------------------------------------------------------------------
// Batched reads.
// -------------------------------------------------------------------------

#[test]
fn list_by_root_returns_only_that_projects_endpoints() {
    let (pool, _dir) = fresh_pool();
    let got = db::with_conn(&pool, |c| list_by_root(c, "p1")).unwrap();
    let ids: Vec<&str> = got.iter().map(|d| d.node_id.as_str()).collect();
    assert_eq!(ids, vec!["e1", "e2", "e3"], "ordered by the node's slot");

    let other = db::with_conn(&pool, |c| list_by_root(c, "p2")).unwrap();
    assert!(other.is_empty());
}

#[test]
fn list_by_node_ids_handles_an_empty_request() {
    // Called with nothing to look up, which a tree read will hit on any
    // project that has no endpoints. Must not build `IN ()`.
    let (pool, _dir) = fresh_pool();
    let got = db::with_conn(&pool, |c| list_by_node_ids(c, &[])).unwrap();
    assert!(got.is_empty());
}

#[test]
fn list_by_node_ids_round_trips_through_the_json_columns() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e2",
            serde_json::from_str(r#"{"tags": ["x"], "notes": "n"}"#).unwrap(),
        )
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| {
        list_by_node_ids(c, &["e2".to_string(), "e1".to_string()])
    })
    .unwrap();
    let e2 = got.iter().find(|d| d.node_id == "e2").unwrap();
    assert_eq!(e2.tags, vec!["x".to_string()]);
    assert_eq!(e2.notes.as_deref(), Some("n"));
    assert_eq!(got.len(), 2);
}

#[test]
fn a_detail_row_cannot_outlive_its_node() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| nodes::delete(c, "e1")).unwrap();
    let err = db::with_conn(&pool, |c| get(c, "e1")).unwrap_err();
    assert!(
        matches!(err, AppError::NotFound(_)),
        "endpoint_details cascades with its node, got {err:?}"
    );
}
