use super::*;
use crate::db;
use crate::db::nodes;
use crate::domain::{
    AuthType, HttpMethod, Project, RequestBody, ResourceNode, ResourceNodeType, TlsSettings,
};
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

/// 一个含一个项目、一个模块、一个 collection 与三个 endpoint 的库。
/// 经公开 CRUD API 构造：fixture 描述的是旧三表布局，没有一行落在 `nodes` 上。
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
        base_url_override: None,
        params: vec![],
        path_params: vec![],
        headers: vec![],
        body: RequestBody::default(),
        auth: AuthConfig::default(),
        docs: None,
        notes: None,
        tags: vec![],
        pre_request_script: None,
        test_script: None,
        tls: None,
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
        "baseUrlOverride": "https://mirror.example",
        "params": [{"id":"k1","key":"page","value":"1","enabled":true,"description":"分页"}],
        "pathParams": [{"id":"k3","key":"id","value":"42","enabled":true}],
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
    assert_eq!(
        got.base_url_override.as_deref(),
        Some("https://mirror.example")
    );
    assert_eq!(got.params.len(), 1);
    assert_eq!(got.params[0].key, "page");
    assert_eq!(got.params[0].description.as_deref(), Some("分页"));
    assert_eq!(got.path_params.len(), 1);
    assert_eq!(got.path_params[0].key, "id");
    assert_eq!(got.path_params[0].value, "42");
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

#[test]
fn the_script_columns_survive_a_round_trip() {
    let (pool, _dir) = fresh_pool();
    let patch =
        r##"{"preRequestScript":"pm.request.url = 'x';","testScript":"pm.test('t', () => {});"}"##;
    db::with_tx(&pool, |c| {
        update(c, "e1", serde_json::from_str(patch).unwrap())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(
        got.pre_request_script.as_deref(),
        Some("pm.request.url = 'x';")
    );
    assert_eq!(got.test_script.as_deref(), Some("pm.test('t', () => {});"));
}

#[test]
fn an_older_store_gains_the_script_columns_without_a_version_bump() {
    let (pool, dir) = fresh_pool();
    // 造一个「版本号是当前的、但还没有脚本列」的库——既有库在这次改动之前就是这个形状。
    db::with_conn(&pool, |c| {
        Ok(c.execute_batch(
            "ALTER TABLE endpoint_details DROP COLUMN pre_request_script;\
             ALTER TABLE endpoint_details DROP COLUMN test_script;",
        )?)
    })
    .unwrap();
    drop(pool);

    let pool = db::init(&dir).expect("re-init");

    // 列被补上。
    let columns: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('endpoint_details') \
             WHERE name IN ('pre_request_script','test_script')",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(columns, 2, "脚本列应由 ADDED_COLUMNS 补齐");

    // 旧行读回来是「没有脚本」，不是坏行。
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert!(got.pre_request_script.is_none());
    assert!(got.test_script.is_none());

    // 版本号没动：bump 会让 `ensure_current_schema` 删掉每个用户的库。
    let version: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(version, crate::db::SCHEMA_VERSION);
}

#[test]
fn an_older_store_gains_the_https_settings_column_without_a_version_bump() {
    let (pool, dir) = fresh_pool();
    // 造一个「版本号是当前的、但还没有 HTTPS 设置列」的库——既有库在这次改动之前就是这个形状。
    db::with_conn(&pool, |c| {
        Ok(c.execute_batch("ALTER TABLE endpoint_details DROP COLUMN tls_json;")?)
    })
    .unwrap();
    drop(pool);

    let pool = db::init(&dir).expect("re-init");

    // 列被补上。
    let columns: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('endpoint_details') WHERE name = 'tls_json'",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(columns, 1, "HTTPS 设置列应由 ADDED_COLUMNS 补齐");

    // 旧行读回来是「继承全局」，不是坏行。
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert!(got.tls.is_none());

    // 版本号没动：bump 会让 `ensure_current_schema` 删掉每个用户的库。
    let version: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(version, crate::db::SCHEMA_VERSION);
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

/// 路径参数这一列的三态：缺席 = 不动，`null` = 清空，写值 = 写入。
///
/// `path_params_json` 是 `NOT NULL` 的列，故「清空」写成空列表而不是 NULL。
#[test]
fn the_path_params_column_distinguishes_absent_from_null() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            serde_json::from_str(
                r#"{"pathParams":[{"id":"k1","key":"id","value":"42","enabled":true}]}"#,
            )
            .unwrap(),
        )
    })
    .unwrap();
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .path_params
            .len(),
        1
    );

    // 缺席：这一列不动。
    db::with_tx(&pool, |c| {
        update(c, "e1", serde_json::from_str(r#"{"url": "/x"}"#).unwrap())
    })
    .unwrap();
    assert_eq!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .path_params
            .len(),
        1,
        "缺席的键不该动这一列"
    );

    // `null`：清空。内建的 `Option<Vec<KeyValue>>` 把缺席与 `null` 折叠成一件事，
    // 于是清空请求被当成「别动」而静默失效。
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            serde_json::from_str(r#"{"pathParams":null}"#).unwrap(),
        )
    })
    .unwrap();
    assert!(
        db::with_conn(&pool, |c| get(c, "e1"))
            .unwrap()
            .path_params
            .is_empty(),
        "显式的 null 应当清空这一列"
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

// -------------------------------------------------------------------------
// 端点自己的 HTTPS 出站设置
// -------------------------------------------------------------------------

/// 一份端点自带的 HTTPS 设置。
fn endpoint_tls() -> TlsSettings {
    TlsSettings {
        skip_verify: true,
        ca_certificate_path: Some("C:/certs/ca.pem".into()),
        client_certificate_path: Some("C:/certs/client.pem".into()),
        client_key_path: Some("C:/certs/client.key".into()),
    }
}

#[test]
fn an_endpoint_without_its_own_https_settings_inherits_the_global_ones() {
    let (pool, _dir) = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert!(
        got.tls.is_none(),
        "没配过的端点读回来必须是 None（= 继承全局），got {:?}",
        got.tls
    );
}

#[test]
fn the_endpoints_own_https_settings_round_trip() {
    let (pool, _dir) = fresh_pool();
    let tls = endpoint_tls();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            EndpointDetailPatch {
                tls: Some(Some(tls.clone())),
                ..Default::default()
            },
        )
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.tls.as_ref(), Some(&tls));
}

#[test]
fn clearing_the_endpoints_own_https_settings_puts_it_back_to_inheriting() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            EndpointDetailPatch {
                tls: Some(Some(endpoint_tls())),
                ..Default::default()
            },
        )
    })
    .unwrap();
    // `null` = 清除端点自带的那份，回到继承全局默认。
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            EndpointDetailPatch {
                tls: Some(None),
                ..Default::default()
            },
        )
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert!(
        got.tls.is_none(),
        "清除后读回来必须是 None（= 继承全局），got {:?}",
        got.tls
    );
}

#[test]
fn a_patch_without_the_https_settings_leaves_them_alone() {
    let (pool, _dir) = fresh_pool();
    let tls = endpoint_tls();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            EndpointDetailPatch {
                tls: Some(Some(tls.clone())),
                ..Default::default()
            },
        )
    })
    .unwrap();
    // 一次与 HTTPS 无关的更新不该动它。
    db::with_tx(&pool, |c| {
        update(
            c,
            "e1",
            EndpointDetailPatch {
                notes: Some(Some("n".into())),
                ..Default::default()
            },
        )
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e1")).unwrap();
    assert_eq!(got.tls.as_ref(), Some(&tls));
}

#[test]
fn the_endpoints_own_https_settings_survive_a_reload_of_several_rows() {
    let (pool, _dir) = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e2",
            EndpointDetailPatch {
                tls: Some(Some(endpoint_tls())),
                ..Default::default()
            },
        )
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| {
        list_by_node_ids(c, &["e2".to_string(), "e1".to_string()])
    })
    .unwrap();
    let e2 = got.iter().find(|d| d.node_id == "e2").unwrap();
    assert_eq!(e2.tls.as_ref(), Some(&endpoint_tls()));
    let e1 = got.iter().find(|d| d.node_id == "e1").unwrap();
    assert!(e1.tls.is_none(), "没配过的端点仍应是 None");
}
