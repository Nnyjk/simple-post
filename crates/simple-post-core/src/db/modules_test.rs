use super::*;
use crate::db;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-modules-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(&dir).expect("init")
}

fn sample_module(id: &str, project_id: &str, name: &str) -> Module {
    Module {
        id: id.into(),
        project_id: project_id.into(),
        name: name.into(),
        description: None,
        sort_order: 0,
        expanded: None,
    }
}

#[test]
fn create_and_list() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_module("m_new", "p_demo", "New"))?;
        Ok(())
    })
    .unwrap();
    let ms = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
    // 3 seeded modules (m_auth, m_order, m_pay) under p_demo + new
    assert!(ms.iter().any(|m| m.id == "m_new"));
    assert!(ms.iter().any(|m| m.id == "m_auth"));
}

#[test]
fn update_renames_a_module() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_module("m1", "p_demo", "Old"))?;
        update(
            c,
            "m1",
            ModulePatch {
                name: Some("New".into()),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();
    let m = db::with_conn(&pool, |c| get(c, "m1")).unwrap();
    assert_eq!(m.name, "New");
}

#[test]
fn delete_cascades_to_collections_and_endpoints() {
    let pool = fresh_pool();
    // p_demo → m_auth → c_login → e_login_sms is the seed chain.
    let before: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM endpoints", [], |r| r.get(0))?)
    })
    .unwrap();
    assert!(before > 0);

    db::with_tx(&pool, |c| delete(c, "m_auth")).unwrap();

    let after: i64 = db::with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM endpoints", [], |r| r.get(0))?)
    })
    .unwrap();
    // 4 endpoints lived under m_auth: e_login_sms, e_login_pwd, e_user_get, e_user_update
    assert_eq!(after, before - 4);
}

/// Counts (collections, endpoints) reachable under `module_id`, used by the
/// re-parent regression tests below.
fn child_counts(conn: &mut rusqlite::Connection, module_id: &str) -> (i64, i64) {
    let collections: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM collections WHERE module_id = ?1",
            params![module_id],
            |r| r.get(0),
        )
        .unwrap();
    let endpoints: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM endpoints WHERE collection_id IN \
                 (SELECT id FROM collections WHERE module_id = ?1)",
            params![module_id],
            |r| r.get(0),
        )
        .unwrap();
    (collections, endpoints)
}

#[test]
fn reparent_moves_module_and_keeps_its_id() {
    // A move must be an UPDATE, not a delete + re-create: re-creating
    // would mint a new module id and break every caller holding a
    // reference to the old one.
    let pool = fresh_pool();
    let moved = db::with_tx(&pool, |c| reparent(c, "m_auth", "p_invoice", None)).unwrap();
    assert_eq!(moved.id, "m_auth");
    assert_eq!(moved.project_id, "p_invoice");

    let in_new = db::with_conn(&pool, |c| list_by_project(c, "p_invoice")).unwrap();
    assert!(in_new.iter().any(|m| m.id == "m_auth"));
    let in_old = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
    assert!(!in_old.iter().any(|m| m.id == "m_auth"));
}

#[test]
fn reparent_preserves_collections_and_endpoints() {
    // The data-loss regression `reparent` exists to prevent. Both
    // `collections.module_id` and `endpoints.collection_id` are
    // `ON DELETE CASCADE`, so the old delete-then-re-create implementation
    // silently wiped every collection under the module and every
    // endpoint under those collections.
    let pool = fresh_pool();
    let before = db::with_conn(&pool, |c| Ok(child_counts(c, "m_auth"))).unwrap();
    assert!(
        before.0 > 0 && before.1 > 0,
        "seed must have children to lose"
    );

    db::with_tx(&pool, |c| reparent(c, "m_auth", "p_invoice", None)).unwrap();

    let after = db::with_conn(&pool, |c| Ok(child_counts(c, "m_auth"))).unwrap();
    assert_eq!(
        after, before,
        "cross-project move must not destroy collections or endpoints"
    );
}

#[test]
fn reparent_pins_sort_order_only_when_asked() {
    let pool = fresh_pool();
    let original = db::with_conn(&pool, |c| get(c, "m_auth")).unwrap();

    let untouched = db::with_tx(&pool, |c| reparent(c, "m_auth", "p_invoice", None)).unwrap();
    assert_eq!(
        untouched.sort_order, original.sort_order,
        "new_sort_order = None must leave the existing order alone"
    );

    let pinned = db::with_tx(&pool, |c| reparent(c, "m_auth", "p_iot", Some(7))).unwrap();
    assert_eq!(pinned.sort_order, 7);
    assert_eq!(pinned.project_id, "p_iot");
}

#[test]
fn reparent_missing_module_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| reparent(c, "m_ghost", "p_invoice", None)).unwrap_err();
    assert_eq!(err.code(), "not_found");
}
