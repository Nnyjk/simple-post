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
    KeyValue, Project, RequestBody, ValueMode,
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
        name: "sns".into(),
        description: None,
        source_endpoint_id: None,
        source_endpoint_name: None,
        source_inline_json: Some(INLINE_SOURCE.to_string()),
        source_environment_id: None,
        source_base_url_definition_id: None,
        extract_path: "$".into(),
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
    // 覆盖值过期了 → 重新取数。定义允许过期值兜底时，那个过期的覆盖值就是兜底候选。
    let def = definition(60, true);
    let e = entry(true, 1_000, Some(500));
    assert_eq!(
        decide(&def, Some(&e), 900, false),
        Decision::Fetch {
            stale_fallback: Some("\"v\"".to_string())
        }
    );

    // 定义不允许兜底时没有候选。
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
                default_base_url_definition_id: None,
                base_url_definitions: vec![],
                base_url: None,
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
    ResolveContext { pool: pool.clone() }
}

#[tokio::test]
async fn a_resolution_fetches_the_value_and_caches_it() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[{\"code\":\"A001\"}]");

    let first = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(
        first.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"A001\"}]")
    );
    assert_eq!(fetcher.calls(), 1);

    // 第二次命中缓存，不再取数。
    let second = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 1);
    assert_eq!(
        second.values.get("sns").map(String::as_str),
        Some("[{\"code\":\"A001\"}]")
    );
}

#[tokio::test]
async fn a_force_ignores_the_cache() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::new("[]");

    resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    resolve_names(&ctx, &["sns".to_string()], true, &fetcher)
        .await
        .expect("解析");
    assert_eq!(fetcher.calls(), 2);
}

#[tokio::test]
async fn a_failing_fetch_leaves_the_name_missing_and_reports_the_reason() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let fetcher = CountingFetcher::failing();

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
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

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
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

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
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

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors[0].kind, DynamicErrorKind::NoMatch);
}

#[tokio::test]
async fn a_value_that_is_too_large_is_not_written_to_the_cache() {
    let (pool, _dir) = seeded_pool();
    let ctx = context(&pool);
    let big = format!("\"{}\"", "x".repeat(MAX_VALUE_BYTES + 10));
    let fetcher = CountingFetcher::new(&big);

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
        .await
        .expect("解析");
    assert_eq!(out.errors[0].kind, DynamicErrorKind::TooLarge);

    // 内联数据源没有环境，缓存键的环境段就是空串。
    let cached = db::with_conn(&pool, |c| {
        db::dynamic_variable_cache::get(c, "d1", crate::db::dynamic_variable_cache::NO_ENVIRONMENT)
    })
    .expect("read");
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
            resolve_names(&ctx, &["sns".to_string()], false, fetcher.as_ref())
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

    let out = resolve_names(&ctx, &["sns".to_string()], false, &fetcher)
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

    let out = resolve_names(&ctx, &["token".to_string()], false, &fetcher)
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
    resolve_names(&ctx, &["sns".to_string()], false, &good)
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
    let out = resolve_names(&ctx, &["sns".to_string()], false, &bad)
        .await
        .expect("解析");
    assert_eq!(out.values.get("sns").map(String::as_str), Some("[1]"));
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.statuses[0].source, DynamicValueSource::Stale);
}
