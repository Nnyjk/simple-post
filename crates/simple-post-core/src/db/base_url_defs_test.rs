use super::*;
use crate::db;
use crate::domain::BaseUrlDefinition;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-base-url-defs-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

#[test]
fn list_by_project_returns_seeded_defs() {
    let pool = fresh_pool();
    let defs = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].id, "def_default");
}

#[test]
fn create_and_get() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(
            c,
            &BaseUrlDefinition {
                id: "def_auth".into(),
                name: "鉴权".into(),
                description: Some("OAuth 入口".into()),
            },
            "p_demo",
        )?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "def_auth")).unwrap();
    assert_eq!(got.name, "鉴权");
    assert_eq!(got.description.as_deref(), Some("OAuth 入口"));
}

#[test]
fn update_renames() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        update(c, "def_default", Some("Default".into()), None)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "def_default")).unwrap();
    assert_eq!(got.name, "Default");
}

#[test]
fn update_can_clear_description() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        update(c, "def_default", None, Some(None))?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "def_default")).unwrap();
    assert!(got.description.is_none());
}

#[test]
fn delete_removes_def() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    let res = db::with_conn(&pool, |c| get(c, "def_default"));
    assert!(res.is_err());
}

#[test]
fn delete_clears_a_star_that_pointed_at_it() {
    // p_demo 出厂就以 def_default 为 ★。留着那个指针会让项目继续宣传一个已不存在的服务。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
    assert_eq!(proj.default_base_url_definition_id, None);
}

#[test]
fn delete_leaves_other_stars_alone() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(
            c,
            &BaseUrlDefinition {
                id: "def_alt".into(),
                name: "Alt".into(),
                description: None,
            },
            "p_demo",
        )?;
        set_default(c, "p_demo", Some("def_alt"))?;
        Ok(())
    })
    .unwrap();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
    assert_eq!(
        proj.default_base_url_definition_id.as_deref(),
        Some("def_alt"),
        "only the star of the deleted service is taken down"
    );
}

#[test]
fn delete_missing_returns_not_found_and_changes_nothing() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| delete(c, "def_ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
    // savepoint 回滚了，故那个真正的 ★ 在这次失败的调用之后仍在。
    let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
    assert_eq!(
        proj.default_base_url_definition_id.as_deref(),
        Some("def_default")
    );
}

#[test]
fn set_default_updates_project_pointer() {
    let pool = fresh_pool();
    // Add a second def under p_demo, then point at it.
    db::with_tx(&pool, |c| {
        create(
            c,
            &BaseUrlDefinition {
                id: "def_alt".into(),
                name: "Alt".into(),
                description: None,
            },
            "p_demo",
        )?;
        set_default(c, "p_demo", Some("def_alt"))?;
        Ok(())
    })
    .unwrap();
    let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
    assert_eq!(
        proj.default_base_url_definition_id.as_deref(),
        Some("def_alt")
    );
}

#[test]
fn set_default_can_clear_with_none() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        set_default(c, "p_demo", None)?;
        Ok(())
    })
    .unwrap();
    let proj = db::with_conn(&pool, |c| db::projects::get(c, "p_demo")).unwrap();
    assert!(proj.default_base_url_definition_id.is_none());
}

#[test]
fn set_default_unknown_project_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| set_default(c, "p_ghost", Some("def_default"))).unwrap_err();
    assert_eq!(err.code(), "not_found");
}
