use super::*;
use crate::db;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-envs-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

fn sample_env(id: &str, project_id: &str, name: &str) -> Environment {
    Environment {
        id: id.into(),
        project_id: project_id.into(),
        name: name.into(),
        base_urls: HashMap::new(),
        module_base_urls: HashMap::new(),
        variables: HashMap::new(),
        is_active: false,
    }
}

#[test]
fn list_by_project_returns_seeded_envs() {
    let pool = fresh_pool();
    let envs = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
    // fixtures seed 3 envs (dev/staging/prod) under p_demo
    assert_eq!(envs.len(), 3);
}

#[test]
fn get_active_returns_dev_from_seeds() {
    let pool = fresh_pool();
    let active = db::with_conn(&pool, |c| get_active(c, "p_demo")).unwrap();
    let env = active.expect("fixtures seed env_dev with is_active=true");
    assert_eq!(env.id, "env_dev");
}

#[test]
fn set_active_demotes_others_atomically() {
    let pool = fresh_pool();
    // Initially env_dev is active. Promote env_staging.
    db::with_tx(&pool, |c| set_active(c, "env_staging")).unwrap();
    let active = db::with_conn(&pool, |c| get_active(c, "p_demo")).unwrap();
    assert_eq!(active.unwrap().id, "env_staging");

    // env_dev must be inactive now
    let envs = db::with_conn(&pool, |c| list_by_project(c, "p_demo")).unwrap();
    let dev = envs.iter().find(|e| e.id == "env_dev").unwrap();
    let staging = envs.iter().find(|e| e.id == "env_staging").unwrap();
    let prod = envs.iter().find(|e| e.id == "env_prod").unwrap();
    assert!(!dev.is_active);
    assert!(staging.is_active);
    assert!(!prod.is_active);
}

#[test]
fn set_active_missing_returns_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| set_active(c, "env_ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn round_trip_preserves_hashmap() {
    let pool = fresh_pool();
    let mut env = sample_env("env1", "p_demo", "qa");
    env.module_base_urls
        .entry("def_default".into())
        .or_default()
        .insert("m_auth".into(), "http://qa".into());
    env.variables.insert("token".into(), "abc".into());
    env.is_active = false;
    db::with_tx(&pool, |c| create(c, &env)).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env1")).unwrap();
    assert_eq!(
        got.module_base_urls
            .get("def_default")
            .unwrap()
            .get("m_auth")
            .unwrap(),
        "http://qa"
    );
    assert_eq!(got.variables.get("token").unwrap(), "abc");
    // The retired column must not be resurrected by a caller that still
    // fills in the legacy field: it is written only by the schema default.
    assert!(got.base_urls.is_empty());
}

#[test]
fn seeded_module_base_urls_come_back_per_module() {
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    let per_module = got.module_base_urls.get("def_default").unwrap();
    assert_eq!(
        per_module.get("m_order").unwrap(),
        "http://localhost:4000/demo/order",
        "one service carries a different host per module — that is the \
         whole reason the value layer is keyed by module"
    );
    assert_eq!(
        per_module.get("m_auth").unwrap(),
        "http://localhost:4000/demo/auth"
    );
    assert!(
        !got.module_base_urls.contains_key("def_invoice_default"),
        "a project the fixture never gave an env must contribute nothing"
    );
}

#[test]
fn set_module_base_url_writes_one_cell_and_leaves_its_siblings() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "def_default", "m_pay", Some("http://pay"))
    })
    .unwrap();
    let per_module = env.module_base_urls.get("def_default").unwrap();
    assert_eq!(per_module.get("m_pay").unwrap(), "http://pay");
    assert_eq!(
        per_module.get("m_auth").unwrap(),
        "http://localhost:4000/demo/auth",
        "the other modules of the same service must be untouched"
    );
    // And durable, not merely echoed.
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(
        got.module_base_urls
            .get("def_default")
            .unwrap()
            .get("m_pay")
            .unwrap(),
        "http://pay"
    );
}

#[test]
fn set_module_base_url_overwrites_rather_than_appending() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "def_default", "m_auth", Some("http://v2"))
    })
    .unwrap();
    let per_module = env.module_base_urls.get("def_default").unwrap();
    assert_eq!(per_module.get("m_auth").unwrap(), "http://v2");
    assert_eq!(per_module.len(), 3, "only the three fixture modules");
}

#[test]
fn clearing_a_cell_does_not_borrow_a_sibling_modules_value() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "def_default", "m_pay", None)
    })
    .unwrap();
    let per_module = env.module_base_urls.get("def_default").unwrap();
    assert!(
        !per_module.contains_key("m_pay"),
        "an empty cell is the answer, not a pointer to m_auth's host"
    );
    assert!(per_module.contains_key("m_auth"));
}

#[test]
fn clearing_the_last_cell_drops_the_service_key_entirely() {
    let pool = fresh_pool();
    // A service nobody in the environment has an address for should not
    // linger as an empty object in the stored JSON — that is what makes
    // "not set" distinguishable from "set to nothing" on disk.
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "def_default", "m_auth", None)?;
        set_module_base_url(c, "env_dev", "def_default", "m_order", None)?;
        set_module_base_url(c, "env_dev", "def_default", "m_pay", None)
    })
    .unwrap();
    assert!(
        !env.module_base_urls.contains_key("def_default"),
        "the service key must disappear with its last cell"
    );
}

#[test]
fn set_module_base_url_can_add_a_service_the_environment_never_had() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(
            c,
            "env_staging",
            "def_default",
            "m_auth",
            Some("http://stage"),
        )
    })
    .unwrap();
    assert_eq!(
        env.module_base_urls
            .get("def_default")
            .unwrap()
            .get("m_auth")
            .unwrap(),
        "http://stage"
    );
}

#[test]
fn set_module_base_url_missing_environment_is_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_ghost", "def_default", "m_auth", Some("http://x"))
    })
    .unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn update_can_replace_module_base_urls() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut patch = EnvironmentPatch::default();
        let mut new_map = HashMap::new();
        new_map.insert("m_auth".to_string(), "http://new".to_string());
        let mut per_service = HashMap::new();
        per_service.insert("def_default".to_string(), new_map);
        patch.module_base_urls = Some(per_service);
        update(c, "env_dev", patch)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    let per_module = got.module_base_urls.get("def_default").unwrap();
    assert_eq!(per_module.len(), 1, "the patch replaces the whole map");
    assert_eq!(per_module.get("m_auth").unwrap(), "http://new");
}

#[test]
fn update_ignores_the_retired_base_urls_field() {
    // `baseUrls` still deserialises — an old client's payload must not
    // fail — but writing it would resurrect a dead layer, so the patch is
    // accepted and dropped.
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut patch = EnvironmentPatch::default();
        let mut dead = HashMap::new();
        dead.insert("def_default".to_string(), "http://ghost".to_string());
        patch.base_urls = Some(dead);
        update(c, "env_dev", patch)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert!(got.base_urls.is_empty(), "the dead column stays empty");
    assert!(
        got.module_base_urls
            .get("def_default")
            .unwrap()
            .contains_key("m_auth"),
        "the live layer must be untouched by a dead field"
    );
}

#[test]
fn lookup_project_id_returns_owner_of_seeded_env() {
    let pool = fresh_pool();
    let pid = db::with_conn(&pool, |c| lookup_project_id(c, "env_dev")).unwrap();
    assert_eq!(pid, "p_demo");
}

#[test]
fn lookup_project_id_returns_not_found_for_unknown_env() {
    // Critical for closing the "global" permission-bypass: a tool
    // calling lookup_project_id on a forged env_id must NOT get
    // back any project (and thus any permission grant).
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| lookup_project_id(c, "env_ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn lookup_project_id_does_not_match_empty_string() {
    // An empty env_id is the easiest bypass attempt — make sure
    // we don't accidentally accept it.
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| lookup_project_id(c, "")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn rename_var_moves_key_and_keeps_its_value() {
    // env_dev seeds {token, env}. The rename must be one atomic
    // UPDATE of variables_json: the renderer used to model it as
    // set_var(from, None) + set_var(to, value), leaving a window
    // where the variable did not exist at all.
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| rename_var(c, "env_dev", "token", "access_token")).unwrap();
    assert_eq!(
        env.variables.get("access_token").unwrap(),
        "eyJhbGciOiJIUzI1NiJ9.dev",
        "the value must survive the rename"
    );
    assert!(!env.variables.contains_key("token"));
    assert_eq!(
        env.variables.get("env").unwrap(),
        "dev",
        "sibling keys must be untouched"
    );

    // And it must be durable, not just correct in the echoed row.
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(
        got.variables.get("access_token").unwrap(),
        "eyJhbGciOiJIUzI1NiJ9.dev"
    );
    assert!(!got.variables.contains_key("token"));
}

#[test]
fn rename_var_missing_from_key_is_validation_error() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| rename_var(c, "env_dev", "nope", "other")).unwrap_err();
    assert_eq!(err.code(), "validation");
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert!(got.variables.contains_key("token"), "nothing may be lost");
    assert!(!got.variables.contains_key("other"));
}

#[test]
fn rename_var_existing_to_key_refuses_and_keeps_both_keys() {
    // Renaming `token` -> `env` would clobber a live variable, so the
    // call must be refused and the stored map left exactly as it was.
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| rename_var(c, "env_dev", "token", "env")).unwrap_err();
    assert_eq!(err.code(), "validation");
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(
        got.variables.get("token").unwrap(),
        "eyJhbGciOiJIUzI1NiJ9.dev"
    );
    assert_eq!(got.variables.get("env").unwrap(), "dev");
}

#[test]
fn rename_var_same_key_is_validation_error() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| rename_var(c, "env_dev", "token", "token")).unwrap_err();
    assert_eq!(err.code(), "validation");
}

#[test]
fn rename_var_missing_environment_is_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| rename_var(c, "env_ghost", "a", "b")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}
