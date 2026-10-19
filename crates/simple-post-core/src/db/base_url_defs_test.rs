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
fn delete_clears_the_service_cells_in_every_environment() {
    // 服务删了，它在各环境里填过的地址也必须一起失效。留着就是孤儿键：解析读不到
    // 它们（服务已死），JSON 却只增不减——而删除确认框答应过「一并失效」。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    for env_id in ["env_dev", "env_staging", "env_prod"] {
        let env = db::with_conn(&pool, |c| db::environments::get(c, env_id)).unwrap();
        assert!(
            !env.module_base_urls.contains_key("def_default"),
            "{env_id} still holds a cell for the deleted service"
        );
    }
}

#[test]
fn delete_keeps_the_cells_of_other_services() {
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
        db::environments::set_module_base_url(
            c,
            "env_dev",
            "def_alt",
            "m_auth",
            Some("http://alt"),
        )?;
        Ok(())
    })
    .unwrap();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    let env = db::with_conn(&pool, |c| db::environments::get(c, "env_dev")).unwrap();
    assert_eq!(
        env.module_base_urls
            .get("def_alt")
            .and_then(|m| m.get("m_auth")),
        Some(&"http://alt".to_string()),
        "the surviving service keeps its own cell"
    );
    assert!(!env.module_base_urls.contains_key("def_default"));
}

#[test]
fn delete_does_not_touch_another_projects_environments() {
    // 清理按项目找环境：别的项目的环境里即使有同名服务的格子也不该被牵动。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(
            c,
            &BaseUrlDefinition {
                id: "def_invoice".into(),
                name: "发票服务".into(),
                description: None,
            },
            "p_invoice",
        )?;
        db::environments::create(
            c,
            &crate::domain::Environment {
                id: "env_invoice".into(),
                project_id: "p_invoice".into(),
                name: "prod".into(),
                base_urls: Default::default(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                is_active: false,
            },
        )?;
        db::environments::set_module_base_url(
            c,
            "env_invoice",
            "def_invoice",
            "m_auth",
            Some("http://invoice"),
        )?;
        Ok(())
    })
    .unwrap();
    db::with_tx(&pool, |c| delete(c, "def_default")).unwrap();
    let env = db::with_conn(&pool, |c| db::environments::get(c, "env_invoice")).unwrap();
    assert_eq!(
        env.module_base_urls
            .get("def_invoice")
            .and_then(|m| m.get("m_auth")),
        Some(&"http://invoice".to_string()),
        "another project's environment is none of this project's business"
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
