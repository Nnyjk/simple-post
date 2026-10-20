//! 脚本链的单元测试：顺序、失败语义、链内共享。

use super::*;

fn collection(id: &str, name: &str, pre: Option<&str>, test: Option<&str>) -> ScriptUnit {
    ScriptUnit {
        source: ScriptSource::Collection {
            id: id.to_string(),
            name: name.to_string(),
        },
        pre_request: pre.map(str::to_string),
        test: test.map(str::to_string),
    }
}

fn endpoint(pre: Option<&str>, test: Option<&str>) -> ScriptUnit {
    ScriptUnit {
        source: ScriptSource::Endpoint {
            id: "e1".to_string(),
            name: "端点".to_string(),
        },
        pre_request: pre.map(str::to_string),
        test: test.map(str::to_string),
    }
}

fn request() -> ScriptRequest {
    ScriptRequest {
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        headers: vec![],
        body: None,
    }
}

fn limits() -> ScriptLimits {
    ScriptLimits {
        timeout_ms: 5_000,
        ..DEFAULT_SCRIPT_LIMITS
    }
}

#[test]
fn the_chain_runs_outermost_collection_first() {
    let units = vec![
        collection("c1", "外层", Some("console.log('外层');"), None),
        collection("c2", "内层", Some("console.log('内层');"), None),
        endpoint(Some("console.log('端点');"), None),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    assert!(outcome.errors.is_empty());
    let order: Vec<&str> = outcome.logs.iter().map(|l| l.message.as_str()).collect();
    assert_eq!(order, vec!["外层", "内层", "端点"]);
}

#[test]
fn the_test_phase_uses_the_same_order_as_the_pre_phase() {
    let units = vec![
        collection("c1", "外层", None, Some("console.log('外层');")),
        collection("c2", "内层", None, Some("console.log('内层');")),
        endpoint(None, Some("console.log('端点');")),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Test,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    let order: Vec<&str> = outcome.logs.iter().map(|l| l.message.as_str()).collect();
    assert_eq!(order, vec!["外层", "内层", "端点"]);
}

#[test]
fn a_blank_script_is_not_part_of_the_chain() {
    let units = vec![
        collection("c1", "外层", Some("   \n\t "), None),
        endpoint(Some("console.log('端点');"), None),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    assert_eq!(outcome.logs.len(), 1);
    assert_eq!(outcome.logs[0].message, "端点");
}

#[test]
fn a_failing_script_does_not_stop_the_rest_of_the_chain() {
    let units = vec![
        collection("c1", "外层", Some("console.log('外层');"), None),
        collection("c2", "坏段", Some("throw new Error('boom');"), None),
        endpoint(Some("console.log('端点');"), None),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    assert_eq!(outcome.errors.len(), 1);
    assert_eq!(outcome.errors[0].source, "坏段");
    assert_eq!(outcome.errors[0].kind, ScriptErrorKind::Runtime);
    // 链继续跑完了。
    let order: Vec<&str> = outcome.logs.iter().map(|l| l.message.as_str()).collect();
    assert_eq!(order, vec!["外层", "端点"]);
}

#[test]
fn a_later_script_sees_what_an_earlier_one_changed() {
    let units = vec![
        collection(
            "c1",
            "外层",
            Some("pm.request.url = 'http://changed.test/';"),
            None,
        ),
        endpoint(Some("console.log(pm.request.url);"), None),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    assert!(outcome.errors.is_empty());
    assert_eq!(outcome.logs[0].message, "http://changed.test/");
    assert_eq!(request.url, "http://changed.test/");
}

#[test]
fn a_chain_without_scripts_produces_nothing() {
    let units = vec![collection("c1", "外层", None, None), endpoint(None, None)];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    assert!(outcome.logs.is_empty());
    assert!(outcome.assertions.is_empty());
    assert!(outcome.errors.is_empty());
    assert!(outcome.env_writes.is_empty());
}

#[test]
fn the_env_writes_of_the_whole_chain_are_collected() {
    let units = vec![
        collection("c1", "外层", Some("pm.environment.set('a', '1');"), None),
        endpoint(Some("pm.environment.set('b', '2');"), None),
    ];

    let mut request = request();
    let outcome = run_phase(
        ScriptPhase::Pre,
        &units,
        &mut request,
        None,
        &EnvSnapshot::default(),
        &limits(),
    );

    let keys: Vec<&str> = outcome.env_writes.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(keys, vec!["a", "b"]);
}
