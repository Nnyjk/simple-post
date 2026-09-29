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

// -------------------------------------------------------------------------
// Cross-module move — `module_id` is denormalised onto every collection
// so `list_by_module` can return a whole subtree in one indexed read.
// That makes it a derived value: a move that changes the owning module
// has to re-derive it for the entire subtree, not just the row the
// caller named. The store's optimistic `moduleId` fixup masked the
// missing cascade for as long as nobody reloaded.
// -------------------------------------------------------------------------

#[test]
fn move_across_modules_reassigns_descendant_module_id() {
    let pool = fresh_pool();
    // m_auth holds c_login; nest one child under it so the move has a
    // descendant whose `module_id` can go stale.
    db::with_tx(&pool, |c| {
        let mut sub = sample_collection("c_login_sub", "m_auth", "Sub");
        sub.parent_collection_id = Some("c_login".into());
        create(c, &sub)?;
        Ok(())
    })
    .unwrap();

    // c_login (m_auth) → m_order. Both modules live under p_demo.
    db::with_tx(&pool, |c| {
        move_many(c, &["c_login".to_string()], "module", "m_order", 0)
    })
    .unwrap();

    let moved = db::with_conn(&pool, |c| get(c, "c_login")).unwrap();
    assert_eq!(moved.module_id, "m_order");

    // The regression: the descendant must follow its parent. Before the
    // cascade this stayed "m_auth" and the subtree vanished from the
    // tree on the next read.
    let descendant = db::with_conn(&pool, |c| get(c, "c_login_sub")).unwrap();
    assert_eq!(
        descendant.module_id, "m_order",
        "descendant module_id must be re-derived on a cross-module move"
    );
    assert_eq!(descendant.parent_collection_id.as_deref(), Some("c_login"));
}

#[test]
fn move_across_modules_updates_sibling_listings() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut sub = sample_collection("c_login_sub", "m_auth", "Sub");
        sub.parent_collection_id = Some("c_login".into());
        create(c, &sub)?;
        move_many(c, &["c_login".to_string()], "module", "m_order", 0)
    })
    .unwrap();

    // `list_by_module` is what builds the tree, so both rows have to
    // show up under the new module — including the nested child.
    let order = db::with_conn(&pool, |c| list_by_module(c, "m_order")).unwrap();
    assert!(order.iter().any(|c| c.id == "c_login"));
    assert!(order.iter().any(|c| c.id == "c_login_sub"));

    // And neither may linger under the old one.
    let auth = db::with_conn(&pool, |c| list_by_module(c, "m_auth")).unwrap();
    assert!(!auth.iter().any(|c| c.id == "c_login"));
    assert!(!auth.iter().any(|c| c.id == "c_login_sub"));
}

#[test]
fn move_under_sibling_collection_keeps_module_ownership() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut sub = sample_collection("c_login_sub", "m_auth", "Sub");
        sub.parent_collection_id = Some("c_login".into());
        create(c, &sub)?;
        // c_login → child of c_profile, also in m_auth. No owner change,
        // so nothing outside the moved row should be rewritten.
        move_many(c, &["c_login".to_string()], "collection", "c_profile", 0)
    })
    .unwrap();

    let moved = db::with_conn(&pool, |c| get(c, "c_login")).unwrap();
    assert_eq!(moved.module_id, "m_auth");
    assert_eq!(moved.parent_collection_id.as_deref(), Some("c_profile"));

    let descendant = db::with_conn(&pool, |c| get(c, "c_login_sub")).unwrap();
    assert_eq!(
        descendant.module_id, "m_auth",
        "same-module move must leave the subtree owner untouched"
    );
    assert_eq!(descendant.parent_collection_id.as_deref(), Some("c_login"));
}

#[test]
fn move_rejects_unknown_parent_kind() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        move_many(c, &["c_login".to_string()], "endpoint", "e_login_sms", 0)
    })
    .unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation, got {err:?}"
    );
}

#[test]
fn move_renumbers_siblings_without_gaps() {
    let pool = fresh_pool();
    // c_order_crud(0), c_order_status(1) under m_order. Push c_login in
    // front; all three must come back as a contiguous 0..2 run.
    db::with_tx(&pool, |c| {
        move_many(c, &["c_login".to_string()], "module", "m_order", 0)
    })
    .unwrap();

    let order = db::with_conn(&pool, |c| list_by_module(c, "m_order")).unwrap();
    let orders: Vec<i64> = order.iter().map(|c| c.sort_order).collect();
    assert_eq!(
        orders,
        vec![0, 1, 2],
        "top-level siblings must be renumbered contiguously, got {orders:?}"
    );
    assert_eq!(order[0].id, "c_login", "caller order decides the slot");
}
