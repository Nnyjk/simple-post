use super::*;
use std::path::PathBuf;

/// Build a fresh pool rooted at a temp directory, with the bundled
/// fixture set loaded. Each test owns its own dir so they're
/// independent. Returns the dir so the caller can `keep` / `drop`
/// it as desired.
///
/// `db::init` no longer seeds — see [`fresh_empty_pool`] for the
/// un-seeded shape. Tests below assert against `p_demo` and friends, so
/// they opt into the fixtures explicitly.
fn fresh_pool() -> (DbPool, PathBuf) {
    let (pool, dir) = fresh_empty_pool();
    test_support::seed(&pool);
    (pool, dir)
}

/// Same as [`fresh_pool`] but **no rows inserted** — the state a real
/// first boot leaves behind. Use this when the test is about migration
/// or about how the app behaves on an empty database.
fn fresh_empty_pool() -> (DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-db-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    // Make sure we start clean even if a previous run left files.
    let _ = std::fs::remove_dir_all(&dir);
    let pool = init(&dir).expect("init");
    (pool, dir)
}

#[test]
fn init_creates_dir_and_db_file() {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-db-test-init-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let pool = init(&dir).expect("init");
    assert!(dir.exists(), "data dir should exist");
    assert!(dir.join("store.db").exists(), "store.db should exist");

    // `ping` round-trips a connection.
    ping(&pool).expect("ping");
}

#[test]
fn init_leaves_a_fresh_database_empty() {
    // Simple Post ships no seed data: `db::init` creates tables and
    // stops. Rows only ever come from the user or from an MCP client.
    let (pool, _dir) = fresh_empty_pool();
    for table in [
        "projects",
        "nodes",
        "endpoint_details",
        "environments",
        "base_url_definitions",
    ] {
        let count: i64 = with_conn(&pool, |c| {
            Ok(c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
        })
        .unwrap();
        assert_eq!(count, 0, "{table} should be empty after init");
    }
}

#[test]
fn migrate_is_idempotent() {
    let (pool, _dir) = fresh_pool();
    // Second call on the same DB: every CREATE is IF NOT EXISTS, so
    // re-running the schema is a no-op and rows are untouched.
    with_conn(&pool, migrate).expect("second migrate");
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(count, 3, "migrate must not duplicate or drop rows");
}

#[test]
fn with_tx_rolls_back_on_error() {
    let (pool, _dir) = fresh_pool();
    // Force an error inside the closure; the rollback should
    // restore the row count to 3 (the seeded projects).
    let res: Result<(), AppError> = with_tx(&pool, |c| {
        c.execute("DELETE FROM projects", [])?;
        Err(AppError::Validation("nope".into()))
    });
    assert!(res.is_err());
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(count, 3, "rollback should leave projects intact");
}

#[test]
fn with_tx_commits_on_success() {
    let (pool, _dir) = fresh_pool();
    with_tx(&pool, |c| {
        c.execute(
            "INSERT INTO projects (id, name, color, created_at, updated_at) \
                 VALUES ('p_new', 'New', '#fff', 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let count: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(count, 4);
}

#[test]
fn with_conn_returns_caller_value() {
    let (pool, _dir) = fresh_pool();
    let v: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT 42", [], |r| r.get::<_, i64>(0))?)
    })
    .unwrap();
    assert_eq!(v, 42);
}

// -------------------------------------------------------------------------
// Schema versioning.
// -------------------------------------------------------------------------

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-schema-test-{}-{}-{}",
        tag,
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Plant a `store.db` shaped like the pre-versioning layout — the
/// three-table tree, the given `user_version`, and one row in it so a
/// rebuild is observable from the outside.
///
/// `legacy_marker` is a table the current `schema.sql` would never
/// create. It is the phase-independent witness that the file was
/// *replaced* rather than emptied in place: an assertion about which
/// tables the new schema happens to define would keep changing as the
/// layout moves, and would pass even if the rebuild quietly reused the
/// old file.
fn plant_legacy_store(dir: &Path, version: i64) {
    std::fs::create_dir_all(dir).unwrap();
    let conn = Connection::open(dir.join("store.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, \
             color TEXT, created_at INTEGER, updated_at INTEGER);
         CREATE TABLE modules (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, \
             name TEXT NOT NULL, description TEXT, sort_order INTEGER NOT NULL);
         CREATE TABLE legacy_marker (note TEXT);
         INSERT INTO projects VALUES ('p_legacy', 'Legacy', '#000', 1, 1);
         INSERT INTO modules VALUES ('m_legacy', 'p_legacy', 'Legacy Module', NULL, 0);
         INSERT INTO legacy_marker VALUES ('this build must never write a table like this');",
    )
    .unwrap();
    conn.pragma_update(None, "user_version", version).unwrap();
    // Dropped here so the handle is released before `init` tries to
    // delete the file — on Windows an open handle makes removal fail.
}

#[test]
fn schema_version_matches_the_constant() {
    let (pool, _dir) = fresh_empty_pool();
    let v: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(
        v, SCHEMA_VERSION,
        "the PRAGMA at the bottom of schema.sql must agree with SCHEMA_VERSION; \
         bump the constant when the layout changes shape"
    );
}

#[test]
fn init_rebuilds_a_legacy_store() {
    let dir = temp_dir("legacy");
    plant_legacy_store(&dir, 0);

    let pool = init(&dir).expect("init");

    let projects: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects, 0, "legacy rows must not survive a rebuild");

    // The legacy `legacy_marker` table is gone entirely. This is the
    // "the file was replaced" check: emptying the old file in place
    // would have left it behind, and a schema this build no longer
    // understands is a schema it has to keep understanding forever.
    let still_there: i64 = with_conn(&pool, |c| {
        Ok(c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='legacy_marker'",
            [],
            |r| r.get(0),
        )?)
    })
    .unwrap();
    assert_eq!(
        still_there, 0,
        "rebuild must replace the file, not just clear its rows"
    );

    let v: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(v, SCHEMA_VERSION, "the rebuilt store is stamped current");
}

#[test]
fn init_keeps_a_current_store_intact() {
    let (pool, dir) = fresh_pool();
    assert_eq!(
        with_conn(&pool, |c| Ok(c.query_row(
            "SELECT COUNT(*) FROM projects",
            [],
            |r| r.get::<_, i64>(0)
        )?))
        .unwrap(),
        3
    );
    drop(pool);

    // Re-opening a store this build already wrote must not touch it.
    let pool = init(&dir).expect("second init");
    let projects: i64 = with_conn(&pool, |c| {
        Ok(c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?)
    })
    .unwrap();
    assert_eq!(projects, 3, "a current store survives re-init intact");
}

#[test]
fn init_refuses_a_database_from_a_newer_build() {
    let dir = temp_dir("newer");
    plant_legacy_store(&dir, SCHEMA_VERSION + 1);

    let err = init(&dir).expect_err("a newer schema must not be clobbered");
    assert!(
        matches!(err, AppError::Validation(_)),
        "expected Validation, got {err:?}"
    );
    assert!(
        dir.join("store.db").exists(),
        "a downgrade must never delete data it does not understand"
    );
}

#[test]
fn remove_db_files_takes_the_wal_sidecars_with_it() {
    let dir = temp_dir("sidecars");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["store.db", "store.db-wal", "store.db-shm"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    remove_db_files(&dir.join("store.db")).unwrap();

    for name in ["store.db", "store.db-wal", "store.db-shm"] {
        assert!(
            !dir.join(name).exists(),
            "{name} must be gone — a surviving store.db-wal would be replayed \
             into the freshly created store.db"
        );
    }
}

#[test]
fn remove_db_files_tolerates_absent_sidecars() {
    let dir = temp_dir("no-sidecar");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("store.db"), b"x").unwrap();
    // No -wal, no -shm. A failed open elsewhere can leave it that way.
    remove_db_files(&dir.join("store.db")).expect("missing sidecar is not an error");
    assert!(!dir.join("store.db").exists());
}

// ===========================================================================
// Data migration — both predecessor service-list shapes.
//
// The stores below are built with `init` (which runs `migrate` against an
// empty database, a no-op) and then filled with rows written the way each
// predecessor build wrote them. That is a faithful stand-in for a real
// legacy file: the migration reads rows, and the current `CREATE TABLE`
// carries every column either predecessor had — `module_id` and
// `is_default` among them — so the only thing that distinguishes a legacy
// file is the data in it, which is what these tests set.
// ===========================================================================

/// Module → URL map, the shape both `environments` JSON columns use.
type UrlMap = std::collections::BTreeMap<String, String>;
/// def_id → module_id → URL.
type PerModuleMap = std::collections::BTreeMap<String, UrlMap>;

/// A store with the current schema and nothing in it, ready for legacy
/// rows. The data migration has already run once here (against no rows),
/// which is exactly the state a booting app is in when it is handed a
/// database it has to repair.
fn legacy_store(tag: &str) -> DbPool {
    let dir = temp_dir(tag);
    init(&dir).expect("init")
}

fn add_project(conn: &mut Connection, id: &str, star: Option<&str>) {
    conn.execute(
        "INSERT INTO projects (id, name, default_base_url_definition_id, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, '#60a5fa', 0, 0)",
        params![id, id, star],
    )
    .unwrap();
}

fn add_module(conn: &mut Connection, id: &str, project: &str, sort_order: i64, created_at: i64) {
    conn.execute(
        "INSERT INTO nodes (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
         VALUES (?1, ?2, NULL, 'module', ?1, ?3, ?4, ?4)",
        params![id, project, sort_order, created_at],
    )
    .unwrap();
}

/// One service row. `module` is what tells the two shapes apart: `Some`
/// is the module-owned layout, `None` the project-owned one.
fn add_service(
    conn: &mut Connection,
    id: &str,
    project: &str,
    name: &str,
    module: Option<&str>,
    sort_order: i64,
    is_default: bool,
) {
    conn.execute(
        "INSERT INTO base_url_definitions \
         (id, project_id, module_id, name, sort_order, is_default) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, project, module, name, sort_order, is_default as i64],
    )
    .unwrap();
}

fn add_env(
    conn: &mut Connection,
    id: &str,
    project: &str,
    base_urls: &UrlMap,
    module_base_urls: &PerModuleMap,
) {
    conn.execute(
        "INSERT INTO environments \
         (id, project_id, name, is_active, base_urls_json, module_base_urls_json) \
         VALUES (?1, ?2, ?1, 1, ?3, ?4)",
        params![
            id,
            project,
            serde_json::to_string(base_urls).unwrap(),
            serde_json::to_string(module_base_urls).unwrap()
        ],
    )
    .unwrap();
}

/// An endpoint sitting directly under `module` — the tree allows it, and
/// it keeps these fixtures to one node per endpoint.
fn add_endpoint(conn: &mut Connection, id: &str, module: &str, service: Option<&str>, path: &str) {
    conn.execute(
        "INSERT INTO nodes (id, root_id, parent_id, type, name, sort_order, created_at, updated_at) \
         VALUES (?1, (SELECT root_id FROM nodes WHERE id = ?2), ?2, 'endpoint', ?1, 0, 0, 0)",
        params![id, module],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO endpoint_details (node_id, method, url, base_url_definition_id) \
         VALUES (?1, 'GET', ?2, ?3)",
        params![id, path, service],
    )
    .unwrap();
}

/// Which module an endpoint is under. Walks up the tree because an
/// endpoint is at least one edge below its module, and a collection in
/// between must not stop the walk.
fn module_of(conn: &mut Connection, node_id: &str) -> Option<String> {
    conn.query_row(
        "WITH RECURSIVE up(id, parent_id, type) AS ( \
             SELECT id, parent_id, type FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT p.id, p.parent_id, p.type FROM nodes p JOIN up ON p.id = up.parent_id \
         ) \
         SELECT id FROM up WHERE type = 'module' LIMIT 1",
        params![node_id],
        |r| r.get(0),
    )
    .ok()
}

fn endpoint_service(conn: &mut Connection, node_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT base_url_definition_id FROM endpoint_details WHERE node_id = ?1",
        params![node_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .unwrap()
}

fn project_star(conn: &mut Connection, project_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT default_base_url_definition_id FROM projects WHERE id = ?1",
        params![project_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn read_env_maps(conn: &mut Connection, env_id: &str) -> (UrlMap, PerModuleMap) {
    conn.query_row(
        "SELECT base_urls_json, module_base_urls_json FROM environments WHERE id = ?1",
        params![env_id],
        |r| {
            Ok((
                serde_json::from_str::<UrlMap>(&r.get::<_, String>(0)?).unwrap(),
                serde_json::from_str::<PerModuleMap>(&r.get::<_, String>(1)?).unwrap(),
            ))
        },
    )
    .unwrap()
}

/// How the **current** model resolves an endpoint's base URL: the
/// project's ★ when the endpoint names no service, then that service's
/// cell for the endpoint's own module, and nothing else.
fn resolve_current(
    conn: &mut Connection,
    project_id: &str,
    env_id: &str,
    endpoint_id: &str,
) -> String {
    let def = endpoint_service(conn, endpoint_id).or_else(|| project_star(conn, project_id));
    let Some(def) = def else {
        return String::new();
    };
    let Some(module) = module_of(conn, endpoint_id) else {
        return String::new();
    };
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM base_url_definitions WHERE id = ?1 AND project_id = ?2)",
            params![def, project_id],
            |r| r.get(0),
        )
        .unwrap();
    if !exists {
        return String::new();
    }
    read_env_maps(conn, env_id)
        .1
        .get(&def)
        .and_then(|per_module| per_module.get(&module))
        .cloned()
        .unwrap_or_default()
}

/// How the **shape A** (project-owned list) layout resolved it: the
/// per-module override first, the environment-level URL second.
fn resolve_legacy_project_owned(
    conn: &mut Connection,
    project_id: &str,
    endpoint_id: &str,
    env_id: &str,
) -> String {
    let def = endpoint_service(conn, endpoint_id).or_else(|| project_star(conn, project_id));
    let Some(def) = def else {
        return String::new();
    };
    let Some(module) = module_of(conn, endpoint_id) else {
        return String::new();
    };
    let (base_urls, overrides) = read_env_maps(conn, env_id);
    overrides
        .get(&def)
        .and_then(|per_module| per_module.get(&module))
        .or_else(|| base_urls.get(&def))
        .cloned()
        .unwrap_or_default()
}

/// How the **shape B** (module-owned lists) layout resolved it: the
/// service the endpoint named, or its own module's ★, straight out of the
/// environment-level map.
fn resolve_legacy_module_owned(conn: &mut Connection, endpoint_id: &str, env_id: &str) -> String {
    let module = module_of(conn, endpoint_id);
    let def = endpoint_service(conn, endpoint_id).or_else(|| {
        let module = module.clone()?;
        conn.query_row(
            "SELECT id FROM base_url_definitions \
             WHERE module_id = ?1 AND is_default = 1 \
             ORDER BY sort_order ASC, id ASC LIMIT 1",
            params![module],
            |r| r.get(0),
        )
        .ok()
    });
    let Some(def) = def else {
        return String::new();
    };
    read_env_maps(conn, env_id)
        .0
        .get(&def)
        .cloned()
        .unwrap_or_default()
}

/// Endpoints of a project, with the URL each layout would resolve.
fn urls_under_current(
    conn: &mut Connection,
    project_id: &str,
    env_id: &str,
) -> Vec<(String, String)> {
    let ids = {
        let mut stmt = conn
            .prepare("SELECT id FROM nodes WHERE root_id = ?1 AND type = 'endpoint' ORDER BY id")
            .unwrap();
        let ids = stmt
            .query_map(params![project_id], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        ids
    };
    ids.into_iter()
        .map(|id| {
            let url = resolve_current(conn, project_id, env_id, &id);
            (id, url)
        })
        .collect()
}

/// Everything the migration is allowed to touch, as raw strings, so two
/// runs can be compared byte for byte.
fn snapshot(conn: &mut Connection, project_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, module_id, name, sort_order, is_default FROM base_url_definitions \
                 WHERE project_id = ?1 ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "def|{}|{:?}|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, base_urls_json, module_base_urls_json FROM environments \
                 WHERE project_id = ?1 ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "env|{}|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    {
        let mut stmt = conn
            .prepare(
                "SELECT e.node_id, IFNULL(e.base_url_definition_id, '-') \
                 FROM endpoint_details e JOIN nodes n ON n.id = e.node_id \
                 WHERE n.root_id = ?1 ORDER BY e.node_id",
            )
            .unwrap();
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(format!(
                    "ep|{}|{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                ))
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        out.extend(rows);
    }
    out.push(format!("star|{:?}", project_star(conn, project_id)));
    out
}

// -------------------------------------------------------------------------
// Shape A — the project-owned service list.
// -------------------------------------------------------------------------

/// The real pre-v1 layout: one service list per project (`module_id`
/// NULL), values in `base_urls_json[def_id]` with a per-module override
/// in `module_base_urls_json[def_id][module_id]`.
fn seed_shape_a(conn: &mut Connection) {
    add_project(conn, "p_a", Some("d_base"));
    // The user's own store is the model for the ids and timestamps: two
    // modules created minutes apart, `sort_order` 0 and 1.
    add_module(conn, "m_air", "p_a", 0, 1_791_617_412_693);
    add_module(conn, "m_water", "p_a", 1, 1_791_621_861_037);
    add_service(conn, "d_base", "p_a", "base", None, 0, false);
    add_service(conn, "d_air", "p_a", "air", None, 1, false);

    let base_urls: UrlMap = [("d_base".to_string(), "http://127.0.0.1:9200".to_string())]
        .into_iter()
        .collect();
    let overrides: PerModuleMap = [(
        "d_base".to_string(),
        [("m_air".to_string(), "http://127.0.0.1:9201".to_string())]
            .into_iter()
            .collect(),
    )]
    .into_iter()
    .collect();
    add_env(conn, "e_a_prod", "p_a", &base_urls, &overrides);

    add_endpoint(conn, "a_named_air", "m_air", Some("d_base"), "/air/health");
    add_endpoint(
        conn,
        "a_named_water",
        "m_water",
        Some("d_base"),
        "/water/health",
    );
    // No service named: these resolve through the project ★, and are here
    // to prove that path survives too.
    add_endpoint(conn, "a_star_air", "m_air", None, "/star/air");
    add_endpoint(conn, "a_star_water", "m_water", None, "/star/water");
    // A service no environment ever gave a value to: the answer is empty
    // before and after, and the cell must stay absent rather than become
    // an empty string.
    add_endpoint(conn, "a_unset", "m_air", Some("d_air"), "/air/unset");
}

#[test]
fn shape_a_urls_are_byte_identical_after_migration() {
    let pool = legacy_store("shape-a-urls");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        let legacy = [
            "a_named_air",
            "a_named_water",
            "a_star_air",
            "a_star_water",
            "a_unset",
        ]
        .iter()
        .map(|id| {
            (
                id.to_string(),
                resolve_legacy_project_owned(c, "p_a", id, "e_a_prod"),
            )
        })
        .collect::<Vec<_>>();
        let before = urls_under_current(c, "p_a", "e_a_prod");
        migrate(c)?;
        let after = urls_under_current(c, "p_a", "e_a_prod");
        assert_eq!(
            before.len(),
            legacy.len(),
            "the same endpoints must exist on both sides of the migration"
        );
        assert_eq!(
            after, legacy,
            "every endpoint must resolve to exactly the URL it did before"
        );
        // Spot-check the two interesting cells rather than trusting the
        // comparison above to have exercised them at all.
        assert_eq!(
            after[0].1, "http://127.0.0.1:9201",
            "override wins for m_air"
        );
        assert_eq!(
            after[1].1, "http://127.0.0.1:9200",
            "m_water has no override, so it uses the environment value"
        );
        assert_eq!(
            after[4].1, "",
            "a service with no value resolves to nothing"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_puts_every_value_in_the_module_keyed_layer() {
    let pool = legacy_store("shape-a-cells");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        migrate(c)?;
        let (_, cells) = read_env_maps(c, "e_a_prod");
        let per_module = cells.get("d_base").expect("d_base must have cells");
        assert_eq!(per_module.get("m_air").unwrap(), "http://127.0.0.1:9201");
        assert_eq!(per_module.get("m_water").unwrap(), "http://127.0.0.1:9200");
        assert!(
            !cells.contains_key("d_air"),
            "a service with no value anywhere gets no cells at all"
        );
        assert_eq!(
            project_star(c, "p_a").as_deref(),
            Some("d_base"),
            "the project ★ is already the answer shape A was using"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_leaves_the_environment_level_urls_in_place() {
    // Contract §4.6: shape A's `base_urls_json` is left as it was. It is
    // the one layout where that is safe, because re-running the migration
    // re-derives the identical values from it.
    let pool = legacy_store("shape-a-keeps-base-urls");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        migrate(c)?;
        let (base_urls, _) = read_env_maps(c, "e_a_prod");
        assert_eq!(base_urls.get("d_base").unwrap(), "http://127.0.0.1:9200");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_a_star_stays_empty_when_the_project_had_none() {
    // Electing a ★ here would hand endpoints that resolved to `''` a host
    // they never had, so a NULL default survives the migration as NULL.
    let pool = legacy_store("shape-a-null-star");
    with_conn(&pool, |c| {
        seed_shape_a(c);
        c.execute(
            "UPDATE projects SET default_base_url_definition_id = NULL WHERE id = 'p_a'",
            [],
        )
        .unwrap();
        migrate(c)?;
        assert_eq!(project_star(c, "p_a"), None);
        Ok(())
    })
    .unwrap();
}

// -------------------------------------------------------------------------
// Shape B — the module-owned service lists.
// -------------------------------------------------------------------------

/// The layout the module-owned build left behind, modelled on a real user
/// store: module `air` holds `air` / `base`(★) / 「新服务」, module
/// `water` holds only 「新服务」(★), and the environment's URL map is keyed
/// by whichever clone each module created.
fn seed_shape_b(conn: &mut Connection) {
    add_project(conn, "p_b", None);
    add_module(conn, "m_air", "p_b", 0, 1_791_617_412_693);
    add_module(conn, "m_water", "p_b", 1, 1_791_621_861_037);

    add_service(conn, "sp_air_air", "p_b", "air", Some("m_air"), 0, false);
    add_service(conn, "sp_air_base", "p_b", "base", Some("m_air"), 1, true);
    add_service(conn, "sp_air_new", "p_b", "新服务", Some("m_air"), 2, false);
    add_service(
        conn,
        "sp_water_new",
        "p_b",
        "新服务",
        Some("m_water"),
        0,
        true,
    );

    // Two live URLs plus the orphan keys a real store accumulates: ids of
    // services deleted in the UI, which no longer resolve to anything.
    let base_urls: UrlMap = [
        (
            "sp_air_base".to_string(),
            "http://127.0.0.1:9200".to_string(),
        ),
        (
            "sp_water_new".to_string(),
            "http://127.0.0.1:9300".to_string(),
        ),
        ("sp_deleted_1".to_string(), String::new()),
        ("sp_deleted_2".to_string(), String::new()),
    ]
    .into_iter()
    .collect();
    add_env(conn, "e_b_prod", "p_b", &base_urls, &PerModuleMap::new());

    add_endpoint(
        conn,
        "b_named_air",
        "m_air",
        Some("sp_air_base"),
        "/air/health",
    );
    add_endpoint(
        conn,
        "b_named_water",
        "m_water",
        Some("sp_water_new"),
        "/water/health",
    );
    add_endpoint(conn, "b_unset", "m_air", Some("sp_air_air"), "/air/unset");
}

#[test]
fn shape_b_urls_are_byte_identical_after_migration() {
    let pool = legacy_store("shape-b-urls");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        let legacy = ["b_named_air", "b_named_water", "b_unset"]
            .iter()
            .map(|id| {
                (
                    id.to_string(),
                    resolve_legacy_module_owned(c, id, "e_b_prod"),
                )
            })
            .collect::<Vec<_>>();
        migrate(c)?;
        let after = urls_under_current(c, "p_b", "e_b_prod");
        assert_eq!(
            after, legacy,
            "every endpoint must resolve to exactly the URL it did before"
        );
        assert_eq!(after[0].1, "http://127.0.0.1:9200");
        assert_eq!(
            after[1].1, "http://127.0.0.1:9300",
            "m_water's own clone carried the URL and must not be lost"
        );
        assert_eq!(after[2].1, "", "a clone with no URL resolves to nothing");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_collapses_each_service_name_to_one_project_level_row() {
    let pool = legacy_store("shape-b-collapse");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let names: Vec<(String, Option<String>)> = {
            let mut stmt = c
                .prepare(
                    "SELECT name, module_id FROM base_url_definitions \
                     WHERE project_id = 'p_b' ORDER BY sort_order ASC, id ASC",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows
        };
        assert_eq!(
            names.len(),
            3,
            "「新服务」 existed under both modules and must survive once"
        );
        for (name, module) in &names {
            assert!(module.is_none(), "{name} still owns a module: {module:?}");
        }
        // air is the first module, so its rows are the survivors.
        assert_eq!(names[1].0, "base");
        let (_, cells) = read_env_maps(c, "e_b_prod");
        assert_eq!(
            cells.get("sp_air_base").unwrap().get("m_air").unwrap(),
            "http://127.0.0.1:9200"
        );
        assert_eq!(
            cells.get("sp_air_new").unwrap().get("m_water").unwrap(),
            "http://127.0.0.1:9300",
            "water's value lands on the survivor's row, under water"
        );
        assert!(
            !cells.contains_key("sp_water_new"),
            "the collapsed clone's id must not survive as a key"
        );
        assert!(
            !cells.get("sp_air_base").unwrap().contains_key("m_water"),
            "an empty cell stays empty: water never had a `base` service"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_conflicting_stars_resolve_to_the_first_module() {
    // air stars 「base」 and water stars 「新服务」. There is no
    // inheritance between modules, so neither is "the" answer; the
    // tie-break is the first module, stated rather than derived.
    let pool = legacy_store("shape-b-star-conflict");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        assert_eq!(
            project_star(c, "p_b").as_deref(),
            Some("sp_air_base"),
            "the first module's ★ wins, by module display order"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_star_falls_back_to_the_first_service_when_the_first_module_has_none() {
    let pool = legacy_store("shape-b-star-fallback");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        c.execute("UPDATE base_url_definitions SET is_default = 0", [])
            .unwrap();
        migrate(c)?;
        assert_eq!(
            project_star(c, "p_b").as_deref(),
            Some("sp_air_air"),
            "with no ★ anywhere, the first service in display order takes it"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_neutralises_the_columns_it_retired() {
    let pool = legacy_store("shape-b-neutralise");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let (owned, starred): (i64, i64) = c
            .query_row(
                "SELECT COUNT(*) FILTER (WHERE module_id IS NOT NULL), \
                        COUNT(*) FILTER (WHERE is_default != 0) \
                 FROM base_url_definitions WHERE project_id = 'p_b'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(owned, 0, "every row must give up its module");
        assert_eq!(starred, 0, "every row must give up its per-module ★");
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_clears_the_urls_it_moved() {
    // Every value in `base_urls_json` is now in the module-keyed layer,
    // so leaving it would make the next boot read it as shape A and copy
    // air's host into every module's empty cell.
    let pool = legacy_store("shape-b-clears-base-urls");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        let (base_urls, _) = read_env_maps(c, "e_b_prod");
        assert!(
            base_urls.is_empty(),
            "the dead column must not keep answering the next boot: {base_urls:?}"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn shape_b_remaps_endpoints_onto_the_surviving_rows() {
    let pool = legacy_store("shape-b-remap");
    with_conn(&pool, |c| {
        seed_shape_b(c);
        migrate(c)?;
        assert_eq!(
            endpoint_service(c, "b_named_air").as_deref(),
            Some("sp_air_base"),
            "the survivor of air's own row is that row"
        );
        assert_eq!(
            endpoint_service(c, "b_named_water").as_deref(),
            Some("sp_air_new"),
            "water's 「新服务」 collapsed onto air's row of the same name"
        );
        let dangling: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM endpoint_details e \
                 LEFT JOIN base_url_definitions d ON d.id = e.base_url_definition_id \
                 WHERE e.base_url_definition_id IS NOT NULL AND d.id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0, "no endpoint may point at a deleted clone");
        Ok(())
    })
    .unwrap();
}

// -------------------------------------------------------------------------
// Idempotency.
// -------------------------------------------------------------------------

#[test]
fn migrating_twice_changes_nothing_in_either_shape() {
    // (temp-dir tag, project id, fixture builder)
    type Shape = (&'static str, &'static str, fn(&mut Connection));
    let shapes: [Shape; 2] = [
        ("twice-a", "p_a", seed_shape_a),
        ("twice-b", "p_b", seed_shape_b),
    ];
    for (tag, project_id, seed) in shapes {
        let pool = legacy_store(tag);
        with_conn(&pool, |c| {
            seed(c);
            migrate(c)?;
            let after_first = snapshot(c, project_id);
            assert!(
                !after_first.is_empty(),
                "{tag}: fixture rows are missing, the comparison below would pass vacuously"
            );
            migrate(c)?;
            assert_eq!(
                snapshot(c, project_id),
                after_first,
                "{tag}: a second migrate must be a no-op"
            );
            // And a third, for the shape where the guard has to notice
            // the store is already repaired.
            migrate(c)?;
            assert_eq!(snapshot(c, project_id), after_first, "{tag}: third run");
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn a_current_store_is_left_alone() {
    // The seeded fixtures are already in the target shape, so migrating
    // them must not touch a row — that is what makes the pass free on
    // every boot after the first.
    let (pool, _dir) = fresh_pool();
    with_conn(&pool, |c| {
        let before = snapshot(c, "p_demo");
        migrate(c)?;
        assert_eq!(snapshot(c, "p_demo"), before);
        Ok(())
    })
    .unwrap();
}
