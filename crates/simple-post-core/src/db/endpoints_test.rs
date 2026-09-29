use super::*;
use crate::db;
use crate::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BodyMode, HttpMethod, KeyValue, RequestBody,
};

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-endpoints-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

fn sample_endpoint(id: &str) -> Endpoint {
    Endpoint {
        id: id.into(),
        collection_id: "c_login".into(),
        name: "Login".into(),
        description: None,
        method: HttpMethod::Post,
        url: "/login".into(),
        environment_id: None,
        base_url_definition_id: None,
        base_url_override: None,
        params: vec![],
        headers: vec![],
        body: RequestBody {
            mode: BodyMode::None,
            content: String::new(),
            schema: None,
        },
        auth: AuthConfig {
            kind: AuthType::None,
            bearer: None,
            basic: None,
            apikey: None,
        },
        docs: None,
        notes: None,
        tags: vec!["smoke".into()],
        sort_order: 0,
        created_at: 1,
        updated_at: 2,
    }
}

#[test]
fn create_and_get_round_trip_preserves_json_columns() {
    let pool = fresh_pool();
    let mut ep = sample_endpoint("e_new");
    ep.params = vec![KeyValue {
        id: "p1".into(),
        key: "include".into(),
        value: "all".into(),
        enabled: true,
        description: None,
    }];
    ep.body = RequestBody {
        mode: BodyMode::Json,
        content: "{\"x\":1}".into(),
        schema: None,
    };
    ep.auth = AuthConfig {
        kind: AuthType::Apikey,
        bearer: None,
        basic: None,
        apikey: Some(ApiKeyAuth {
            key: "X-API-Key".into(),
            value: "abc".into(),
            in_: ApiKeyLocation::Header,
        }),
    };
    db::with_tx(&pool, |c| create(c, &ep)).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e_new")).unwrap();
    assert_eq!(got.params.len(), 1);
    assert_eq!(got.params[0].key, "include");
    assert_eq!(got.body.mode, BodyMode::Json);
    assert_eq!(got.auth.kind, AuthType::Apikey);
    assert_eq!(got.auth.apikey.unwrap().in_, ApiKeyLocation::Header);
}

#[test]
fn search_by_query_substring() {
    let pool = fresh_pool();
    let ids: Vec<String> = db::with_conn(&pool, |c| {
        let rows = search(c, Some("短信"), None, None)?;
        let out: Vec<String> = rows.into_iter().map(|r| r.id).collect();
        Ok(out)
    })
    .unwrap();
    // The seeded endpoint name "短信登录" contains the substring
    assert!(ids.contains(&"e_login_sms".to_string()));
}

#[test]
fn search_by_method_filters_correctly() {
    let pool = fresh_pool();
    let rows = db::with_conn(&pool, |c| search(c, None, Some("GET"), None)).unwrap();
    let methods: Vec<_> = rows.iter().map(|e| e.method.as_str().to_string()).collect();
    assert!(methods.iter().all(|m| m == "GET"));
    // Sanity: at least one GET endpoint seeded
    assert!(!rows.is_empty());
}

#[test]
fn search_by_tag_uses_like_match() {
    let pool = fresh_pool();
    let tag = "登录".to_string();
    let rows = db::with_conn(&pool, |c| {
        search(c, None, None, Some(std::slice::from_ref(&tag)))
    })
    .unwrap();
    let all_have_tag = rows.iter().all(|e| e.tags.iter().any(|t| t == &tag));
    assert!(all_have_tag, "every result must carry the requested tag");
}

#[test]
fn search_combines_criteria() {
    let pool = fresh_pool();
    let tag = "登录".to_string();
    let rows = db::with_conn(&pool, |c| {
        search(c, None, Some("POST"), Some(std::slice::from_ref(&tag)))
    })
    .unwrap();
    assert!(rows
        .iter()
        .all(|e| e.method.as_str() == "POST" && e.tags.contains(&tag)));
}

#[test]
fn update_changes_tags_json_column() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        update(
            c,
            "e_login_sms",
            EndpointPatch {
                tags: Some(vec!["公开".into(), "auth".into()]),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e_login_sms")).unwrap();
    assert_eq!(got.tags, vec!["公开".to_string(), "auth".to_string()]);
}

#[test]
fn update_can_clear_base_url_override() {
    let pool = fresh_pool();
    // First write a value
    db::with_tx(&pool, |c| {
        update(
            c,
            "e_login_sms",
            EndpointPatch {
                base_url_override: Some(Some("http://override".into())),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();
    // Then clear it
    db::with_tx(&pool, |c| {
        update(
            c,
            "e_login_sms",
            serde_json::from_str(r#"{"baseUrlOverride": null}"#).unwrap(),
        )?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "e_login_sms")).unwrap();
    assert!(got.base_url_override.is_none());
}

#[test]
fn list_by_collection_returns_seeded_endpoints() {
    let pool = fresh_pool();
    let rows = db::with_conn(&pool, |c| list_by_collection(c, "c_login")).unwrap();
    let names: Vec<_> = rows.iter().map(|e| e.name.clone()).collect();
    assert!(names.iter().any(|n| n == "短信登录"));
    assert!(names.iter().any(|n| n == "账号密码登录"));
}
