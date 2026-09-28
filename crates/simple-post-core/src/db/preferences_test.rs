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
    db::init(&dir).expect("init")
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
    assert!(!v, "default must be false — see docs/distribution-modes §5");
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
        "missing row must yield ReadOnly — see docs/mcp-permission-policy.md"
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
    // Forward-compat: a future release can ship a new tier;
    // older clients that can't parse it must still boot, just at
    // the safe ReadOnly default.
    let pool = fresh_pool();
    db::with_tx(&pool, |c| set(c, keys::MCP_POLICY, &"nuclear".to_string())).unwrap();
    let p = db::with_conn(&pool, mcp_policy).unwrap();
    assert_eq!(p, crate::permissions::CapabilityPolicy::ReadOnly);
}
