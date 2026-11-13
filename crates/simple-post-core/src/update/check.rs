//! 「有没有新版本」的抓取、判定与缓存。
//!
//! 数据源是 GitHub 的 `releases/latest` 单个 JSON。这里只回答一个形状固定的
//! [`UpdateCheck`]，**出错也是 `Ok`**：`GET /api/update/check` 除鉴权之外一律 HTTP 200
//! 返回（`apply` 的 403 / 409 是另一回事），因为渲染层的 `invokeCmd` 把非 2xx 变成异常，
//! 降级信息走 `error` 字段才读得到。
//!
//! 缓存是**进程内**的一处（[`global`]），TTL 6 小时。`force` 绕过 TTL，但同一进程
//! 60 秒内最多强制一次——否则界面上的「检查更新」按钮就是一条打 GitHub 的免费通道。
//! 并发请求不会各自出站：整个 check 持一把异步锁，后到的那个要么命中刚写好的缓存，
//! 要么（force 被节流时）拿到上一次的答案。

use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Mutex;

use super::version;

/// 缓存有效期。
pub const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// 两次强制检查之间的最短间隔。
pub const FORCE_MIN_INTERVAL: Duration = Duration::from_secs(60);

/// 出站超时。DNS 解析、TCP 连接与读完响应体都算在这 8 秒里。
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// 发布说明的上限。超长时按字符边界截断，并在末尾留一行标记。
pub const NOTES_LIMIT_BYTES: usize = 128 * 1024;

/// 发布数据的来源。仓库与 tag 前缀都写死在这里：这三个 REST 路由只服务本仓库的
/// 官方构建，没有「检查别的仓库」这种用法。
pub const RELEASES_URL: &str = "https://api.github.com/repos/Nnyjk/simple-post/releases/latest";

/// `User-Agent` 的形状。GitHub 对没有 `User-Agent` 的请求回 403，而不是回一个空列表。
pub fn user_agent() -> String {
    format!("simple-post/{}", current_version())
}

/// 工作区版本。core 的 `version.workspace = true`，故它就是产品的版本号。
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 分类失败的原因。序列化后的字符串就是响应体 `error` 字段的取值，前端按它选文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateError {
    /// 连接 / 超时 / DNS 失败，或除速率限制之外的非 2xx。
    Unreachable,
    /// GitHub 403 / 429 且响应里带了速率限制提示。
    RateLimited,
    /// 最新 release 是 draft 或 prerelease，即没有可发布的版本。
    NoRelease,
    /// 响应不是预期的 JSON，或 `tag_name` 缺失 / 不是版本号。
    BadResponse,
}

/// `GET /api/update/check` 的响应体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// 本进程的版本，即 `env!("CARGO_PKG_VERSION")`。
    pub current: String,
    /// 最新 tag 去掉前导 `v` 后的版本串；没有可用 release 或出错时为 `None`。
    pub latest: Option<String>,
    /// `latest` 是否比 `current` 新。
    pub available: bool,
    /// 这次判定发生的时刻（RFC3339 UTC）。命中缓存时是那次抓取的时刻。
    pub checked_at: String,
    /// `true` = 这份结果来自进程内缓存，没有出站。
    pub cached: bool,
    /// 发布说明 markdown（GitHub release 的 `body`）；没有可用 release 或出错时为 `None`。
    pub notes: Option<String>,
    /// release 页地址（GitHub release 的 `html_url`）；没有可用 release 或出错时为 `None`。
    pub url: Option<String>,
    /// 失败分类；`None` = 这一次拿到了一份可用的（或明确没有的）release 数据。
    pub error: Option<UpdateError>,
}

/// `releases/latest` 里这个模块要用的那几项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    pub body: Option<String>,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
}

/// 抓一次最新 release。测试注入假实现，故 [`check`] 的规则不依赖网络。
pub trait ReleaseFetcher: Send + Sync {
    fn latest(&self) -> Pin<Box<dyn Future<Output = Result<Release, UpdateError>> + Send + '_>>;
}

/// 真的打 `api.github.com`。
pub struct GithubFetcher;

impl ReleaseFetcher for GithubFetcher {
    fn latest(&self) -> Pin<Box<dyn Future<Output = Result<Release, UpdateError>> + Send + '_>> {
        Box::pin(async {
            let client = reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .user_agent(user_agent())
                .build()
                // 建客户端只在 TLS 后端初始化失败时出错，那不是「远程不可达」之外的
                // 任何东西，故也归到这一类。
                .map_err(|_| UpdateError::Unreachable)?;
            let response = client
                .get(RELEASES_URL)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                // 连接、超时、DNS、TLS 都在这里失败；一个 4xx/5xx 不走这个分支。
                .map_err(|_| UpdateError::Unreachable)?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(classify_status(status.as_u16(), &body));
            }
            let body = response
                .text()
                .await
                .map_err(|_| UpdateError::Unreachable)?;
            parse_release(&body)
        })
    }
}

/// 非 2xx 的分类。GitHub 的未认证配额耗尽时回 403（部分场景回 429），响应体里带
/// `API rate limit exceeded`；同样回 403 而正文里没有这句的（例如组织策略拒绝）是
/// 另一种故障，归 `unreachable`。
fn classify_status(status: u16, body: &str) -> UpdateError {
    if (status == 403 || status == 429) && body.to_ascii_lowercase().contains("rate limit") {
        UpdateError::RateLimited
    } else {
        UpdateError::Unreachable
    }
}

/// `releases/latest` 的 JSON → [`Release`]。只认三个必填项里真正要紧的那两个半：
/// `tag_name` 与 `html_url` 缺失即 [`UpdateError::BadResponse`]（拿不到一份能比的版本号，
/// 或拿不到一个能点的链接，这份响应就没有用途）。
pub fn parse_release(body: &str) -> Result<Release, UpdateError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| UpdateError::BadResponse)?;
    let tag_name = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or(UpdateError::BadResponse)?
        .to_string();
    let html_url = value
        .get("html_url")
        .and_then(|v| v.as_str())
        .ok_or(UpdateError::BadResponse)?
        .to_string();
    Ok(Release {
        tag_name,
        body: value
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        html_url,
        draft: value
            .get("draft")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        prerelease: value
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// 截断发布说明。按 UTF-8 字符边界切，末尾补一行标记——用户看到「截断」才知道
/// 后面还有内容，而不是以为发布说明就断在那里。
fn clamp_notes(notes: String) -> String {
    if notes.len() <= NOTES_LIMIT_BYTES {
        return notes;
    }
    let mut cut = NOTES_LIMIT_BYTES;
    while cut > 0 && !notes.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = notes[..cut].to_string();
    out.push_str("\n\n…（发布说明过长，已截断）");
    out
}

/// 把一份 release 折成响应体。`draft` / `prerelease` 视作「没有可发布的版本」：
/// 这两档的 tag 可能比当前版本新，但它们不是给用户升级的目标。
fn from_release(current: &str, release: Release) -> UpdateCheck {
    let checked_at = rfc3339_utc(now_secs());
    if release.draft || release.prerelease {
        return UpdateCheck {
            current: current.to_string(),
            latest: None,
            available: false,
            checked_at,
            cached: false,
            notes: None,
            url: None,
            error: Some(UpdateError::NoRelease),
        };
    }
    if version::parse(&release.tag_name).is_err() {
        return failure(current, UpdateError::BadResponse);
    }
    // tag 已经过 `parse`，故这里的 `Err` 分支只是 `current` 走样时的兜底。
    let available = version::is_newer(&release.tag_name, current).unwrap_or(false);
    UpdateCheck {
        current: current.to_string(),
        latest: Some(strip_v(&release.tag_name)),
        available,
        checked_at,
        cached: false,
        notes: release.body.map(clamp_notes),
        url: Some(release.html_url),
        error: None,
    }
}

/// 失败响应：`latest` / `notes` / `url` 全空，`available` 为假。
fn failure(current: &str, error: UpdateError) -> UpdateCheck {
    UpdateCheck {
        current: current.to_string(),
        latest: None,
        available: false,
        checked_at: rfc3339_utc(now_secs()),
        cached: false,
        notes: None,
        url: None,
        error: Some(error),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// unix 秒 → `YYYY-MM-DDTHH:MM:SSZ`。不拉一个日期库只为这一处格式化。
fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// Howard Hinnant 的 `civil_from_days`：unix 日序 → 公历年月日。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}

/// 版本串去掉一个前导 `v`：`url` 里的 tag 带 `v`，而 `latest` 字段不带。
fn strip_v(tag: &str) -> String {
    tag.trim().strip_prefix('v').unwrap_or(tag).to_string()
}

#[derive(Default)]
struct CacheState {
    /// 最近一次**可用**的结果（`error == None`）。TTL 只对它生效：失败不缓存，
    /// 否则一台断网的机器会连着 6 小时都报同一次失败。
    fresh: Option<UpdateCheck>,
    fresh_at: Option<SystemTime>,
    /// 最近一次结果，含失败。force 被节流而缓存里又没有可用结果时返回它。
    last: Option<UpdateCheck>,
    last_force: Option<SystemTime>,
}

/// 一个进程一份缓存。测试直接构造自己的实例，不碰全局那一份。
pub struct UpdateChecker {
    fetcher: Box<dyn ReleaseFetcher>,
    cache_ttl: Duration,
    force_min_interval: Duration,
    state: Mutex<CacheState>,
}

impl UpdateChecker {
    pub fn new(fetcher: Box<dyn ReleaseFetcher>) -> Self {
        Self::with_limits(fetcher, CACHE_TTL, FORCE_MIN_INTERVAL)
    }

    pub fn with_limits(
        fetcher: Box<dyn ReleaseFetcher>,
        cache_ttl: Duration,
        force_min_interval: Duration,
    ) -> Self {
        Self {
            fetcher,
            cache_ttl,
            force_min_interval,
            state: Mutex::new(CacheState::default()),
        }
    }

    /// 取一份判定。`force` 绕过 TTL，但受 60 秒节流。
    ///
    /// 锁从头持有到尾：后到的并发请求因此不会各自出站——第一个写进缓存后，
    /// 非 force 的那些直接命中它。
    pub async fn check(&self, force: bool) -> UpdateCheck {
        let mut state = self.state.lock().await;
        let now = SystemTime::now();

        if force {
            if force_allowed(&state, now, self.force_min_interval) {
                state.last_force = Some(now);
                return self.refresh(&mut state).await;
            }
            // 节流：能还的就还上一次的答案。缓存里一份都没有时（第一次调用就是 force
            // 且上一个进程状态为空）没有可还的，照常抓一次。
            if let Some(previous) = state.last.clone() {
                return UpdateCheck {
                    cached: true,
                    ..previous
                };
            }
            return self.refresh(&mut state).await;
        }

        if let (Some(fresh), Some(at)) = (state.fresh.as_ref(), state.fresh_at) {
            if now
                .duration_since(at)
                .map(|age| age < self.cache_ttl)
                .unwrap_or(false)
            {
                return UpdateCheck {
                    cached: true,
                    ..fresh.clone()
                };
            }
        }
        self.refresh(&mut state).await
    }

    async fn refresh(&self, state: &mut CacheState) -> UpdateCheck {
        let current = current_version();
        let outcome = match self.fetcher.latest().await {
            Ok(release) => from_release(current, release),
            Err(error) => failure(current, error),
        };
        if outcome.error.is_none() {
            state.fresh = Some(outcome.clone());
            state.fresh_at = Some(SystemTime::now());
        }
        state.last = Some(outcome.clone());
        outcome
    }
}

fn force_allowed(state: &CacheState, now: SystemTime, min_interval: Duration) -> bool {
    match state.last_force {
        None => true,
        Some(at) => now
            .duration_since(at)
            .map(|age| age >= min_interval)
            .unwrap_or(true),
    }
}

/// 进程内唯一的那份缓存。三条 REST 路由都经过它，所以「6 小时一次」是进程级的。
pub fn global() -> &'static UpdateChecker {
    static GLOBAL: LazyLock<UpdateChecker> =
        LazyLock::new(|| UpdateChecker::new(Box::new(GithubFetcher)));
    &GLOBAL
}

/// 走全局缓存做一次判定。
pub async fn check(force: bool) -> UpdateCheck {
    global().check(force).await
}

#[cfg(test)]
#[path = "check_test.rs"]
mod tests;
