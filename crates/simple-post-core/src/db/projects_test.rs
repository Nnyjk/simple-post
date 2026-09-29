use super::*;
use crate::db;
use crate::domain::BaseUrlDefinition;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-projects-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

fn sample_project(id: &str, name: &str) -> Project {
    Project {
        id: id.to_string(),
        name: name.to_string(),
        description: Some("desc".into()),
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: "#000".into(),
        created_at: 1000,
        updated_at: 2000,
    }
}

#[test]
fn create_and_get_round_trip() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "Project One"))?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert_eq!(got.name, "Project One");
    assert_eq!(got.color, "#000");
    assert_eq!(got.created_at, 1000);
}

#[test]
fn get_missing_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| get(c, "missing")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn list_returns_seeded_projects() {
    let pool = fresh_pool();
    let ps = db::with_conn(&pool, list).unwrap();
    // fixtures.json seeds 3 projects
    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0].id, "p_demo");
}

#[test]
fn list_populates_base_url_definitions_from_join() {
    let pool = fresh_pool();
    let ps = db::with_conn(&pool, list).unwrap();
    let demo = ps.iter().find(|p| p.id == "p_demo").unwrap();
    // fixtures seed 1 default base_url_def for p_demo
    assert_eq!(demo.base_url_definitions.len(), 1);
    assert_eq!(demo.base_url_definitions[0].id, "def_default");
    assert_eq!(demo.base_url_definitions[0].name, "默认");
}

#[test]
fn update_partial_only_changes_provided_fields() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "Original"))?;
        update(
            c,
            "p1",
            ProjectPatch {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert_eq!(got.name, "Renamed");
    assert_eq!(got.description.as_deref(), Some("desc"));
    assert!(got.updated_at >= 2000, "updated_at should bump on patch");
}

#[test]
fn update_can_clear_a_nullable_field_via_double_none() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "x"))?;
        update(
            c,
            "p1",
            serde_json::from_str(r#"{"description": null}"#).unwrap(),
        )?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert!(got.description.is_none(), "explicit null clears the column");
}

#[test]
fn update_sets_default_base_url_definition_id() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        create(c, &sample_project("p1", "x"))?;
        update(
            c,
            "p1",
            serde_json::from_str(r#"{"defaultBaseUrlDefinitionId": "def_new"}"#).unwrap(),
        )?;
        Ok(())
    })
    .unwrap();

    let got = db::with_conn(&pool, |c| get(c, "p1")).unwrap();
    assert_eq!(
        got.default_base_url_definition_id.as_deref(),
        Some("def_new")
    );
}

#[test]
fn delete_removes_project_and_cascades_to_the_whole_tree() {
    let pool = fresh_pool();
    // One module, one nested collection, one endpoint under it, plus the
    // endpoint's configuration row. Deleting the project has to take all
    // four: `nodes.root_id` cascades from `projects`, `nodes.parent_id`
    // cascades down the tree, and `endpoint_details` cascades from its
    // node. Asserting only the module would pass even if the deeper
    // levels leaked.
    db::with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('m_extra', 'p_demo', NULL, 'module', 'extra', 99, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('c_extra', 'p_demo', 'm_extra', 'collection', 'c', 0, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO nodes \
                 (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
                 VALUES ('e_extra', 'p_demo', 'c_extra', 'endpoint', 'e', 0, 0, 0)",
            [],
        )?;
        c.execute(
            "INSERT INTO endpoint_details (node_id, method, url) \
                 VALUES ('e_extra', 'GET', '/x')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // `endpoint_details` keys on `node_id`; `nodes` on `id`. Same shape
    // of question — "is this row still here?" — for both tables.
    let count = |table: &str, col: &str, id: &str| {
        db::with_conn(&pool, |c| {
            let n: i64 = c.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {col} = ?1"),
                [id],
                |r| r.get(0),
            )?;
            Ok(n)
        })
        .unwrap()
    };
    let node = |id: &str| count("nodes", "id", id);
    let detail = |id: &str| count("endpoint_details", "node_id", id);

    assert_eq!(node("m_extra"), 1);
    assert_eq!(node("c_extra"), 1);
    assert_eq!(node("e_extra"), 1);
    assert_eq!(detail("e_extra"), 1);

    db::with_tx(&pool, |c| delete(c, "p_demo")).unwrap();

    assert_eq!(
        node("m_extra"),
        0,
        "ON DELETE CASCADE should drop child rows"
    );
    assert_eq!(node("c_extra"), 0);
    assert_eq!(node("e_extra"), 0);
    assert_eq!(
        detail("e_extra"),
        0,
        "the endpoint's configuration must not outlive its node"
    );
}

#[test]
fn delete_missing_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| delete(c, "ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn get_returns_default_def_id_from_row() {
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "p_demo")).unwrap();
    assert_eq!(
        got.default_base_url_definition_id.as_deref(),
        Some("def_default")
    );
}

#[test]
fn round_trip_via_base_url_definitions_struct() {
    // Smoke test: serialise + deserialise a Project with defs and
    // confirm the wire form matches the TS interface.
    let p = Project {
        id: "p1".into(),
        name: "x".into(),
        description: None,
        base_url: None,
        base_url_definitions: vec![BaseUrlDefinition {
            id: "d".into(),
            name: "默认".into(),
            description: None,
        }],
        default_base_url_definition_id: Some("d".into()),
        color: "#fff".into(),
        created_at: 1,
        updated_at: 2,
    };
    let s = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&s).unwrap();
    assert_eq!(back.base_url_definitions.len(), 1);
    assert_eq!(back.base_url_definitions[0].name, "默认");
}
