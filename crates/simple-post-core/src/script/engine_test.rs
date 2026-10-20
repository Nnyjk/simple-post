//! 沙箱引擎的单元测试。
//!
//! 这些测试同时是**沙箱的安全承诺**的回归网：死循环被打断、超内存被挡住、深递归被
//! 挡住、脚本够不到 I/O、失败的脚本不留下副作用。

use std::time::{Duration, Instant};

use super::*;

/// 一个最小的请求。
fn request() -> ScriptRequest {
    ScriptRequest {
        method: "GET".to_string(),
        url: "http://example.test/api".to_string(),
        headers: vec![("X-Token".to_string(), "old".to_string())],
        body: None,
    }
}

fn limits(timeout_ms: u64) -> ScriptLimits {
    ScriptLimits {
        timeout_ms,
        ..DEFAULT_SCRIPT_LIMITS
    }
}

/// 建沙箱并注入输入。失败直接 panic：那是测试自身的搭法出错。
fn sandbox(limits: &ScriptLimits, request: &ScriptRequest) -> Sandbox {
    let sandbox = Sandbox::new(limits).expect("engine should start");
    sandbox
        .init(request, None, &EnvSnapshot::default(), limits)
        .expect("init should succeed");
    sandbox
}

#[test]
fn an_infinite_loop_is_interrupted_at_the_deadline() {
    let limits = limits(200);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let started = Instant::now();
    let run = sandbox.run("while (true) {}", &request);
    let elapsed = started.elapsed();

    let failure = run.failure.expect("死循环应被打断");
    assert_eq!(failure.kind, ScriptErrorKind::Timeout);
    // 真的被打断了，而不是等它自己跑完。
    assert!(
        elapsed < Duration::from_secs(5),
        "中断应远早于脚本自行结束，实际耗时 {elapsed:?}"
    );
    // 宿主还活着：下一段脚本照跑。
    assert!(sandbox.run("1 + 1", &request).failure.is_none());
}

#[test]
fn allocating_past_the_memory_limit_fails_without_killing_the_host() {
    let limits = ScriptLimits {
        memory_bytes: 4 * 1024 * 1024,
        ..limits(5_000)
    };
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run("const s = 'x'.repeat(64 * 1024 * 1024);", &request);

    let failure = run.failure.expect("超内存应失败");
    assert_eq!(failure.kind, ScriptErrorKind::Memory);
    assert!(sandbox.run("1 + 1", &request).failure.is_none());
}

#[test]
fn deep_recursion_is_stopped_by_the_stack_limit() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run("function f() { return f(); } f();", &request);

    let failure = run.failure.expect("深递归应被挡住");
    assert_eq!(
        failure.kind,
        ScriptErrorKind::Stack,
        "报错文本：{}",
        failure.message
    );
    assert!(sandbox.run("1 + 1", &request).failure.is_none());
}

#[test]
fn the_script_cannot_reach_filesystem_network_or_processes() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run(
        "console.log([typeof require, typeof fetch, typeof process, typeof XMLHttpRequest, \
         typeof globalThis.setTimeout, typeof globalThis.Deno].join(','));",
        &request,
    );

    assert!(run.failure.is_none());
    assert_eq!(
        run.effects.logs[0].message,
        "undefined,undefined,undefined,undefined,undefined,undefined"
    );
}

#[test]
fn a_pre_request_script_can_rewrite_the_request() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run(
        "pm.request.url = 'http://changed.test/x';\
         pm.request.method = 'POST';\
         pm.request.headers['X-Token'] = 'fresh';\
         pm.request.body = '{\"a\":1}';",
        &request,
    );

    assert!(run.failure.is_none());
    assert_eq!(run.effects.request.url, "http://changed.test/x");
    assert_eq!(run.effects.request.method, "POST");
    assert_eq!(run.effects.request.body.as_deref(), Some("{\"a\":1}"));
    assert!(run
        .effects
        .request
        .headers
        .iter()
        .any(|(k, v)| k == "X-Token" && v == "fresh"));
}

#[test]
fn a_failing_script_leaves_no_trace_on_the_request() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run(
        "pm.request.url = 'http://changed.test/x'; throw new Error('boom');",
        &request,
    );

    let failure = run.failure.expect("抛错应被记下");
    assert_eq!(failure.kind, ScriptErrorKind::Runtime);
    assert!(failure.message.contains("boom"));
    // 改动被丢弃：部分生效的请求比没生效的更难排查。
    assert_eq!(run.effects.request.url, request.url);
    assert_eq!(run.effects.request.headers, request.headers);
}

#[test]
fn a_failing_script_writes_no_environment_variables() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run(
        "pm.environment.set('token', 'x'); throw new Error('boom');",
        &request,
    );

    assert!(run.failure.is_some());
    assert!(run.effects.env_writes.is_empty());
}

#[test]
fn environment_variables_can_be_read_and_written() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = Sandbox::new(&limits).expect("engine");
    let env = EnvSnapshot {
        id: Some("env_1".to_string()),
        variables: [("token".to_string(), "abc".to_string())]
            .into_iter()
            .collect(),
    };
    sandbox.init(&request, None, &env, &limits).expect("init");

    let run = sandbox.run(
        "console.log(String(pm.environment.get('token')));\
         pm.environment.set('next', pm.environment.get('token') + '!');\
         pm.environment.unset('token');",
        &request,
    );

    assert!(run.failure.is_none());
    assert_eq!(run.effects.logs[0].message, "abc");
    assert_eq!(run.effects.env_writes.len(), 2);
    assert_eq!(run.effects.env_writes[0].key, "next");
    assert_eq!(run.effects.env_writes[0].value.as_deref(), Some("abc!"));
    assert_eq!(run.effects.env_writes[1].key, "token");
    assert_eq!(run.effects.env_writes[1].value, None);
}

#[test]
fn request_scoped_variables_do_not_reach_the_environment() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run("pm.variables.set('local', '1');", &request);

    assert!(run.failure.is_none());
    assert!(run.effects.env_writes.is_empty());
}

#[test]
fn assertions_are_collected_with_their_outcome() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run(
        "pm.test('通过', function () { pm.expect(1).to.equal(1); });\
         pm.test('不通过', function () { pm.expect(1).to.equal(2); });\
         pm.test('深比较', function () { pm.expect({a: 1}).to.eql({a: 1}); });",
        &request,
    );

    assert!(run.failure.is_none());
    assert_eq!(run.effects.assertions.len(), 3);
    assert!(run.effects.assertions[0].passed);
    assert!(!run.effects.assertions[1].passed);
    assert!(run.effects.assertions[1].error.is_some());
    assert!(run.effects.assertions[2].passed);
}

#[test]
fn console_output_is_captured_as_logs() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run("console.log('hello', 1); console.error('bad');", &request);

    assert!(run.failure.is_none());
    assert_eq!(run.effects.logs.len(), 2);
    assert_eq!(run.effects.logs[0].level, "log");
    assert_eq!(run.effects.logs[0].message, "hello 1");
    assert_eq!(run.effects.logs[1].level, "error");
}

#[test]
fn headers_the_script_did_not_touch_keep_their_order_and_duplicates() {
    let limits = limits(5_000);
    let request = ScriptRequest {
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        headers: vec![
            ("X-A".to_string(), "1".to_string()),
            ("X-B".to_string(), "2".to_string()),
            ("X-A".to_string(), "3".to_string()),
        ],
        body: None,
    };
    let sandbox = sandbox(&limits, &request);

    let run = sandbox.run("console.log('nothing touched');", &request);

    assert!(run.failure.is_none());
    assert_eq!(run.effects.request.headers, request.headers);
}

#[test]
fn the_response_is_visible_to_a_test_script() {
    let limits = limits(5_000);
    let request = request();
    let sandbox = Sandbox::new(&limits).expect("engine");
    let response = ScriptResponse {
        code: 200,
        status: "OK".to_string(),
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: "{\"id\":7}".to_string(),
        response_time_ms: 12,
    };
    sandbox
        .init(&request, Some(&response), &EnvSnapshot::default(), &limits)
        .expect("init");

    let run = sandbox.run(
        "pm.test('状态码', function () { pm.response.to.have.status(200); });\
         pm.test('响应体', function () { pm.expect(pm.response.json().id).to.equal(7); });\
         console.log(String(pm.response.code), String(pm.response.responseTime));",
        &request,
    );

    assert!(run.failure.is_none());
    assert_eq!(run.effects.assertions.len(), 2);
    assert!(run.effects.assertions.iter().all(|a| a.passed));
    assert_eq!(run.effects.logs[0].message, "200 12");
}
