use super::*;
use crate::db;

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-envs-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::init(&dir).expect("init")
}

fn sample_env(id: &str, project_id: &str, name: &str) -> Environment {
    Environment {
        id: id.into(),
        project_id: project_id.into(),
        name: name.into(),
        base_urls: HashMap::new(),
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
    env.base_urls
        .insert("def_default".into(), "http://qa".into());
    env.variables.insert("token".into(), "abc".into());
    env.is_active = false;
    db::with_tx(&pool, |c| create(c, &env)).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env1")).unwrap();
    assert_eq!(got.base_urls.get("def_default").unwrap(), "http://qa");
    assert_eq!(got.variables.get("token").unwrap(), "abc");
}

#[test]
fn update_can_replace_base_urls() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        let mut patch = EnvironmentPatch::default();
        let mut new_map = HashMap::new();
        new_map.insert("def_default".into(), "http://new".into());
        patch.base_urls = Some(new_map);
        update(c, "env_dev", patch)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(got.base_urls.get("def_default").unwrap(), "http://new");
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
