//! 动态变量引擎的单元测试。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::*;

/// 一份最小可用的内联数据源。定义必须有一个数据源，故测试里的定义都带它。
const INLINE_SOURCE: &str = r#"{"method":"GET","url":"http://127.0.0.1:9/points"}"#;
use crate::db;
use crate::domain::{
    AuthConfig, AuthType, DynamicCacheEntry, DynamicErrorKind, DynamicValueSource, DynamicVariable,
    DynamicVariablePatch, EndpointDetail, HttpMethod, KeyValue, Project, RequestBody, ResourceNode,
    ResourceNodeType, ValueMode,
};

// -------------------------------------------------------------------------
// 合并规则
// -------------------------------------------------------------------------

#[test]
fn the_dynamic_value_wins_over_the_environment_variable_of_the_same_name() {
    let env = HashMap::from([("sns".to_string(), "env".to_string())]);
    let dynamic = HashMap::from([("sns".to_string(), "dyn".to_string())]);
    let pool = merge_pool(&env, &[], &dynamic);
    assert_eq!(pool.get("sns").map(String::as_str), Some("dyn"));
}

#[test]
fn a_disabled_environment_variable_does_not_block_the_dynamic_value() {
    let env = HashMap::from([("sns".to_string(), "env".to_string())]);
    let dynamic = HashMap::from([("sns".to_string(), "dyn".to_string())]);
    let pool = merge_pool(&env, &["sns".to_string()], &dynamic);
    assert_eq!(pool.get("sns").map(String::as_str), Some("dyn"));
}

#[test]
fn a_disabled_environment_variable_stays_out_of_the_pool() {
    let env = HashMap::from([
        ("sns".to_string(), "env".to_string()),
        ("token".to_string(), "t".to_string()),
    ]);
    let pool = merge_pool(&env, &["token".to_string()], &HashMap::new());
    assert!(pool.contains_key("sns"));
    assert!(!pool.contains_key("token"));
}

// -------------------------------------------------------------------------
// 序列化
// -------------------------------------------------------------------------

#[test]
fn the_value_modes_serialize_as_documented() {
    let array = serde_json::json!([{ "code": "A001" }]);
    assert_eq!(
        serialize_value(&array, ValueMode::Auto),
        "[{\"code\":\"A001\"}]"
    );
    assert_eq!(
        serialize_value(&array, ValueMode::Json),
        "[{\"code\":\"A001\"}]"
    );
    assert_eq!(
        serialize_value(&array, ValueMode::Text),
        "[{\"code\":\"A001\"}]"
    );

    let text = serde_json::json!("A001");
    assert_eq!(serialize_value(&text, ValueMode::Auto), "A001");
    assert_eq!(serialize_value(&text, ValueMode::Text), "A001");
    assert_eq!(serialize_value(&text, ValueMode::Json), "\"A001\"");
}

#[test]
fn a_value_that_is_not_json_becomes_a_plain_string() {
    let parsed = parse_raw("A001");
    assert_eq!(parsed, serde_json::Value::String("A001".into()));
    assert_eq!(serialize_value(&parsed, ValueMode::Auto), "A001");

    let parsed = parse_raw("[1,2]");
    assert_eq!(parsed, serde_json::json!([1, 2]));
}

// -------------------------------------------------------------------------
// 名字扫描
// -------------------------------------------------------------------------

#[test]
fn the_scan_covers_the_fields_that_actually_get_substituted() {
    let params = vec![KeyValue {
        id: "p1".into(),
        key: "q".into(),
        value: "{{paramVar}}".into(),
        enabled: true,
        description: None,
    }];
    let headers = vec![KeyValue {
        id: "h1".into(),
        key: "X".into(),
        value: "{{headerVar}}".into(),
        enabled: true,
        description: None,
    }];
    let body = RequestBody {
        mode: crate::domain::BodyMode::Json,
        content: "{\"a\": {{bodyVar}}}".into(),
        schema: None,
    };
    let auth = AuthConfig {
        kind: AuthType::Bearer,
        bearer: Some("{{authVar}}".into()),
        ..Default::default()
    };
    let parts = TemplateParts {
        base_url: "https://{{host}}",

        url: "/{{pathVar}}",
        params: &params,
        path_params: &[],
        headers: &headers,
        body: &body,
        auth: &auth,
    };
    let names = list_names(&parts);
    assert_eq!(
        names,
        vec![
            "host".to_string(),
            "pathVar".to_string(),
            "paramVar".to_string(),
            "headerVar".to_string(),
            "bodyVar".to_string(),
            "authVar".to_string(),
        ]
    );
}

#[test]
fn a_disabled_parameter_is_not_referenced() {
    let params = vec![KeyValue {
        id: "p1".into(),
        key: "q".into(),
        value: "{{off}}".into(),
        enabled: false,
        description: None,
    }];
    let parts = TemplateParts {
        base_url: "",

        url: "/x",
        params: &params,
        path_params: &[],
        headers: &[],
        body: &RequestBody::default(),
        auth: &AuthConfig::default(),
    };
    assert!(list_names(&parts).is_empty());
}

// -------------------------------------------------------------------------
// 缓存决策
// -------------------------------------------------------------------------

fn definition(ttl_seconds: i64, stale_fallback: bool) -> DynamicVariable {
    DynamicVariable {
        id: "d1".into(),
        project_id: Some("p1".into()),
        name: "sns".into(),
        description: None,
        source_endpoint_id: None,
        source_endpoint_name: None,
        source_inline_json: Some(INLINE_SOURCE.to_string()),
        source_environment_id: None,
        extract_path: "$".into(),
        transform_script: None,
        value_mode: ValueMode::Auto,
        ttl_seconds,
        stale_fallback,
        enabled: true,
        sort_order: 0,
        created_at: 0,
        updated_at: 0,
    }
}

fn entry(
    is_override: bool,
    expires_at: i64,
    override_expires_at: Option<i64>,
) -> DynamicCacheEntry {
    DynamicCacheEntry {
        definition_id: "d1".into(),
        environment_id: "env1".into(),
        value_json: Some("\"v\"".into()),
        fetched_at: 100,
        expires_at,
        is_override,
        override_expires_at,
    }
}

#[test]
fn a_fresh_cache_entry_is_used() {
    let def = definition(60, false);
    let e = entry(false, 1_000, None);
    assert_eq!(
        decide(&def, Some(&e), 500, false),
        Decision::UseCache {
            value_json: "\"v\"".to_string()
        }
    );
}

#[test]
fn a_fresh_override_is_used_and_never_expires_when_it_has_no_deadline() {
    let def = definition(60, false);
    let e = entry(true, 1_000, None);
    assert_eq!(
        decide(&def, Some(&e), 9_999_999, false),
        Decision::UseOverride {
            value_json: "\"v\"".to_string()
        }
    );
}

#[test]
fn an_expired_override_goes_back_to_fetching() {
    // 覆盖值过期了 → 重新取数。定义允许用过期值回退时，那个过期的覆盖值就是回退候选。
    let def = definition(60, true);
    let e = entry(true, 1_000, Some(500));
    assert_eq!(
        decide(&def, Some(&e), 900, false),
        Decision::Fetch {
            stale_fallback: Some("\"v\"".to_string())
        }
    );

    // 定义不允许用过期值回退时没有候选。
    let strict = definition(60, false);
    assert_eq!(
        decide(&strict, Some(&e), 900, false),
        Decision::Fetch {
            stale_fallback: None
        }
    );
}

#[test]
fn an_expired_cache_entry_is_fetched_again_and_keeps_the_stale_value_as_a_fallback() {
    let def = definition(60, true);
    let e = entry(false, 500, None);
    assert_eq!(
        decide(&def, Some(&e), 900, false),
        Decision::Fetch {
            stale_fallback: Some("\"v\"".to_string())
        }
    );
}

#[test]
fn a_stale_value_is_not_kept_as_a_fallback_when_the_definition_does_not_allow_it() {
    let def = definition(60, false);
    let e = entry(false, 500, None);
    assert_eq!(
        decide(&def, Some(&e), 900, false),
        Decision::Fetch {
            stale_fallback: None
        }
    );
}

#[test]
fn force_ignores_a_fresh_cache_entry() {
    let def = definition(60, false);
    let e = entry(false, 1_000, None);
    assert!(matches!(
        decide(&def, Some(&e), 500, true),
        Decision::Fetch { .. }
    ));
}

#[test]
fn force_offers_only_an_expired_value_as_the_fallback() {
    // 「过期值」这个身份只对真的过期的行成立：`force` 不看有效期，一份新鲜值也会走到取数，
    // 但它不是过期值，取数失败时按缺失处理，而不是把它标成「过期值」交给界面。
    let def = definition(60, true);
    let fresh = entry(false, 1_000, None);
    assert_eq!(
        decide(&def, Some(&fresh), 500, true),
        Decision::Fetch {
            stale_fallback: None
        }
    );

    let expired = entry(false, 400, None);
    assert_eq!(
        decide(&def, Some(&expired), 500, true),
        Decision::Fetch {
            stale_fallback: Some("\"v\"".to_string())
        }
    );
}

#[test]
fn force_also_drops_a_live_override() {
    // 「刷新」是强制重新取数：它跳过缓存里的一切，包括用户手动指定的值。
    // 一个什么都不做的按钮比丢掉一份手动值更糟——丢掉是可见的（徽标变回「自动取回」）。
    let def = definition(60, false);
    let e = entry(true, 1_000, Some(9_999));
    assert_eq!(
        decide(&def, Some(&e), 500, true),
        Decision::Fetch {
            stale_fallback: None
        }
    );
}

#[test]
fn an_absent_cache_entry_is_fetched() {
    let def = definition(60, false);
    assert_eq!(
        decide(&def, None, 500, false),
        Decision::Fetch {
            stale_fallback: None
        }
    );
}

// -------------------------------------------------------------------------
// 抽取
// -------------------------------------------------------------------------

#[test]
fn the_root_path_returns_the_whole_response() {
    let value = extract_value("{\"a\":1}", "$").expect("抽取");
    assert_eq!(value, serde_json::json!({ "a": 1 }));
}

#[test]
fn a_path_that_matches_nothing_is_reported_as_no_match() {
    let err = extract_value("{\"a\":1}", "$.b").expect_err("不该命中");
    assert_eq!(err, DynamicErrorKind::NoMatch);
}

#[test]
fn a_body_that_is_not_json_is_reported_as_not_json() {
    let err = extract_value("not json", "$").expect_err("不该成功");
    assert_eq!(err, DynamicErrorKind::NotJson);
}

#[test]
fn an_illegal_path_is_reported_as_a_source_error() {
    let err = extract_value("{\"a\":1}", "$$$[").expect_err("不该成功");
    assert_eq!(err, DynamicErrorKind::SourceError);
}

// -------------------------------------------------------------------------
// 编排：取数、缓存、single-flight
// -------------------------------------------------------------------------

/// 一个数着次数的假取数器。
struct CountingFetcher {
    calls: AtomicUsize,
    body: String,
    status: u16,
    fail: bool,
    delay_ms: u64,
}

impl CountingFetcher {
    fn new(body: &str) -> CountingFetcher {
        CountingFetcher {
            calls: AtomicUsize::new(0),
            body: body.to_string(),
            status: 200,
            fail: false,
            delay_ms: 0,
        }
    }

    fn failing() -> CountingFetcher {
        CountingFetcher {
            calls: AtomicUsize::new(0),
            body: String::new(),
            status: 0,
            fail: true,
            delay_ms: 0,
        }
    }

    fn slow(body: &str, delay_ms: u64) -> CountingFetcher {
        CountingFetcher {
            calls: AtomicUsize::new(0),
            body: body.to_string(),
            status: 200,
            fail: false,
            delay_ms,
        }
    }

    fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl SourceFetcher for CountingFetcher {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        _ctx: &'a ResolveContext,
        _source: &'a SourceContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            if self.fail {
                return Err(DynamicError {
                    name: def.name.clone(),
                    kind: DynamicErrorKind::Network,
                    message: "连不上".into(),
                });
            }
            Ok(RawSource {
                status: self.status,
                content_type: "application/json".into(),
                body: self.body.clone(),
                truncated: false,
                body_base64: None,
                headers: vec![],
                duration_ms: 0,
            })
        })
    }
}

fn seeded_pool() -> (db::DbPool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-dynamic-engine-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        db::dynamic_variables::create(c, &definition(60, false))?;
        Ok(())
    })
    .expect("seed");
    (pool, dir)
}

fn context(pool: &db::DbPool) -> ResolveContext {
    ResolveContext {
        pool: pool.clone(),
        environment_id: None,
        lock_wait: None,
    }
}

/// 带「等锁」探针的上下文：解析在拿单飞行锁之前往返回的通道里报一声。
///
/// 用它取代固定时长的等待——`sleep` 证明不了对方已经算完进门那一次、正排在锁上。
fn context_with_lock_wait(
    pool: &db::DbPool,
) -> (ResolveContext, tokio::sync::mpsc::UnboundedReceiver<()>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = ResolveContext {
        pool: pool.clone(),
        environment_id: None,
        lock_wait: Some(tx),
    };
    (ctx, rx)
}

#[tokio::test]
async fn a_resolution_fetches_the_value_and_caches_it() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[{\"code\":\"A001\"}]");

    let first = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(
        first.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"A001\"}]")
    );
    assert_eq!(fetcher.calls(), 1);

    // 第二次命中缓存，不再取数。
    let second = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 1);
    assert_eq!(
        second.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"A001\"}]")
    );
}

/// 取数上下文造不出来时，这一次解析按缺失处理并记一条错误，而不是让整次发送中止。
#[tokio::test]
async fn a_source_context_that_cannot_be_built_leaves_the_variable_missing() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[]");
    let mut def = definition(60, false);
    // 数据源端点已经被删掉。外键会把它置空，但定义对象可能是删除之前读出来的那一份，
    // 故这条路径真的会走到「端点不在了」。
    def.source_endpoint_id = Some("已经删掉的端点".into());

    let status = resolve_definition(&ctx, &def, false, &fetcher)
        .await
        .expect("按缺失处理，不该中止整次发送");
    assert!(status.value.is_none());
    assert_eq!(status.source, DynamicValueSource::Missing);
    let error = status.error.expect("记一条错误");
    assert_eq!(error.kind, DynamicErrorKind::SourceMissing);
    assert_eq!(error.name, "sns");
    assert_eq!(fetcher.calls(), 0, "取数不该被发起");
}

#[tokio::test]
async fn a_force_ignores_the_cache() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[]");

    resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    resolve_names(&ctx, Some("p1"), &["sns".to_string()], true, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 2);
}

/// 改了抽取规则之后，缓存里那份按旧规则算出来的值作废，下一次解析重新取数。
#[tokio::test]
async fn changing_the_extraction_rule_refetches_instead_of_reading_the_old_value() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[{\"name\":\"A\"}]");

    // 定义第一条是 `$`：整个响应体进缓存。
    let whole = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(
        whole.values.get("sns").map(String::as_str),
        Some("[{\"name\":\"A\"}]")
    );
    assert_eq!(fetcher.calls(), 1);

    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                extract_path: Some("$[0].name".into()),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let narrowed = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 2, "旧值已作废，必须重新取数");
    assert_eq!(narrowed.values.get("sns").map(String::as_str), Some("A"));
}

#[tokio::test]
async fn a_failing_fetch_leaves_the_name_missing_and_reports_the_reason() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::failing();

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert!(!out.values.contains_key("sns"));
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.errors[0].kind, DynamicErrorKind::Network);
    assert_eq!(out.errors[0].name, "sns");
}

#[tokio::test]
async fn a_non_2xx_source_is_reported_as_a_status_error() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("{}").with_status(503);

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.errors[0].kind, DynamicErrorKind::Status);
}

#[tokio::test]
async fn a_source_body_that_is_not_json_is_reported_as_not_json() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("not json");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors[0].kind, DynamicErrorKind::NotJson);
}

#[tokio::test]
async fn a_source_whose_path_matches_nothing_is_reported_as_no_match() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("{\"a\":1}");

    let mut def = definition(60, false);
    def.extract_path = "$.b".into();
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                extract_path: Some("$.b".into()),
                ..Default::default()
            },
        )
    })
    .expect("patch");
    def.name = "sns".into();

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors[0].kind, DynamicErrorKind::NoMatch);
}

// ---------------------------------------------------------------------------
// 处理脚本
// ---------------------------------------------------------------------------

/// 给库里那条定义装上一段处理脚本。
fn set_transform_script(pool: &db::DbPool, script: &str) {
    db::with_tx(pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                transform_script: Some(Some(script.into())),
                ..Default::default()
            },
        )
    })
    .expect("patch");
}

#[tokio::test]
async fn the_transform_script_rewrites_the_extracted_value() {
    // 取数 → 抽取 → 脚本改写：脚本看到的是**抽取出来的那一份**，不是整段响应。
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[{\"code\":\"A001\"},{\"code\":\"B002\"}]");
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                extract_path: Some("$[0].code".into()),
                transform_script: Some(Some("return pm.data + \"-X\";".into())),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(
        out.values.get("sns").map(String::as_str),
        Some("A001-X"),
        "脚本 return 的值就是最终值"
    );
}

#[tokio::test]
async fn the_transform_script_can_read_the_raw_response() {
    // `pm.response` 是**整段**响应：抽取规则之外的字段也能拿到。
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("{\"total\":7,\"items\":[1,2]}");
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                extract_path: Some("$.items".into()),
                transform_script: Some(Some("return pm.response.json().total;".into())),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.values.get("sns").map(String::as_str), Some("7"));
}

#[tokio::test]
async fn a_transform_script_that_throws_leaves_the_name_missing() {
    // 脚本失败按「取值失败」处理：名字缺失，原因是脚本错误，不是网络错误。
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("{\"a\":1}");
    set_transform_script(&pool, "throw new Error('炸了');");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert!(!out.values.contains_key("sns"));
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.errors[0].kind, DynamicErrorKind::ScriptError);
}

#[tokio::test]
async fn a_transform_script_that_returns_nothing_leaves_the_name_missing() {
    // 没有 `return` 就没有值：脚本「跑通了」不等于「给出了结果」。
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("{\"a\":1}");
    set_transform_script(&pool, "const x = pm.data;");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert!(!out.values.contains_key("sns"));
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.errors[0].kind, DynamicErrorKind::ScriptError);
}

#[tokio::test]
async fn a_value_that_is_too_large_is_not_written_to_the_cache() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let big = format!("\"{}\"", "x".repeat(MAX_VALUE_BYTES + 10));
    let fetcher = CountingFetcher::new(&big);

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors[0].kind, DynamicErrorKind::TooLarge);

    // 内联数据源没有环境，缓存键的环境段就是空串（空串表示没有环境）。
    let cached =
        db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", "")).expect("read");
    assert!(cached.is_none());
}

#[tokio::test]
async fn concurrent_resolutions_of_the_same_definition_fetch_only_once() {
    let (pool, _dir) = seeded_pool();
    let ctx = Arc::new(context(&pool));
    let fetcher = Arc::new(CountingFetcher::slow("[1]", 80));

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let ctx = Arc::clone(&ctx);
        let fetcher = Arc::clone(&fetcher);
        tasks.push(tokio::spawn(async move {
            resolve_names(
                &ctx,
                Some("p1"),
                &["sns".to_string()],
                false,
                fetcher.as_ref(),
            )
            .await
            .expect("解析")
        }));
    }
    for task in tasks {
        let out = task.await.expect("任务");
        assert_eq!(
            out.values.get("sns").map(String::as_str),
            Some("[1]"),
            "每个等待者都拿到同一个值"
        );
    }
    assert_eq!(fetcher.calls(), 1, "同一时刻只发一次取数请求");
}

#[tokio::test]
async fn a_disabled_definition_is_not_resolved() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[]");
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                enabled: Some(false),
                ..Default::default()
            },
        )
    })
    .expect("patch");

    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert!(out.values.is_empty());
    assert!(out.errors.is_empty());
    assert_eq!(fetcher.calls(), 0);
}

#[tokio::test]
async fn a_name_without_a_definition_is_skipped() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[]");

    let out = resolve_names(&ctx, Some("p1"), &["token".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert!(out.values.is_empty());
    assert!(out.errors.is_empty());
    assert_eq!(fetcher.calls(), 0);
}

#[tokio::test]
async fn a_stale_value_is_used_when_the_fetch_fails_and_the_definition_allows_it() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    // 先取一次，把值写进缓存；随后把 TTL 改成 0（立刻过期）并允许回退。
    let good = CountingFetcher::new("[1]");
    resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &good)
        .await
        .expect("解析");
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            crate::domain::DynamicVariablePatch {
                ttl_seconds: Some(0),
                stale_fallback: Some(true),
                ..Default::default()
            },
        )
    })
    .expect("patch");
    // 让缓存过期：把过期时间推到过去。
    db::with_tx(&pool, |c| {
        c.execute(
            "UPDATE dynamic_variable_cache SET expires_at = 1 WHERE definition_id = 'd1'",
            [],
        )
        .map_err(crate::error::AppError::Sqlite)
    })
    .expect("expire");

    let bad = CountingFetcher::failing();
    let out = resolve_names(&ctx, Some("p1"), &["sns".to_string()], false, &bad)
        .await
        .expect("解析");
    assert_eq!(out.values.get("sns").map(String::as_str), Some("[1]"));
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.statuses[0].source, DynamicValueSource::Stale);
}

// -------------------------------------------------------------------------
// 取数上下文
// -------------------------------------------------------------------------

/// 一个项目，一个模块，其下一个接口，以及两个环境。
fn pool_with_a_module() -> (db::DbPool, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "simple-post-dynamic-ctx-test-{}-{}",
        std::process::id(),
        crate::db::test_counter::next()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pool = db::init(&dir).expect("init");
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p1".into(),
                name: "First".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        for (id, name) in [("env_a", "alpha"), ("env_b", "beta")] {
            db::environments::create(
                c,
                &crate::domain::Environment {
                    id: id.into(),
                    project_id: "p1".into(),
                    name: name.into(),
                    module_base_urls: Default::default(),
                    variables: Default::default(),
                    disabled_variables: Vec::new(),
                    created_at: 0,
                },
            )?;
        }
        db::nodes::create(
            c,
            &ResourceNode {
                id: "m1".into(),
                root_id: "p1".into(),
                parent_id: None,
                kind: ResourceNodeType::Module,
                name: "模块".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            None,
        )?;
        db::nodes::create(
            c,
            &ResourceNode {
                id: "e1".into(),
                root_id: "p1".into(),
                parent_id: Some("m1".into()),
                kind: ResourceNodeType::Endpoint,
                name: "取数接口".into(),
                description: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
            },
            Some(&EndpointDetail {
                node_id: "e1".into(),
                method: HttpMethod::Get,
                url: "/points".into(),
                base_url_override: None,
                params: vec![],
                path_params: vec![],
                headers: vec![],
                body: RequestBody::default(),
                auth: AuthConfig::default(),
                docs: None,
                notes: None,
                tags: vec![],
                pre_request_script: None,
                test_script: None,
                tls: None,
                responses: vec![],
                protocol: Default::default(),
                graphql: None,
            }),
        )?;
        Ok(())
    })
    .expect("seed");
    (pool, dir)
}

#[test]
fn the_source_context_names_the_module_the_endpoint_sits_in() {
    // base URL 住在 (环境 × 模块) 那一格，故取数端点所在的模块决定取哪一格。
    let (pool, _dir) = pool_with_a_module();
    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;

    let ctx = source_context(&pool, &def, None).expect("取数上下文");
    assert_eq!(ctx.module_id.as_deref(), Some("m1"));
    assert_eq!(ctx.project_id.as_deref(), Some("p1"));
}

/// 定义点名的环境是**指定**，压过调用方此刻选中的那个。
#[test]
fn the_source_context_takes_the_environment_the_definition_names() {
    let (pool, _dir) = pool_with_a_module();
    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;
    def.source_environment_id = Some("env_b".into());

    let ctx = source_context(&pool, &def, Some("env_a")).expect("取数上下文");
    assert_eq!(ctx.environment_id.as_deref(), Some("env_b"));
}

/// 定义没点名环境就是**继承**：用调用方此刻选中的那个。这条是本次改动的核心语义——
/// 同一个 `{{name}}` 随用户切环境换值，故缓存键也跟着换。
#[test]
fn the_source_context_inherits_the_callers_environment() {
    let (pool, _dir) = pool_with_a_module();
    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;

    let ctx = source_context(&pool, &def, Some("env_b")).expect("取数上下文");
    assert_eq!(ctx.environment_id.as_deref(), Some("env_b"));
}

/// 调用方没有环境时回落到数据源项目里最早创建的那个（`environments::get_default`）。
#[test]
fn the_source_context_falls_back_to_the_projects_default_environment() {
    let (pool, _dir) = pool_with_a_module();
    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;

    // 两个环境的 `created_at` 相同（同一毫秒建的），故按 id 取较小的那个：`env_a`。
    let ctx = source_context(&pool, &def, None).expect("取数上下文");
    assert_eq!(ctx.environment_id.as_deref(), Some("env_a"));
}

/// 调用方选中的环境属于**别的**项目时不算数：环境的格子按 (环境 × 模块) 寻址，拿别处
/// 的环境去取数会取到别的内容。回落到数据源项目的默认环境。
#[test]
fn the_source_context_ignores_an_environment_of_another_project() {
    let (pool, _dir) = pool_with_a_module();
    db::with_tx(&pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p2".into(),
                name: "Second".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        db::environments::create(
            c,
            &crate::domain::Environment {
                id: "env_p2".into(),
                project_id: "p2".into(),
                name: "other".into(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .expect("seed p2");

    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;

    let ctx = source_context(&pool, &def, Some("env_p2")).expect("取数上下文");
    assert_eq!(ctx.environment_id.as_deref(), Some("env_a"));
}

#[test]
fn an_inline_source_has_no_project_no_environment_and_no_module() {
    let (pool, _dir) = pool_with_a_module();
    let def = definition(60, false);

    // 内联请求自带完整地址，调用方选了什么环境都一样。
    let ctx = source_context(&pool, &def, Some("env_a")).expect("取数上下文");
    assert!(ctx.project_id.is_none());
    assert!(ctx.environment_id.is_none());
    assert!(ctx.module_id.is_none());
}

/// 补一个属于另一个项目 `p2` 的环境。
fn seed_foreign_environment(pool: &db::DbPool) {
    db::with_tx(pool, |c| {
        db::projects::create(
            c,
            &Project {
                id: "p2".into(),
                name: "Second".into(),
                description: None,
                color: "#000000".into(),
                created_at: 1,
                updated_at: 1,
            },
        )?;
        db::environments::create(
            c,
            &crate::domain::Environment {
                id: "env_p2".into(),
                project_id: "p2".into(),
                name: "other".into(),
                module_base_urls: Default::default(),
                variables: Default::default(),
                disabled_variables: Vec::new(),
                created_at: 0,
            },
        )?;
        Ok(())
    })
    .expect("seed p2");
}

/// 定义点名的环境不属于数据源接口所属的项目：与保存侧同一条规则，取数直接报错。
///
/// 报错而不是静默换成默认环境——否则「这个值从哪个环境来」的回报就是假的。
#[tokio::test]
async fn a_named_environment_of_another_project_is_rejected() {
    let (pool, _dir) = pool_with_a_module();
    seed_foreign_environment(&pool);
    let mut def = definition(60, false);
    def.source_endpoint_id = Some("e1".into());
    def.source_inline_json = None;
    def.source_environment_id = Some("env_p2".into());

    let err = match source_context(&pool, &def, Some("env_a")) {
        Ok(_) => panic!("点名的环境不属于数据源的项目"),
        Err(err) => err,
    };
    let crate::error::AppError::Validation(message) = err else {
        panic!("要按校验错误报出来：{err:?}");
    };
    assert!(message.contains("不在数据源接口所属的项目里"), "{message}");

    // 解析侧同一个结论：报 source_error 并按缺失处理，取数器根本没被叫到。
    let fetcher = CountingFetcher::new("\"不该取到\"");
    let status = resolve_definition(&context(&pool), &def, true, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 0, "判定在发请求之前");
    assert_eq!(status.source, DynamicValueSource::Missing);
    assert!(status.value.is_none());
    let error = status.error.expect("要报取数失败");
    assert_eq!(error.kind, DynamicErrorKind::SourceError);
    assert!(
        error.message.contains("不在数据源接口所属的项目里"),
        "{}",
        error.message
    );
}

/// 数据源端点被删之后（`ON DELETE SET NULL`）留下的自动缓存值不再算数。
///
/// 没有可执行的数据源，它的旧值就没有依据；按缺失处理，而不是把旧值当成当前值。
#[tokio::test]
async fn a_definition_without_a_source_ignores_its_cached_value() {
    let (pool, _dir) = seeded_pool();
    let now = crate::domain::now_ms();
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"旧值\"".into()),
                fetched_at: now - 1_000,
                expires_at: now + 60_000,
                is_override: false,
                override_expires_at: None,
            },
        )
    })
    .expect("seed 缓存");

    let mut def = definition(60, false);
    def.source_inline_json = None; // 数据源被删：两个来源都空
                                   // 真实取数器在这里报「没有数据源」；这条用例只关心「根本没读缓存就去了取数」。
    let fetcher = CountingFetcher::failing();
    let status = resolve_definition(&context(&pool), &def, false, &fetcher)
        .await
        .expect("解析");

    assert_eq!(fetcher.calls(), 1, "没有数据源就不读自动缓存行");
    assert!(status.value.is_none(), "旧值不再生效：{:?}", status.value);
    assert_eq!(status.source, DynamicValueSource::Missing);
}

/// 覆盖值是用户手写的，不来自取数：没有数据源也照样生效。
#[tokio::test]
async fn a_definition_without_a_source_still_serves_an_override() {
    let (pool, _dir) = seeded_pool();
    let now = crate::domain::now_ms();
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"手写\"".into()),
                fetched_at: now,
                expires_at: now + 60_000,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("seed 覆盖值");

    let mut def = definition(60, false);
    def.source_inline_json = None;
    let fetcher = CountingFetcher::new("\"不该取\"");
    let status = resolve_definition(&context(&pool), &def, false, &fetcher)
        .await
        .expect("解析");

    assert_eq!(fetcher.calls(), 0, "覆盖值不取数");
    assert_eq!(status.value.as_deref(), Some("手写"));
    assert_eq!(status.source, DynamicValueSource::Override);
}

/// 取数时顺手改掉定义的那条取数器，模拟「用户在请求飞行途中保存了编辑」。
struct RewritingFetcher {
    pool: db::DbPool,
    changed: AtomicUsize,
}

impl SourceFetcher for RewritingFetcher {
    fn fetch<'a>(
        &'a self,
        def: &'a DynamicVariable,
        _ctx: &'a ResolveContext,
        _source: &'a SourceContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>> {
        Box::pin(async move {
            // 与写路径同一个入口：改抽取规则同时清掉自动缓存行。
            db::with_tx(&self.pool, |c| {
                db::dynamic_variables::update(
                    c,
                    &def.id,
                    DynamicVariablePatch {
                        extract_path: Some("$.name".into()),
                        ..Default::default()
                    },
                )
            })
            .map_err(|e| DynamicError {
                name: def.name.clone(),
                kind: DynamicErrorKind::SourceError,
                message: e.to_string(),
            })?;
            self.changed.fetch_add(1, Ordering::SeqCst);
            Ok(RawSource {
                status: 200,
                content_type: "application/json".into(),
                body: r#"{"name":"A001"}"#.into(),
                truncated: false,
                body_base64: None,
                headers: vec![],
                duration_ms: 0,
            })
        })
    }
}

/// 取数途中改了抽取规则：这次请求的值照常返回，但不写进缓存。
///
/// 否则下一次解析读到的是一整对象（旧规则算出来的），而用户已经把规则改成 `$.name`。
#[tokio::test]
async fn a_definition_changed_during_the_fetch_is_not_cached_with_the_old_value() {
    let (pool, _dir) = seeded_pool();
    let def = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let fetcher = RewritingFetcher {
        pool: pool.clone(),
        changed: AtomicUsize::new(0),
    };

    let first = resolve_definition(&context(&pool), &def, true, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.changed.load(Ordering::SeqCst), 1);
    assert_eq!(
        first.value.as_deref(),
        Some(r#"{"name":"A001"}"#),
        "这次请求照常返回"
    );
    assert!(
        first.expires_at.is_none(),
        "没有写缓存就没有到期时间：{:?}",
        first.expires_at
    );

    // 旧规则算出的值不落库。
    let cached =
        db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", "")).expect("读缓存");
    assert!(cached.is_none(), "改了定义之后不写旧值");

    // 下一次解析按新规则重新取数。
    let again = CountingFetcher::new(r#"{"name":"A001"}"#);
    let second = resolve_names(
        &context(&pool),
        Some("p1"),
        &["sns".to_string()],
        false,
        &again,
    )
    .await
    .expect("解析");
    assert_eq!(again.calls(), 1, "缓存里没有值，故重新取数");
    assert_eq!(second.values.get("sns").map(String::as_str), Some("A001"));
}

/// 取数器：发请求时先报一声，然后挂在半路等放行——好让测试在「请求已发出、还没回来」
/// 这段窗口里做别的事。
struct GatedFetcher {
    calls: AtomicUsize,
    reached: tokio::sync::mpsc::Sender<()>,
    release: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>,
    body: String,
}

/// 等一个「已经走到取数」的信号，超时就当失败。
///
/// 被测代码若根本没走到取数，无超时的 `recv()` 会让整个测试进程挂在半路——那不是失败，
/// 是挂死。故这里给一个宽裕但有限的上限，超时直接 panic 说明原因。
async fn wait_for_fetch(reached: &mut tokio::sync::mpsc::Receiver<()>, what: &str) {
    match tokio::time::timeout(Duration::from_secs(10), reached.recv()).await {
        Ok(Some(())) => {}
        Ok(None) => panic!("{what}：取数器的信号通道关了，测试没等到取数"),
        Err(_) => panic!("{what}：10 秒内没走到取数"),
    }
}

/// 等一个「已经排在单飞行锁上」的信号，超时就当失败。
///
/// 与 `wait_for_fetch` 同一个道理：无超时的等待会把整个测试进程挂死，宽裕但有限的上限才是
/// 失败该有的样子。
async fn wait_for_lock_wait(reached: &mut tokio::sync::mpsc::UnboundedReceiver<()>, what: &str) {
    match tokio::time::timeout(Duration::from_secs(10), reached.recv()).await {
        Ok(Some(())) => {}
        Ok(None) => panic!("{what}：等锁信号的通道关了，测试没等到排队"),
        Err(_) => panic!("{what}：10 秒内没排到锁上"),
    }
}

impl SourceFetcher for GatedFetcher {
    fn fetch<'a>(
        &'a self,
        _def: &'a DynamicVariable,
        _ctx: &'a ResolveContext,
        _source: &'a SourceContext,
    ) -> Pin<Box<dyn Future<Output = Result<RawSource, DynamicError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = self.reached.send(()).await;
            let mut release = self.release.lock().await;
            let _ = release.recv().await;
            Ok(RawSource {
                status: 200,
                content_type: "application/json".into(),
                body: self.body.clone(),
                truncated: false,
                body_base64: None,
                headers: vec![],
                duration_ms: 0,
            })
        })
    }
}

/// 取数在飞时写下的覆盖值不被这次写回冲掉。
///
/// 覆盖值是用户在那个环境上亲手钉住的，取数只是「本该这样算」，不该盖过它。故写回事务里
/// 看见仍然生效的覆盖值就跳过写入：这次的值照常交给调用方（它只是不进缓存），手工值留在
/// 缓存行里，下一次解析读到的仍是它。
#[tokio::test]
async fn an_override_written_while_the_fetch_is_in_flight_survives_the_write_back() {
    let (pool, _dir) = seeded_pool();
    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let (reached_tx, mut reached_rx) = tokio::sync::mpsc::channel(1);
    let (release_tx, release_rx) = tokio::sync::mpsc::channel(1);
    let fetcher = Arc::new(GatedFetcher {
        calls: AtomicUsize::new(0),
        reached: reached_tx,
        release: tokio::sync::Mutex::new(release_rx),
        body: "\"取回\"".to_string(),
    });

    let task = tokio::spawn({
        let pool = pool.clone();
        let fetcher = Arc::clone(&fetcher);
        async move {
            resolve_definition(&context(&pool), &definition, true, fetcher.as_ref())
                .await
                .expect("解析")
        }
    });

    // 请求已经发出、还没回来：这时用户在另一个连接上把它存成覆盖值。
    wait_for_fetch(&mut reached_rx, "第一次取数").await;
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"手写\"".into()),
                fetched_at: 1,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写覆盖值");

    release_tx.send(()).await.expect("放行取数器");
    let out = task.await.expect("任务");
    assert_eq!(out.value.as_deref(), Some("取回"), "这次请求的值照常返回");
    assert!(
        out.expires_at.is_none(),
        "没有写缓存就没有到期时间：{:?}",
        out.expires_at
    );

    // 缓存行仍是覆盖值。
    let cached = db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", ""))
        .expect("读缓存")
        .expect("覆盖行还在");
    assert!(cached.is_override, "覆盖值没被自动取回的值顶掉");
    assert_eq!(cached.value_json.as_deref(), Some("\"手写\""));

    // 再解析一次：读到手工值，且不再取数。
    let again = CountingFetcher::new("\"不该取数\"");
    let second = resolve_names(
        &context(&pool),
        Some("p1"),
        &["sns".to_string()],
        false,
        &again,
    )
    .await
    .expect("解析");
    assert_eq!(again.calls(), 0, "覆盖值生效时不该取数");
    assert_eq!(second.values.get("sns").map(String::as_str), Some("手写"));
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
}

/// 刷新要能换掉取数开始前就有的覆盖值。
///
/// 只有 `force` 会在一片有效覆盖值上开始取数（`cache::decide` 特意跳过它），而那个跳过是
/// 可见的：来源徽标从「手动改的」变回「自动取回」。写回侧的守卫若连这份值也保护，刷新就
/// 变成「取回了新值但库里还是手工值」，重载一次界面又变回手工值。
#[tokio::test]
async fn a_refresh_replaces_the_override_that_was_there_before_the_fetch() {
    let (pool, _dir) = seeded_pool();
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"手写\"".into()),
                fetched_at: 1,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写覆盖值");

    // 普通解析先确认：覆盖值生效，取数器不该被叫。
    let idle = CountingFetcher::new("\"取回\"");
    let before = resolve_names(
        &context(&pool),
        Some("p1"),
        &["sns".to_string()],
        false,
        &idle,
    )
    .await
    .expect("解析");
    assert_eq!(idle.calls(), 0, "覆盖值生效时不取数");
    assert_eq!(before.values.get("sns").map(String::as_str), Some("手写"));

    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let fetcher = CountingFetcher::new("\"取回\"");
    let refreshed = resolve_definition(&context(&pool), &definition, true, &fetcher)
        .await
        .expect("刷新");
    assert_eq!(fetcher.calls(), 1, "刷新要真的取数");
    assert_eq!(
        refreshed.value.as_deref(),
        Some("取回"),
        "刷新拿到的是取回的新值"
    );
    assert!(
        refreshed.expires_at.is_some(),
        "写进缓存了就该有到期时间：{:?}",
        refreshed.expires_at
    );

    // 库里那份手工值被换成了取回值。
    let cached = db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", ""))
        .expect("读缓存")
        .expect("有缓存行");
    assert!(!cached.is_override, "覆盖值已被刷新替换");
    assert_eq!(cached.value_json.as_deref(), Some("\"取回\""));

    // 再普通解析一次：读到取回值，而不是手工值。
    let again = CountingFetcher::new("\"不该取数\"");
    let after = resolve_names(
        &context(&pool),
        Some("p1"),
        &["sns".to_string()],
        false,
        &again,
    )
    .await
    .expect("解析");
    assert_eq!(again.calls(), 0, "缓存新鲜时不再取数");
    assert_eq!(after.values.get("sns").map(String::as_str), Some("取回"));
}

/// 等锁期间写下的覆盖值胜出，这次解析不再发请求。
///
/// 第二次解析在 `decide` 时还看不到缓存行，于是排到单飞行锁上等；等的那段时间用户在另一条
/// 连接上写了覆盖值。拿到锁之后重算一次才能看见它——否则这一趟会白打一次接口，交给调用方
/// 的还可能是自动值而不是用户刚写下的那份。
#[tokio::test]
async fn an_override_written_while_waiting_for_the_lock_wins_without_a_request() {
    let (pool, _dir) = seeded_pool();
    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let (reached_tx, mut reached_rx) = tokio::sync::mpsc::channel(1);
    let (release_tx, release_rx) = tokio::sync::mpsc::channel(1);
    let holder = Arc::new(GatedFetcher {
        calls: AtomicUsize::new(0),
        reached: reached_tx,
        release: tokio::sync::Mutex::new(release_rx),
        body: "\"取回\"".to_string(),
    });

    // 第一次解析占住锁，停在半路。
    let first = tokio::spawn({
        let pool = pool.clone();
        let holder = Arc::clone(&holder);
        async move {
            resolve_definition(&context(&pool), &definition, false, holder.as_ref())
                .await
                .expect("解析")
        }
    });
    wait_for_fetch(&mut reached_rx, "第一次取数").await;

    // 第二次解析同一个名字：它已经算过一次结论（那时没有缓存行），此刻正排队等锁。
    // 等锁那一步不可观测，故用探针确认它已经排上——`sleep` 证明不了这件事，覆盖值早一步
    // 提交的话它会直接命中覆盖值、零取数，断言照样过。
    let (waiter_ctx, mut lock_wait_rx) = context_with_lock_wait(&pool);
    let waiter_fetcher = Arc::new(CountingFetcher::new("\"不该取数\""));
    let second = tokio::spawn({
        let waiter_fetcher = Arc::clone(&waiter_fetcher);
        async move {
            resolve_names(
                &waiter_ctx,
                Some("p1"),
                &["sns".to_string()],
                false,
                waiter_fetcher.as_ref(),
            )
            .await
            .expect("解析")
        }
    });
    wait_for_lock_wait(&mut lock_wait_rx, "第二次解析").await;

    // 等锁期间写了覆盖值。
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"手写\"".into()),
                fetched_at: 1,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写覆盖值");

    release_tx.send(()).await.expect("放行第一次取数");
    let out = first.await.expect("任务");
    assert_eq!(out.value.as_deref(), Some("取回"), "在飞那次照常返回取回值");

    let waited = second.await.expect("任务");
    assert_eq!(
        waiter_fetcher.calls(),
        0,
        "锁后才看见的覆盖值不该再发一次请求"
    );
    assert_eq!(
        waited.values.get("sns").map(String::as_str),
        Some("手写"),
        "返回的是用户刚写下的那份"
    );
    assert_eq!(holder.calls.load(Ordering::SeqCst), 1, "只发过第一次请求");
}

/// 刷新在等锁期间被别人写下的手工值，也算「途中写的」——这次刷新照样取数，但不写回。
///
/// 刷新只该换掉它**进门时**看见的那份覆盖值；等锁那段时间用户又写了一份，那是新的手工值，
/// 写回侧要留住它。判定取进门那一刻，故这里 `had_live_override` 为假，写回被跳过。
#[tokio::test]
async fn a_refresh_waits_out_an_override_written_during_the_wait() {
    let (pool, _dir) = seeded_pool();
    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let (reached_tx, mut reached_rx) = tokio::sync::mpsc::channel(1);
    let (release_tx, release_rx) = tokio::sync::mpsc::channel(1);
    let holder = Arc::new(GatedFetcher {
        calls: AtomicUsize::new(0),
        reached: reached_tx,
        release: tokio::sync::Mutex::new(release_rx),
        body: "\"取回\"".to_string(),
    });

    let first = tokio::spawn({
        let pool = pool.clone();
        let definition = definition.clone();
        let holder = Arc::clone(&holder);
        async move {
            resolve_definition(&context(&pool), &definition, false, holder.as_ref())
                .await
                .expect("解析")
        }
    });
    wait_for_fetch(&mut reached_rx, "第一次取数").await;

    // 刷新：进门时缓存里没有覆盖值，于是排到锁上等。探针确认它已经排上，再写覆盖值。
    let (refreshing_ctx, mut lock_wait_rx) = context_with_lock_wait(&pool);
    let refreshing = Arc::new(CountingFetcher::new("\"刷新取回\""));
    let second = tokio::spawn({
        let refreshing = Arc::clone(&refreshing);
        async move {
            resolve_definition(&refreshing_ctx, &definition, true, refreshing.as_ref())
                .await
                .expect("刷新")
        }
    });
    wait_for_lock_wait(&mut lock_wait_rx, "刷新").await;

    // 等锁期间写了覆盖值：它比任何一次取数都新。
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"等锁时手写\"".into()),
                fetched_at: 1,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写覆盖值");

    release_tx.send(()).await.expect("放行第一次取数");
    first.await.expect("任务");
    let refreshed = second.await.expect("任务");

    assert_eq!(
        refreshing.calls(),
        1,
        "刷新照样取数：它只该换掉进门时看见的那份"
    );
    assert_eq!(
        refreshed.value.as_deref(),
        Some("刷新取回"),
        "这次刷新返回的是取回值"
    );

    let cached = db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", ""))
        .expect("读缓存")
        .expect("有缓存行");
    assert!(cached.is_override, "等锁期间写下的手工值没被顶掉");
    assert_eq!(cached.value_json.as_deref(), Some("\"等锁时手写\""));
}

/// 刷新不该冲掉取数途中新存下的覆盖值：它只该换掉自己进门时看见的那一份。
///
/// 只看「此刻有没有覆盖值」不够——进门就有一份（刷新该换掉它），但途中用户又存了另一份，
/// 那一份不属于这次刷新。故写回侧按身份比对：还是进门那一份才换，换过了就留住。
#[tokio::test]
async fn a_refresh_does_not_overwrite_an_override_saved_during_the_fetch() {
    let (pool, _dir) = seeded_pool();
    // 进门时已经有一份永久覆盖值。
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"旧手写\"".into()),
                fetched_at: 1,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写覆盖值");

    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");
    let (reached_tx, mut reached_rx) = tokio::sync::mpsc::channel(1);
    let (release_tx, release_rx) = tokio::sync::mpsc::channel(1);
    let fetcher = Arc::new(GatedFetcher {
        calls: AtomicUsize::new(0),
        reached: reached_tx,
        release: tokio::sync::Mutex::new(release_rx),
        body: "\"刷新取回\"".to_string(),
    });

    let task = tokio::spawn({
        let pool = pool.clone();
        let fetcher = Arc::clone(&fetcher);
        async move {
            resolve_definition(&context(&pool), &definition, true, fetcher.as_ref())
                .await
                .expect("刷新")
        }
    });
    wait_for_fetch(&mut reached_rx, "刷新取数").await;

    // 请求还没回来，用户又存了一份覆盖值：这是新的手工值，刷新不该顶掉它。
    db::with_tx(&pool, |c| {
        db::dynamic_variable_cache::upsert(
            c,
            &DynamicCacheEntry {
                definition_id: "d1".into(),
                environment_id: String::new(),
                value_json: Some("\"新手写\"".into()),
                fetched_at: 2,
                expires_at: 1,
                is_override: true,
                override_expires_at: None,
            },
        )
    })
    .expect("写第二份覆盖值");

    release_tx.send(()).await.expect("放行刷新取数");
    let out = task.await.expect("任务");
    assert_eq!(
        out.value.as_deref(),
        Some("刷新取回"),
        "这次取回的值照常返回"
    );
    assert!(
        out.expires_at.is_none(),
        "没写回就没有到期时间：{:?}",
        out.expires_at
    );

    let cached = db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", ""))
        .expect("读缓存")
        .expect("覆盖行还在");
    assert!(cached.is_override, "途中新写的覆盖值还在");
    assert_eq!(cached.value_json.as_deref(), Some("\"新手写\""));
}

/// 等锁期间别人刚写下的新鲜值不是「过期值」。
///
/// 刷新不看有效期（它本来就要跳过缓存），故锁后读到的新鲜值也会走到取数。取数失败时只有
/// **真的过期**的行才配当回退候选：把新鲜值标成「过期值」交给界面，等于告诉用户这份数据
/// 已经旧了。
#[tokio::test]
async fn a_fresh_value_cached_during_the_wait_is_not_returned_as_stale() {
    let (pool, _dir) = seeded_pool();
    db::with_tx(&pool, |c| {
        db::dynamic_variables::update(
            c,
            "d1",
            DynamicVariablePatch {
                stale_fallback: Some(true),
                ..Default::default()
            },
        )
    })
    .expect("允许过期值回退");
    let definition = db::with_conn(&pool, |c| db::dynamic_variables::get(c, "d1")).expect("定义");

    // 第一次解析占住锁，停在半路；放行之后它会把值写进缓存。
    let (reached_tx, mut reached_rx) = tokio::sync::mpsc::channel(1);
    let (release_tx, release_rx) = tokio::sync::mpsc::channel(1);
    let holder = Arc::new(GatedFetcher {
        calls: AtomicUsize::new(0),
        reached: reached_tx,
        release: tokio::sync::Mutex::new(release_rx),
        body: "\"[1]\"".to_string(),
    });
    let first = tokio::spawn({
        let pool = pool.clone();
        let definition = definition.clone();
        let holder = Arc::clone(&holder);
        async move {
            resolve_definition(&context(&pool), &definition, false, holder.as_ref())
                .await
                .expect("解析")
        }
    });
    wait_for_fetch(&mut reached_rx, "第一次取数").await;

    // 刷新排队等锁：它进门时缓存还是空的。
    let (refreshing_ctx, mut lock_wait_rx) = context_with_lock_wait(&pool);
    let refreshing = Arc::new(CountingFetcher::failing());
    let second = tokio::spawn({
        let refreshing = Arc::clone(&refreshing);
        async move {
            resolve_definition(&refreshing_ctx, &definition, true, refreshing.as_ref())
                .await
                .expect("刷新")
        }
    });
    wait_for_lock_wait(&mut lock_wait_rx, "刷新").await;

    release_tx.send(()).await.expect("放行第一次取数");
    first.await.expect("任务");

    let refreshed = second.await.expect("刷新");
    assert_eq!(
        refreshed.source,
        DynamicValueSource::Missing,
        "新鲜值不是过期值：{:?}",
        refreshed.source
    );
    assert!(
        refreshed.value.is_none(),
        "取数失败且没有过期值可用：{:?}",
        refreshed.value
    );
    assert!(refreshed.error.is_some(), "取数失败仍要记一条错误");

    let cached = db::with_conn(&pool, |c| db::dynamic_variable_cache::get(c, "d1", ""))
        .expect("读缓存")
        .expect("缓存行还在");
    assert!(!cached.is_override);
    assert_eq!(cached.value_json.as_deref(), Some("\"[1]\""));
}
