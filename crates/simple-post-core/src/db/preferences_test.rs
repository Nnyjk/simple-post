use super::*;
use crate::db;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Theme {
    mode: String,
    accent: String,
}

fn fresh_pool() -> db::DbPool {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-prefs-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    db::test_support::init_seeded(&dir)
}

#[test]
fn missing_key_returns_none() {
    let pool = fresh_pool();
    let got: Option<Theme> = db::with_conn(&pool, |c| get(c, "theme")).unwrap();
    assert_eq!(got, None);
}

#[test]
fn set_then_get_round_trips_struct() {
    let pool = fresh_pool();
    let theme = Theme {
        mode: "dark".into(),
        accent: "#fff".into(),
    };
    db::with_tx(&pool, |c| set(c, "theme", &theme)).unwrap();
    let back: Theme = db::with_conn(&pool, |c| get(c, "theme"))
        .unwrap()
        .expect("must be present");
    assert_eq!(back, theme);
}

#[test]
fn set_overwrites_existing_value() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            "theme",
            &Theme {
                mode: "dark".into(),
                accent: "#000".into(),
            },
        )?;
        set(
            c,
            "theme",
            &Theme {
                mode: "light".into(),
                accent: "#fff".into(),
            },
        )?;
        Ok(())
    })
    .unwrap();
    let back: Theme = db::with_conn(&pool, |c| get(c, "theme")).unwrap().unwrap();
    assert_eq!(back.mode, "light");
}

#[test]
fn set_with_primitive_types() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, "last_project", &"p_demo".to_string())).unwrap();
    let back: String = db::with_conn(&pool, |c| get(c, "last_project"))
        .unwrap()
        .unwrap();
    assert_eq!(back, "p_demo");
}

#[test]
fn list_all_returns_every_pair() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        set(c, "a", &"1".to_string())?;
        set(c, "b", &"2".to_string())?;
        Ok(())
    })
    .unwrap();
    let pairs = db::with_conn(&pool, list_all).unwrap();
    let keys: Vec<_> = pairs.iter().map(|(k, _)| k.clone()).collect();
    assert!(keys.contains(&"a".to_string()));
    assert!(keys.contains(&"b".to_string()));
}

#[test]
fn delete_removes_a_key() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, "tmp", &"v".to_string())).unwrap();
    db::with_tx(&pool, |c| delete(c, "tmp")).unwrap();
    let got: Option<String> = db::with_conn(&pool, |c| get(c, "tmp")).unwrap();
    assert_eq!(got, None);
}

#[test]
fn get_with_wrong_type_errors() {
    let pool = fresh_pool();
    // Store a number, try to read as a struct.
    db::with_tx(&pool, |c| set(c, "x", &42_i64)).unwrap();
    let err: Result<Option<Theme>, _> = db::with_conn(&pool, |c| get(c, "x"));
    assert!(err.is_err());
}

#[test]
fn mcp_auto_start_defaults_to_false_when_missing() {
    let pool = fresh_pool();
    let v = db::with_conn(&pool, mcp_auto_start).unwrap();
    assert!(!v, "default must be false — the daemon must not auto-start");
}

#[test]
fn mcp_auto_start_round_trips() {
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, keys::MCP_AUTO_START, &true)).unwrap();
    assert!(db::with_conn(&pool, mcp_auto_start).unwrap());
    db::with_tx(&pool, |c| set(c, keys::MCP_AUTO_START, &false)).unwrap();
    assert!(!db::with_conn(&pool, mcp_auto_start).unwrap());
}

#[test]
fn mcp_policy_defaults_to_read_only_when_missing() {
    let pool = fresh_pool();
    let p = db::with_conn(&pool, mcp_policy).unwrap();
    assert_eq!(
        p,
        crate::permissions::CapabilityPolicy::ReadOnly,
        "missing row must yield ReadOnly — an absent key is not consent"
    );
}

#[test]
fn mcp_policy_round_trips() {
    use crate::permissions::CapabilityPolicy;
    let pool = fresh_pool();
    for policy in [
        CapabilityPolicy::ReadOnly,
        CapabilityPolicy::SafeWrite,
        CapabilityPolicy::FullAccess,
    ] {
        db::with_tx(&pool, |c| set_mcp_policy(c, policy)).unwrap();
        let back = db::with_conn(&pool, mcp_policy).unwrap();
        assert_eq!(back, policy);
    }
}

#[test]
fn mcp_policy_tolerates_garbage_value() {
    // 向前兼容：将来的版本可以发新档位；解析不了的旧客户端仍必须能启动，只是落在安全的
    // ReadOnly 默认值上。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, keys::MCP_POLICY, &"nuclear".to_string())).unwrap();
    let p = db::with_conn(&pool, mcp_policy).unwrap();
    assert_eq!(p, crate::permissions::CapabilityPolicy::ReadOnly);
}

// mcp.http_config

#[test]
fn mcp_http_config_defaults_when_missing() {
    let pool = fresh_pool();
    let c = db::with_conn(&pool, mcp_http_config).unwrap();
    assert_eq!(c, crate::domain::McpHttpConfig::default());
    assert_eq!(c.host, "127.0.0.1");
    assert_eq!(c.port, 0);
    assert_eq!(c.path, "/mcp");
    assert!(
        !c.allow_remote,
        "default must be loopback-only — an absent row is not consent to expose the daemon"
    );
    assert!(c.allowed_hosts.is_empty());
    assert!(c.allowed_origins.is_empty());
}

#[test]
fn mcp_http_config_round_trips_every_field() {
    use crate::domain::McpHttpConfig;
    let pool = fresh_pool();
    let cfg = McpHttpConfig {
        host: "0.0.0.0".into(),
        port: 4173,
        path: "/ai/mcp".into(),
        allow_remote: true,
        allowed_hosts: vec!["studio.local".into(), "10.0.0.5".into()],
        allowed_origins: vec!["https://claude.ai".into()],
    };
    db::with_tx(&pool, |c| set_mcp_http_config(c, &cfg)).unwrap();
    let back = db::with_conn(&pool, mcp_http_config).unwrap();
    assert_eq!(back, cfg);
}

#[test]
fn mcp_http_config_is_stored_as_json_not_bare_text() {
    // `preferences.value` 契约：`set` 跑 `serde_json::to_string`，每个读者都跑 `from_str`。
    // 裸（未加引号的）对象正是本代码库别处会静默退回默认值的形态，故此处把编码钉在列这一层，
    // 而不只靠上面那次往返。
    let pool = fresh_pool();
    let cfg = crate::domain::McpHttpConfig::default();
    db::with_tx(&pool, |c| set_mcp_http_config(c, &cfg)).unwrap();

    let raw = db::with_conn(&pool, list_all)
        .unwrap()
        .into_iter()
        .find(|(k, _)| k == keys::MCP_HTTP_CONFIG)
        .map(|(_, v)| v)
        .expect("row must be present");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("stored value must be JSON");
    assert_eq!(parsed["host"], "127.0.0.1");
    assert_eq!(parsed["path"], "/mcp");
    assert_eq!(parsed["allowRemote"], false);
    assert_eq!(parsed["allowedHosts"].as_array().map(Vec::len), Some(0));
    assert_eq!(parsed["allowedOrigins"].as_array().map(Vec::len), Some(0));
}

#[test]
fn mcp_http_config_missing_fields_fall_back_to_defaults() {
    // 容器级 `#[serde(default)]`：更早构建写下的 blob 必须仍能反序列化，缺席的键取它们各自
    // 有文档的默认值，而不是让整次读失败。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| {
        set(
            c,
            keys::MCP_HTTP_CONFIG,
            &serde_json::json!({
                "host": "0.0.0.0",
                "allowRemote": true,
            }),
        )
    })
    .unwrap();
    let back = db::with_conn(&pool, mcp_http_config).unwrap();
    assert_eq!(back.host, "0.0.0.0");
    assert!(back.allow_remote);
    assert_eq!(back.port, 0);
    assert_eq!(back.path, "/mcp");
    assert!(back.allowed_hosts.is_empty());
}

#[test]
fn mcp_http_config_reports_an_unparseable_row_instead_of_hiding_it() {
    // 手工写入的（未加引号的）值。静默退回默认会把一个暴露在局域网上的守护进程降级成
    // 仅 loopback 且日志里毫无痕迹，故这必须表现为错误。
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, keys::MCP_HTTP_CONFIG, &"0.0.0.0")).unwrap();
    assert!(db::with_conn(&pool, mcp_http_config).is_err());
}
