//! `check.rs` 的规则。全部注入假 fetcher，不碰网络、不碰环境变量。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::*;

/// 记录被调用几次的假 fetcher。
#[derive(Clone)]
struct Fake {
    hits: Arc<AtomicUsize>,
    outcome: Result<Release, UpdateError>,
}

impl Fake {
    fn release(tag: &str) -> Self {
        Self {
            hits: Arc::new(AtomicUsize::new(0)),
            outcome: Ok(Release {
                tag_name: tag.to_string(),
                body: Some("## 这一版的改动\n- 修了一个 bug".to_string()),
                html_url: format!("https://github.com/Nnyjk/simple-post/releases/tag/{tag}"),
                draft: false,
                prerelease: false,
            }),
        }
    }

    fn flagged(tag: &str, draft: bool, prerelease: bool) -> Self {
        let mut fake = Self::release(tag);
        if let Ok(release) = fake.outcome.as_mut() {
            release.draft = draft;
            release.prerelease = prerelease;
        }
        fake
    }

    fn failing(error: UpdateError) -> Self {
        Self {
            hits: Arc::new(AtomicUsize::new(0)),
            outcome: Err(error),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }
}

impl ReleaseFetcher for Fake {
    fn latest(&self) -> Pin<Box<dyn Future<Output = Result<Release, UpdateError>> + Send + '_>> {
        let hits = Arc::clone(&self.hits);
        let outcome = self.outcome.clone();
        Box::pin(async move {
            hits.fetch_add(1, Ordering::Relaxed);
            outcome
        })
    }
}

fn checker(fake: &Fake) -> UpdateChecker {
    UpdateChecker::new(Box::new(fake.clone()))
}

#[tokio::test]
async fn newer_release_is_available() {
    let fake = Fake::release("v0.1.35");
    let result = checker(&fake).check(false).await;

    assert_eq!(result.current, current_version());
    assert_eq!(result.latest.as_deref(), Some("0.1.35"));
    assert!(result.available);
    assert!(!result.cached);
    assert_eq!(result.error, None);
    assert!(result.checked_at.ends_with('Z'), "{}", result.checked_at);
}

#[tokio::test]
async fn same_version_is_not_available() {
    let fake = Fake::release("v0.1.34");
    let result = checker(&fake).check(false).await;

    assert_eq!(result.latest.as_deref(), Some("0.1.34"));
    assert!(!result.available);
    assert_eq!(result.error, None);
}

#[tokio::test]
async fn local_version_ahead_of_the_release_is_not_available() {
    let fake = Fake::release("v0.0.1");
    let result = checker(&fake).check(false).await;

    assert_eq!(result.latest.as_deref(), Some("0.0.1"));
    assert!(!result.available);
    assert_eq!(result.error, None);
}

#[tokio::test]
async fn draft_release_reports_no_release() {
    let fake = Fake::flagged("v0.1.35", true, false);
    let result = checker(&fake).check(false).await;

    assert_eq!(result.error, Some(UpdateError::NoRelease));
    assert_eq!(result.latest, None);
    assert_eq!(result.notes, None);
    assert!(!result.available);
}

#[tokio::test]
async fn prerelease_reports_no_release() {
    let fake = Fake::flagged("v0.1.35-rc1", false, true);
    let result = checker(&fake).check(false).await;

    assert_eq!(result.error, Some(UpdateError::NoRelease));
    assert_eq!(result.latest, None);
}

#[tokio::test]
async fn unreachable_degrades_to_an_error_field() {
    let fake = Fake::failing(UpdateError::Unreachable);
    let result = checker(&fake).check(false).await;

    assert_eq!(result.error, Some(UpdateError::Unreachable));
    assert_eq!(result.latest, None);
    assert_eq!(result.notes, None);
    assert!(!result.available);
    // 出错也是一份 200 的响应体，故 `current` 仍然要填。
    assert_eq!(result.current, current_version());
}

#[tokio::test]
async fn rate_limited_degrades_to_an_error_field() {
    let fake = Fake::failing(UpdateError::RateLimited);
    let result = checker(&fake).check(false).await;

    assert_eq!(result.error, Some(UpdateError::RateLimited));
    assert_eq!(result.latest, None);
}

#[tokio::test]
async fn notes_and_url_come_from_the_release() {
    let fake = Fake::release("v0.1.35");
    let result = checker(&fake).check(false).await;

    assert_eq!(
        result.notes.as_deref(),
        Some("## 这一版的改动\n- 修了一个 bug")
    );
    assert_eq!(
        result.url.as_deref(),
        Some("https://github.com/Nnyjk/simple-post/releases/tag/v0.1.35")
    );
}

#[tokio::test]
async fn second_check_hits_the_cache_and_fetches_once() {
    let fake = Fake::release("v0.1.35");
    let checker = checker(&fake);

    let first = checker.check(false).await;
    let second = checker.check(false).await;

    assert!(!first.cached);
    assert!(second.cached);
    assert_eq!(second.checked_at, first.checked_at);
    assert_eq!(fake.hits(), 1);
}

#[tokio::test]
async fn expired_cache_refetches() {
    let fake = Fake::release("v0.1.35");
    let checker =
        UpdateChecker::with_limits(Box::new(fake.clone()), Duration::ZERO, FORCE_MIN_INTERVAL);

    assert!(!checker.check(false).await.cached);
    assert!(!checker.check(false).await.cached);
    assert_eq!(fake.hits(), 2);
}

#[tokio::test]
async fn force_bypasses_the_cache_but_only_once_a_minute() {
    let fake = Fake::release("v0.1.35");
    let checker = checker(&fake);

    // 先正常取一次，把缓存填上。
    assert!(!checker.check(false).await.cached);
    // force 无视那份新鲜缓存。
    let forced = checker.check(true).await;
    assert!(!forced.cached);
    assert_eq!(fake.hits(), 2);
    // 60 秒内的第二次 force 被节流，还回去的就是上一次的答案。
    let throttled = checker.check(true).await;
    assert!(throttled.cached);
    assert_eq!(throttled.latest.as_deref(), Some("0.1.35"));
    assert_eq!(fake.hits(), 2);
}

#[tokio::test]
async fn failures_are_not_cached_for_the_full_ttl() {
    let fake = Fake::failing(UpdateError::Unreachable);
    let checker = checker(&fake);

    checker.check(false).await;
    checker.check(false).await;

    // 断网这件事不该被 6 小时的 TTL 钉住：两次非 force 都重新试。
    assert_eq!(fake.hits(), 2);
}

#[test]
fn missing_tag_name_is_a_bad_response() {
    assert_eq!(
        parse_release(r#"{"html_url":"https://example.test"}"#),
        Err(UpdateError::BadResponse)
    );
    assert_eq!(
        parse_release(r#"{"tag_name":"v0.1.35"}"#),
        Err(UpdateError::BadResponse)
    );
    assert_eq!(parse_release("not json"), Err(UpdateError::BadResponse));
}

#[test]
fn release_json_gives_the_fields_this_module_uses() {
    let release = parse_release(
        r#"{"tag_name":"v0.1.35","html_url":"https://example.test/r","body":"  hi  ","draft":false,"prerelease":false}"#,
    )
    .expect("parses");
    assert_eq!(release.tag_name, "v0.1.35");
    assert_eq!(release.body.as_deref(), Some("hi"));
    assert_eq!(release.html_url, "https://example.test/r");
    assert!(!release.draft);
    assert!(!release.prerelease);
}

#[tokio::test]
async fn non_numeric_tag_is_a_bad_response() {
    let fake = Fake::release("vnightly");
    let result = checker(&fake).check(false).await;
    assert_eq!(result.error, Some(UpdateError::BadResponse));
    assert_eq!(result.latest, None);
}

#[test]
fn rate_limit_bodies_are_classified() {
    assert_eq!(
        classify_status(403, r#"{"message":"API rate limit exceeded for 1.2.3.4"}"#),
        UpdateError::RateLimited
    );
    assert_eq!(classify_status(429, "rate limit"), UpdateError::RateLimited);
    // 同样回 403 但没有速率限制提示的，是另一种故障。
    assert_eq!(classify_status(403, "forbidden"), UpdateError::Unreachable);
    assert_eq!(classify_status(404, "not found"), UpdateError::Unreachable);
    assert_eq!(classify_status(500, ""), UpdateError::Unreachable);
}

#[test]
fn rfc3339_renders_utc() {
    assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339_utc(1_762_992_000), "2025-11-13T00:00:00Z");
    assert_eq!(rfc3339_utc(1_763_020_800), "2025-11-13T08:00:00Z");
}

#[test]
fn long_notes_are_clamped_on_a_char_boundary() {
    let notes = "改".repeat(NOTES_LIMIT_BYTES);
    let clamped = clamp_notes(notes);
    assert!(clamped.len() < 2 * NOTES_LIMIT_BYTES);
    assert!(clamped.ends_with("已截断）"));
    assert!(clamped.starts_with('改'));

    let short = "短的".to_string();
    assert_eq!(clamp_notes(short.clone()), short);
}
