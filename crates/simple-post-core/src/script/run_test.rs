//! 脚本链的单元测试：顺序、失败语义、链内共享。

use super::*;

use crate::domain::BodyMode;

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

/// 链上的一段脚本，`label` 是它在链里的名字。
fn unit(label: &str, pre: Option<&str>, test: Option<&str>) -> ScriptUnit {
    ScriptUnit {
        source: ScriptSource::Collection {
            id: label.to_string(),
            name: label.to_string(),
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
        body_mode: BodyMode::None,
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
fn a_failed_script_does_not_leak_its_environment_writes_to_the_next_script() {
    // 链上的脚本共用一个沙箱：失败脚本留下的变量会被下一段读到，并可能随它的写入落库。
    let units = vec![
        unit(
            "A",
            Some("pm.environment.set('leaked', 'yes'); throw new Error('boom');"),
            None,
        ),
        unit(
            "B",
            Some("console.log(String(pm.environment.get('leaked')));"),
            None,
        ),
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
    // 失败脚本的写入被丢弃：下一段脚本读到的仍是「没有这个变量」。
    assert_eq!(outcome.logs[0].message, "undefined");
    assert!(
        outcome.env_writes.is_empty(),
        "失败脚本的变量写入不该被记成产出"
    );
}

#[test]
fn a_failing_script_does_not_leak_its_request_scoped_variables_either() {
    let units = vec![
        unit(
            "A",
            Some("pm.variables.set('leaked', 'yes'); throw new Error('boom');"),
            None,
        ),
        unit(
            "B",
            Some("console.log(String(pm.variables.get('leaked')));"),
            None,
        ),
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

    assert_eq!(outcome.logs[0].message, "undefined");
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

#[test]
fn a_script_whose_state_cannot_be_snapshotted_is_not_run_at_all() {
    // 拍不下快照就没有回滚的基准。此时**这段脚本一次都不执行**：照跑的话它写下的变量会留在
    // 沙箱里，链上的下一段读到，并可能随它自己的写入落库——正是回滚要挡的那条路。
    //
    // 第一段把钩子删掉（它自己拍快照时钩子还在，故正常执行），第二段因此拍不下快照。
    let units = vec![
        unit(
            "A",
            Some("pm.environment.set('kept', 'yes'); delete globalThis.__SP_SNAPSHOT_STATE__;"),
            None,
        ),
        unit(
            "B",
            Some("console.log('B ran'); pm.environment.set('b', '1');"),
            None,
        ),
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

    // A 成功的写入照常被记下。
    let keys: Vec<&str> = outcome.env_writes.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(keys, vec!["kept"]);
    // B 什么都没做：没有日志、没有写入，失败指向它。
    assert!(outcome.logs.is_empty(), "B 不该被执行：{:?}", outcome.logs);
    assert_eq!(outcome.errors.len(), 1);
    assert_eq!(outcome.errors[0].source, "B");
    assert_eq!(outcome.errors[0].kind, ScriptErrorKind::Runtime);
    assert!(
        outcome.errors[0].message.contains("cannot snapshot"),
        "got {}",
        outcome.errors[0].message
    );
}

#[test]
fn a_failed_rollback_is_reported_in_the_logs_and_keeps_the_original_failure() {
    // 回滚失败不能 panic（release 是 `panic = "abort"`，脚本路径上的 panic 会带走整个宿主），
    // 也不能静默——那会让「沙箱里还留着失败脚本的值」无从发现。脚本自己的失败照原样返回。
    let units = vec![unit(
        "A",
        Some(
            "pm.environment.set('leaked', 'yes');\n\
             delete globalThis.__SP_SET_REQUEST__;\n\
             delete globalThis.__SP_RESTORE_STATE__;\n\
             throw new Error('boom');",
        ),
        None,
    )];

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
    assert_eq!(outcome.errors[0].kind, ScriptErrorKind::Runtime);
    assert!(
        outcome.errors[0].message.contains("boom"),
        "原始失败要原样保留，got {}",
        outcome.errors[0].message
    );
    assert!(
        outcome.env_writes.is_empty(),
        "失败脚本的写入不该被记成产出"
    );

    let rollback: Vec<&str> = outcome
        .logs
        .iter()
        .filter(|l| l.level == "error")
        .map(|l| l.message.as_str())
        .collect();
    assert_eq!(rollback.len(), 2, "got {rollback:?}");
    assert!(rollback[0].contains("request"), "got {}", rollback[0]);
    assert!(rollback[1].contains("environment"), "got {}", rollback[1]);
}
