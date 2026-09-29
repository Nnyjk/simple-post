use super::*;
use crate::db;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-collections-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

fn sample_collection(id: &str, module_id: &str, name: &str) -> Collection {
    Collection {
        id: id.into(),
        module_id: module_id.into(),
        parent_collection_id: None,
        name: name.into(),
        description: None,
        sort_order: 0,
        expanded: None,
    }
}

#[test]
fn list_by_module_filters_correctly() {
    let pool = fresh_pool();
    let auth = db::with_conn(&pool, |c| list_by_module(c, "m_auth")).unwrap();
    assert_eq!(auth.len(), 2);
    assert!(auth.iter().any(|c| c.id == "c_login"));
    assert!(auth.iter().any(|c| c.id == "c_profile"));
}

#[test]
fn create_nested_collection() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut sub = sample_collection("c_sub", "m_auth", "Sub");
        sub.parent_collection_id = Some("c_login".into());
        create(c, &sub)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "c_sub")).unwrap();
    assert_eq!(got.parent_collection_id.as_deref(), Some("c_login"));
}

#[test]
fn update_can_clear_parent_collection_id() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut sub = sample_collection("c_sub", "m_auth", "Sub");
        sub.parent_collection_id = Some("c_login".into());
        create(c, &sub)?;
        update(
            c,
            "c_sub",
            serde_json::from_str(r#"{"parentCollectionId": null}"#).unwrap(),
        )?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "c_sub")).unwrap();
    assert!(got.parent_collection_id.is_none());
}

#[test]
fn delete_cascades_to_endpoints() {
    let pool = fresh_pool();
    // c_login holds 2 endpoints (e_login_sms, e_login_pwd) seeded.
    db::with_tx(&pool, |c| delete(c, "c_login")).unwrap();
    let n: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM endpoints WHERE collection_id = 'c_login'",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(n, 0);
}
