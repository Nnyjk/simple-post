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
        module_base_urls: HashMap::new(),
        variables: HashMap::new(),
        disabled_variables: Vec::new(),
        is_active: false,
    }
}

/// 一格地址。`proxy` 缺省表示这一格没有代理。
fn cell(url: &str) -> ModuleBaseUrl {
    ModuleBaseUrl {
        url: url.into(),
        proxy: None,
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
        .insert("m_auth".into(), cell("http://qa"));
    env.variables.insert("token".into(), "abc".into());
    env.is_active = false;
    db::with_tx(&pool, |c| create(c, &env)).unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env1")).unwrap();
    assert_eq!(got.module_base_urls.get("m_auth").unwrap().url, "http://qa");
    assert_eq!(got.variables.get("token").unwrap(), "abc");
}

#[test]
fn seeded_module_base_urls_come_back_per_module() {
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(
        got.module_base_urls.get("m_order").unwrap().url,
        "http://localhost:4000/demo/order",
        "每个模块各自有一格地址，这正是值层按模块分格的原因"
    );
    assert_eq!(
        got.module_base_urls.get("m_auth").unwrap().url,
        "http://localhost:4000/demo/auth"
    );
    assert!(
        !got.module_base_urls.contains_key("m_title"),
        "fixture 没给发票项目写过地址，那一格必须缺席"
    );
}

#[test]
fn the_seeded_proxy_comes_back_with_its_cell() {
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    let pay = got.module_base_urls.get("m_pay").unwrap();
    assert_eq!(pay.url, "http://localhost:4000/demo/pay");
    assert_eq!(
        pay.proxy.as_deref(),
        Some("https://gw.example.com/demo/pay"),
        "格子里的代理地址要跟着格子一起存下来"
    );
    assert!(
        got.module_base_urls.get("m_auth").unwrap().proxy.is_none(),
        "没有代理的格子读回来是 None"
    );
}

#[test]
fn set_module_base_url_writes_one_cell_and_leaves_its_siblings() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_pay", Some("http://pay"), None)
    })
    .unwrap();
    let pay = env.module_base_urls.get("m_pay").unwrap();
    assert_eq!(pay.url, "http://pay");
    assert_eq!(
        pay.proxy.as_deref(),
        Some("https://gw.example.com/demo/pay"),
        "只写地址时，这一格原有的代理地址必须保持不变"
    );
    assert_eq!(
        env.module_base_urls.get("m_auth").unwrap().url,
        "http://localhost:4000/demo/auth",
        "别的模块那一格必须不动"
    );
    // And durable, not merely echoed.
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(got.module_base_urls.get("m_pay").unwrap().url, "http://pay");
}

#[test]
fn set_module_base_url_overwrites_rather_than_appending() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_auth", Some("http://v2"), None)
    })
    .unwrap();
    assert_eq!(env.module_base_urls.get("m_auth").unwrap().url, "http://v2");
    assert_eq!(
        env.module_base_urls.len(),
        3,
        "only the three fixture modules"
    );
}

#[test]
fn writing_only_the_proxy_leaves_the_address_alone() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_auth", None, Some("https://gw/auth"))
    })
    .unwrap();
    let auth = env.module_base_urls.get("m_auth").unwrap();
    assert_eq!(
        auth.url, "http://localhost:4000/demo/auth",
        "只写代理时，这一格原有的地址必须保持不变"
    );
    assert_eq!(auth.proxy.as_deref(), Some("https://gw/auth"));
}

#[test]
fn clearing_the_proxy_keeps_the_address() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_pay", None, Some(""))
    })
    .unwrap();
    let pay = env.module_base_urls.get("m_pay").unwrap();
    assert_eq!(pay.url, "http://localhost:4000/demo/pay");
    assert!(
        pay.proxy.is_none(),
        "空串清掉代理，但这一格还有地址，故这一格留着"
    );
}

#[test]
fn clearing_a_cell_does_not_borrow_a_sibling_modules_value() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_pay", Some(""), None)
    })
    .unwrap();
    let pay = env.module_base_urls.get("m_pay").unwrap();
    assert_eq!(pay.url, "", "地址被清空，这一格没有地址可解析");
    assert_eq!(
        pay.proxy.as_deref(),
        Some("https://gw.example.com/demo/pay"),
        "清地址不动代理"
    );
    assert_eq!(
        env.module_base_urls.get("m_auth").unwrap().url,
        "http://localhost:4000/demo/auth",
        "别的模块那一格的值不能被借过来"
    );
}

#[test]
fn clearing_the_last_cell_drops_the_module_entirely() {
    let pool = fresh_pool();
    // 一个既没有地址也没有代理的格子解析不出任何东西，留着只会让按键遍历的读法误判成
    // 「此处已配置」。
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_dev", "m_auth", Some(""), Some(""))?;
        set_module_base_url(c, "env_dev", "m_order", Some(""), Some(""))?;
        set_module_base_url(c, "env_dev", "m_pay", Some(""), Some(""))
    })
    .unwrap();
    assert!(
        !env.module_base_urls.contains_key("m_auth"),
        "这一格的两个字段都空了，模块这一项必须一起消失"
    );
    assert!(!env.module_base_urls.contains_key("m_order"));
    assert!(!env.module_base_urls.contains_key("m_pay"));
}

#[test]
fn set_module_base_url_can_add_a_module_the_environment_never_had() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_staging", "m_auth", Some("http://stage"), None)
    })
    .unwrap();
    assert_eq!(
        env.module_base_urls.get("m_auth").unwrap().url,
        "http://stage"
    );
}

#[test]
fn set_module_base_url_missing_environment_is_not_found() {
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| {
        set_module_base_url(c, "env_ghost", "m_auth", Some("http://x"), None)
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
        new_map.insert("m_auth".to_string(), cell("http://new"));
        patch.module_base_urls = Some(new_map);
        update(c, "env_dev", patch)?;
        Ok(())
    })
    .unwrap();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(
        got.module_base_urls.len(),
        1,
        "the patch replaces the whole map"
    );
    assert_eq!(
        got.module_base_urls.get("m_auth").unwrap().url,
        "http://new"
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
    // 对堵住「global」权限绕过很关键：工具拿一个伪造的 env_id 调 lookup_project_id
    // 绝不能拿回任何项目（因而也拿不到任何权限授予）。
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| lookup_project_id(c, "env_ghost")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn lookup_project_id_does_not_match_empty_string() {
    // 空 env_id 是最容易尝试的绕过——确保我们不会顺手接受它。
    let pool = fresh_pool();
    let err = db::with_conn(&pool, |c| lookup_project_id(c, "")).unwrap_err();
    assert_eq!(err.code(), "not_found");
}

#[test]
fn rename_var_moves_key_and_keeps_its_value() {
    // env_dev seed 了 {token, env}。改名必须是对 variables_json 的一次原子 UPDATE：否则那个
    // 变量会存在一个它根本不存在的窗口。
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

#[test]
fn a_disabled_variable_survives_the_round_trip() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", true)).unwrap();
    assert_eq!(env.disabled_variables, vec!["token".to_string()]);
    // 值还在：停用的是「不参与替换」，不是「删掉」。
    assert_eq!(
        env.variables.get("token").unwrap(),
        "eyJhbGciOiJIUzI1NiJ9.dev"
    );

    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert_eq!(got.disabled_variables, vec!["token".to_string()]);
}

#[test]
fn enabling_a_variable_removes_it_from_the_disabled_set() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", true)).unwrap();
    let env = db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", false)).unwrap();
    assert!(env.disabled_variables.is_empty());
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert!(got.disabled_variables.is_empty());
}

#[test]
fn renaming_a_variable_carries_its_disabled_flag_along() {
    // 改名后它若静默变成启用的，就等于「改个名字就把它放开了」。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", true)).unwrap();
    let env = db::with_tx(&pool, |c| rename_var(c, "env_dev", "token", "access_token")).unwrap();
    assert_eq!(
        env.disabled_variables,
        vec!["access_token".to_string()],
        "停用状态跟着键一起搬"
    );
}

#[test]
fn deleting_a_variable_drops_it_from_the_disabled_set() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", true)).unwrap();
    let env = db::with_tx(&pool, |c| set_var(c, "env_dev", "token", None)).unwrap();
    assert!(env.disabled_variables.is_empty());
    assert!(!env.variables.contains_key("token"));
}

#[test]
fn disabling_a_variable_that_does_not_exist_is_a_validation_error() {
    // 静默成功会让调用方以为它生效了。
    let pool = fresh_pool();
    let err = db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "nope", true)).unwrap_err();
    assert_eq!(err.code(), "validation");
}

#[test]
fn an_environment_without_the_column_reads_back_an_empty_disabled_set() {
    // 老库没有这一列：`db::ADDED_COLUMNS` 补上，语义等价于「全部启用」。
    let pool = fresh_pool();
    let got = db::with_conn(&pool, |c| get(c, "env_dev")).unwrap();
    assert!(
        got.disabled_variables.is_empty(),
        "一个没有被停用的环境读回来是空集合"
    );
}

#[test]
fn the_patch_can_replace_the_whole_disabled_set() {
    let pool = fresh_pool();
    let env = db::with_tx(&pool, |c| {
        update(
            c,
            "env_dev",
            EnvironmentPatch {
                disabled_variables: Some(vec!["token".into(), "env".into()]),
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(
        env.disabled_variables,
        vec!["env".to_string(), "token".to_string()]
    );
}

#[test]
fn a_patch_without_the_field_leaves_the_disabled_set_alone() {
    // 部分更新：缺席 = 不碰。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set_var_disabled(c, "env_dev", "token", true)).unwrap();
    let env = db::with_tx(&pool, |c| {
        update(
            c,
            "env_dev",
            EnvironmentPatch {
                name: Some("renamed".into()),
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(env.name, "renamed");
    assert_eq!(
        env.disabled_variables,
        vec!["token".to_string()],
        "改名字不该动停用集合"
    );
}
