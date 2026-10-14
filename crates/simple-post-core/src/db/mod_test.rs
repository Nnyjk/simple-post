use super::*;
use std::path::PathBuf;

/// 建一个以临时目录为根的新连接池，并灌入内嵌 fixture 集。每个测试独享自己的目录，故彼此
/// 独立。返回目录，好让调用方按需 `keep` / `drop`。
/// 下面断言 `p_demo` 之类的测试显式选用 fixture；未播种的形状见 [`fresh_empty_pool`]。
fn fresh_pool() -> (DbPool, PathBuf) {
    let (pool, dir) = fresh_empty_pool();
    test_support::seed(&pool);
    (pool, dir)
}

/// 同 [`fresh_pool`] 但**不插入任何行**——真实首次启动留下的状态。测迁移、或测应用在空库上
/// 行为时用它。
fn fresh_empty_pool() -> (DbPool, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-db-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    // 即使上一次运行留下了文件也确保从干净状态开始。
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

    // `ping` 会真正走一次连接。
    ping(&pool).expect("ping");
}

#[test]
fn init_leaves_a_fresh_database_empty() {
    // Simple Post 不带任何种子数据：`db::init` 建完表即止。行只来自用户或 MCP 客户端。
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
    // 同一个库上第二次调用：每个 CREATE 都是 IF NOT EXISTS，故重跑 schema 是无操作且行不受影响。
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
    // 在闭包里制造错误；回滚应把行数恢复到 3（seed 出来的项目数）。
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

// schema 版本。

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

/// 种一个形状像版本号之前布局的 `store.db`——三表树、给定的 `user_version`，再加一行，
/// 使重建可从外部观察。
/// `legacy_marker` 是当前 `schema.sql` 绝不会创建的表。它是与阶段无关的证据，证明文件是被
/// **替换**而非就地清空的：断言「新 schema 恰好定义了哪些表」会随布局演进不断变化，而且
/// 即便重建悄悄复用了旧文件也照样会通过。
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
    // 在此处 drop，好让句柄在 `init` 试图删除文件之前释放——Windows 上打开的句柄会让删除失败。
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

    // 旧的 `legacy_marker` 表彻底消失。这就是「文件被替换」的检查：就地清空旧文件会把它留下，
    // 而一个本构建已不再理解的 schema，是它必须永远继续理解下去的东西。
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

    // 重新打开一个本构建已经写过的库，必须不碰它。
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
    // 没有 -wal，没有 -shm。别处的打开失败就可能留下这种状态。
    remove_db_files(&dir.join("store.db")).expect("missing sidecar is not an error");
    assert!(!dir.join("store.db").exists());
}

// 数据迁移——两种前任服务表形状。
//
// 下面的库是用 `init` 建的（对空库跑 `migrate`，无操作），再按每个前任构建各自的写法填入行。
// 这对真实的旧文件是忠实的替身：迁移读的是行，而当前的 `CREATE TABLE` 带着两个前任都有的
// 每一列——`module_id` 与 `is_default` 也在内——故唯一区分「旧文件」的就是里面的数据，
// 而那正是这些测试所设置的。

/// 模块 → URL map，两个 `environments` JSON 列共用的形状。
type UrlMap = std::collections::BTreeMap<String, String>;
/// def_id → module_id → URL。
type PerModuleMap = std::collections::BTreeMap<String, UrlMap>;

/// 当前 schema 且空无一行的库，等着放旧行。数据迁移在此已经跑过一次（对零行），这正是
/// 一个启动中的应用被交上一个必须修复的数据库时所处的状态。
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

/// 一条服务行。`module` 正是区分两种形状的东西：`Some` 是模块持有布局，`None` 是项目持有。
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

/// 直接挂在 `module` 下的 endpoint——树允许这样，且它让这些 fixture 每个 endpoint 只用一个节点。
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

/// 某 endpoint 挂在哪个模块下。沿树上溯，因为 endpoint 至少比它的模块低一条边，而中间夹
/// 一个 collection 不该终止这次上溯。
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

/// **当前**模型如何解析一个 endpoint 的 base URL：endpoint 未指名服务时用项目的 ★，然后是
/// 该服务在 endpoint 自己模块下的单元格，别无其他。
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

/// **shape A**（项目持有列表）布局当年如何解析：先按模块覆盖，再环境级 URL。
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

/// **shape B**（模块持有列表）布局当年如何解析：endpoint 指名的服务，或它自己模块的 ★，
/// 直接取自环境级 map。
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

/// 一个项目下的 endpoint，以及每种布局各自会解析出的 URL。
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

/// 迁移被允许触碰的一切，以原始字符串给出，好让两次运行能逐字节比对。
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

// shape A——项目持有的服务表。

/// 真实的 pre-v1 布局：每项目一份服务列表（`module_id` 为 NULL），值在
/// `base_urls_json[def_id]`，按模块的覆盖在 `module_base_urls_json[def_id][module_id]`。
fn seed_shape_a(conn: &mut Connection) {
    add_project(conn, "p_a", Some("d_base"));
    // id 与时间戳以用户自己的库为模型：相隔几分钟创建的两个模块，`sort_order` 0 与 1。
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
    // 未指名服务：这些走项目 ★ 解析，放进来是为了证明那条路径也活着。
    add_endpoint(conn, "a_star_air", "m_air", None, "/star/air");
    add_endpoint(conn, "a_star_water", "m_water", None, "/star/water");
    // 一个任何环境都没给过值的服务：迁移前后答案都是空，且该单元格必须保持缺席，
    // 而不是变成空字符串。
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
        // 抽查那两个有意思的单元格，而不是相信上面的比较真的碰到了它们。
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
    // 契约 §4.6：shape A 的 `base_urls_json` 保持原样。那是这一布局中留它安全的唯一情形，
    // 因为重跑迁移会从它重新推导出完全相同的值。
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
    // 在这里选出一个 ★ 会给原本解析为 `''` 的 endpoint 一个它从未有过的 host，
    // 故 NULL 默认在迁移后仍是 NULL。
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

// shape B——模块持有的服务列表。

/// 模块持有构建留下的布局，以一个真实用户库为模型：模块 `air` 持有 `air` / `base`(★) /
/// 「新服务」，模块 `water` 只持有「新服务」(★)，而环境的 URL map 以各模块各自创建的
/// clone 为键。
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

    // 两个现役 URL，外加真实库会积累的孤儿键：在 UI 里被删掉、已解析不出任何东西的服务 id。
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
        // air 是第一个模块，故它的行是存活者。
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
    // air 给「base」打 ★ 而 water 给「新服务」打 ★。模块之间没有继承，故哪个都不是「那个」
    // 答案；决胜规则就是第一个模块，这是明示的而非推导出的。
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
    // `base_urls_json` 里的每个值现在都在以模块为键的那一层，故留着它会让下次启动把它读成
    // shape A，并把 air 的 host 复制进每个模块的空单元格。
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

// 幂等性。

#[test]
fn migrating_twice_changes_nothing_in_either_shape() {
    // (临时目录 tag, 项目 id, fixture 构造函数)
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
            // 再来第三次，为的是那个守卫必须注意到库已被修好的形状。
            migrate(c)?;
            assert_eq!(snapshot(c, project_id), after_first, "{tag}: third run");
            Ok(())
        })
        .unwrap();
    }
}

#[test]
fn a_current_store_is_left_alone() {
    // seed 出来的 fixture 已经是目标形状，故对它们跑迁移不得碰到任何一行——这正是首轮之后
    // 每次启动都零成本的原因。
    let (pool, _dir) = fresh_pool();
    with_conn(&pool, |c| {
        let before = snapshot(c, "p_demo");
        migrate(c)?;
        assert_eq!(snapshot(c, "p_demo"), before);
        Ok(())
    })
    .unwrap();
}
