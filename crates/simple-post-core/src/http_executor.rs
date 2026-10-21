//! HTTP 执行层。
//!
//! 纯 I/O——不含业务逻辑。调用方（Tauri command、MCP 守护进程或将来的 cron worker）构造
//! [`HttpRequestInput`]、交给 [`execute`]、拿回一个 [`HttpResult`]，其形状与 React 渲染层
//! 已经理解的线契约一致（见 `src/lib/http.ts::ApiResponse`）。
//!
//! 本模块是后端里唯一与 `reqwest` 说话的地方。把线类型与真正的网络调用从 `lib.rs` 里拆出来，
//! 使这一层能被 MCP 守护进程复用，而不必拖进 Tauri 的 `#[command]` 宏管线（以及随之而来的
//! 宏命名空间冲突——见 `lib.rs` 里 `http_request` 的 NOTE）。

use std::time::{Duration, Instant};

use serde::Serialize;

// 线类型——镜像 TS 侧的 `ApiResponse` / `HttpError`。

/// 成功的 HTTP 响应。字段名与 `ApiResponse` 一致，故 React 侧可以复用它现有的响应视图代码
/// 而无需翻译层。
/// `rename_all = "camelCase"` 是承重的，不是装饰：没有它 serde 会发出 `status_text` /
/// `duration_ms` / `size_bytes` / `content_type`，而 `src/lib/http.ts` 读的是
/// `statusText` / `durationMs` / `sizeBytes` / `contentType`——每一个都读回 `undefined`，
/// 于是响应栏对每个请求都渲染成 `NaN s` 与 `0 B`，content type 也永远到不了阅读器。
/// 下面的 `HttpResult` 本就有这个属性；本类型当时漏了。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u128,
    pub size_bytes: u64,
    /// 扁平化为 `[name, value]` 对的响应头（匹配 JS 侧的 `headersToObject` 形状）。
    pub headers: Vec<[String; 2]>,
    pub body: String,
    pub content_type: String,
    pub method: String,
    pub url: String,
    /// 响应体是否在服务端被裁到 `max_bytes`。
    pub truncated: bool,
}

/// 传输层失败。与 `HttpResponse.status == 0` 不同——后者是前端用来表示 HTTP 层的非 2xx。
/// 本类型表示「请求从未抵达应用层」——DNS、连接、TLS、重置、中止、超时。
/// `rename_all` 今天是空操作（两个字段都是单词），但它使本类型不会重新引入
/// [`HttpResponse`] 曾有的那种不匹配。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpError {
    /// `timeout` | `network`（DNS / 连接 / TLS / 重置 / 中止）
    pub kind: String,
    pub message: String,
}

/// 判别式联合——比裸枚举更好从 TS 消费，因为字段形状不随变体而变。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<HttpResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<HttpError>,
}

impl HttpResult {
    fn ok(r: HttpResponse) -> Self {
        Self {
            ok: true,
            response: Some(r),
            error: None,
        }
    }
    fn err(kind: &str, msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            response: None,
            error: Some(HttpError {
                kind: kind.into(),
                message: msg.into(),
            }),
        }
    }
}

// 输入——调用方交接前构造的东西。

/// `execute` 发出一个请求所需的全部信息。保持扁平结构体（而非 builder），因为每个调用方本来
/// 就已经知道全部字段——构造逻辑住在 `crate::variable_fill`，不在这里。
#[derive(Debug, Clone)]
pub struct HttpRequestInput {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

// 内部实现——小到把它们内联为 `pub(crate)` 能保持公开面干净。lib.rs 里的 Tauri command 与
// 将来的 MCP 守护进程都调 `execute`；别的都不需要碰这些。

/// 把文本形式的 HTTP 方法解析成 `reqwest::Method`，未知动词回退 GET（前端已经在已知列表上
/// 把过一道）。
pub(crate) fn parse_method(method: &str) -> reqwest::Method {
    match method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => reqwest::Method::from_bytes(other.as_bytes()).unwrap_or(reqwest::Method::GET),
    }
}

/// 把 body 字符串裁到至多 `max_bytes` 字节（落在 UTF-8 安全的字符边界上）。被裁时加上一个
/// 标记。与原 fetch 路径的截断契约一致。
///
/// 这条契约同时是**流式读取路径**的裁剪判据：流式路径把「读到的原始字节」解码成字符串后
/// 交给本函数，故裁剪点与标记文案只有这一个产生点，不会出现两条路径各裁各的。
pub(crate) fn truncate_body(body: String, max_bytes: u64) -> (String, bool) {
    if max_bytes == 0 {
        return (body, false);
    }
    if body.len() as u64 <= max_bytes {
        return (body, false);
    }
    let cap = (max_bytes as usize).min(body.len());
    // 回退到最近的字符边界，免得把一个多字节 rune 切成两半。
    let mut cut = cap;
    while cut > 0 && !body.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(cut + 96);
    out.push_str(&body[..cut]);
    out.push_str(&format!(
        "\n\n[truncated at {} bytes by Tauri HTTP layer]",
        max_bytes
    ));
    (out, true)
}

/// 上限之上多读 4 字节：保证起始于上限之前的 rune 完整，[`truncate_body`] 的字符边界
/// 回退才与整读落在同一个偏移上。
const BOUNDARY_LOOKAHEAD_BYTES: u64 = 4;

/// 流式读取响应体：边读边计数，累计到上限就停，剩下的字节直接丢弃。
///
/// 缓冲区的大小只与上限有关，与响应大小无关。
///
/// `max_bytes == 0` 表示不设上限，整个 body 都要读进来。
///
/// 解码须与未开 `charset` 的 `Response::text()` 一致（UTF-8 宽松解码）。
///
/// 不用 `Content-Length` 判截断：它描述的是编码后的长度，与 [`truncate_body`] 的判据
/// 分叉，同一个响应会在两种形态下给出不同的 `truncated`。
async fn read_body_capped(
    mut res: reqwest::Response,
    max_bytes: u64,
) -> Result<String, reqwest::Error> {
    let cap = if max_bytes == 0 {
        u64::MAX
    } else {
        max_bytes.saturating_add(BOUNDARY_LOOKAHEAD_BYTES)
    };

    let mut buf: Vec<u8> = Vec::new();
    let mut len: u64 = 0;
    while let Some(chunk) = res.chunk().await? {
        let take = std::cmp::min(chunk.len() as u64, cap - len) as usize;
        buf.extend_from_slice(&chunk[..take]);
        len += take as u64;
        if len >= cap {
            // 读够了：剩下的字节直接丢弃，连接随之关闭，服务端不会把剩下的内容继续推过来。
            break;
        }
    }

    // 合法 UTF-8 时 `String::from_utf8` 原地校验、不复制（`from_utf8_lossy` 要复制一份）。
    match String::from_utf8(buf) {
        Ok(text) => Ok(text),
        Err(e) => Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

// 出站 URL 策略。

/// [`execute`] 对「允许连到哪里」有多宽松。
/// 两个独立的旋钮，因为不同形态的正确答案不同：
/// * **桌面（Tauri）**——本机用户**就是**操作者。调 `http://localhost:3000` 是 API 客户端
///   最常见的动作，故默认就封掉私有网段会直接毁掉产品。只有 scheme 白名单生效。
/// * **Web / Docker**——服务端网络可达，且所有调用方共用同一个静态 bearer 口令。此时服务端
///   就是一台开放代理，除非拒绝私有网段，故运维应设 `SIMPLE_POST_BLOCK_PRIVATE_NET=1`。
///
/// 两种策略下 `execute` 都拒绝非 `http(s)` 的 URL：`file:`、`gopher:` 之类会把一个调试工具
/// 变成读本地文件的通道，而 API 客户端对它们没有任何正当用法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboundPolicy {
    /// 拒绝 loopback / 私有 / link-local / unique-local 地址。由
    /// `SIMPLE_POST_BLOCK_PRIVATE_NET` 驱动。
    pub block_private_net: bool,
}

impl Default for OutboundPolicy {
    /// 桌面友好的默认值：只有 scheme 白名单。
    fn default() -> Self {
        Self {
            block_private_net: false,
        }
    }
}

impl OutboundPolicy {
    /// 从环境读取策略。放在这里而非各调用点，好让桌面与 Web 两种形态无法分叉。
    pub fn from_env() -> Self {
        let flag = std::env::var("SIMPLE_POST_BLOCK_PRIVATE_NET")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes" || v == "on"
            })
            .unwrap_or(false);
        Self {
            block_private_net: flag,
        }
    }
}

/// 目标 URL 对 [`OutboundPolicy`] 检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlVerdict {
    Allowed,
    /// 请求被拒的人读理由。渲染进与传输失败同一个 `HttpResult` 封装，好让 UI 用普通的错误
    /// 横幅显示它。
    Refused(String),
}

/// 判定为绝不该通过服务端代理抵达的 IP：loopback、RFC1918、link-local（含
/// 169.254.169.254 处的云元数据）、运营商级 NAT、以及 IPv6 unique-local。
/// 此处**不**解析主机名：解析会在检查与拨号之间开出一个 DNS rebinding 窗口；需要基于名字的
/// 策略时，调用方应当显式地接受那个取舍，而不是被悄悄上一个半吊子的措施。
fn is_forbidden_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // 100.64.0.0/10——运营商级 NAT，k8s 里常见。
                || (v4.octets()[0] == 100
                    && (64..=127).contains(&v4.octets()[1]))
                // 0.0.0.0/8——「本网络」，解析后即 localhost。
                || v4.octets()[0] == 0
                // 240.0.0.0/4——保留，含 255.255.255.255。
                || v4.octets()[0] >= 240
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                // fc00::/7——unique local。
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10——link local。
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // ::ffff:a.b.c.d——IPv4 映射；检查内嵌的 v4。
                || v6
                    .to_ipv4_mapped()
                    .map(|v| is_forbidden_ip(std::net::IpAddr::V4(v)))
                    .unwrap_or(false)
        }
    }
}

/// 把 [`OutboundPolicy`] 施加到一个目标 URL 字符串上。
pub fn check_url(url: &str, policy: OutboundPolicy) -> UrlVerdict {
    // `reqwest::Url` 就是 `url` crate 的重导出，故这里不需要工作区里多一个依赖。
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        Err(e) => return UrlVerdict::Refused(format!("invalid URL `{url}`: {e}")),
    };

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return UrlVerdict::Refused(format!(
                "scheme `{other}://` is not allowed — only http and https are"
            ))
        }
    }

    if !policy.block_private_net {
        return UrlVerdict::Allowed;
    }

    // `host_str()` 保持 IPv6 的 `[::1]` 形态，故解析前先去掉方括号。不是 IP 字面量的名字被
    // 原样放过：主机名在此**刻意**不被解析，因为解析会在这次检查与拨号之间开出一个
    // DNS rebinding 窗口。
    let host_forbidden = parsed
        .host_str()
        .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|h| h.parse::<std::net::IpAddr>().ok())
        .map(is_forbidden_ip)
        .unwrap_or(false);

    if host_forbidden {
        UrlVerdict::Refused(format!(
            "`{url}` resolves to a private, loopback or link-local address, \
             and SIMPLE_POST_BLOCK_PRIVATE_NET is enabled"
        ))
    } else {
        UrlVerdict::Allowed
    }
}

// 公开入口——真正的那次 HTTP 调用。

/// 经 `reqwest` 发出真实 HTTP 请求，绕开浏览器 CORS。纯异步 I/O；无共享状态。
/// 前端调 `lib.rs` 里的 `http_request` Tauri command，由它转发到这里。MCP 守护进程
/// （`crate::mcp`）今天经它自己的 `execute_http` 转发层路由请求，待该兼容层退役后会
/// 直接调这里。
pub async fn execute(input: &HttpRequestInput) -> HttpResult {
    execute_with_policy(input, OutboundPolicy::from_env()).await
}

/// 带显式策略的 [`execute`]。测试直接用它；由环境驱动的 [`execute`] 是生产入口。
pub async fn execute_with_policy(input: &HttpRequestInput, policy: OutboundPolicy) -> HttpResult {
    let start = Instant::now();

    // 在建客户端或碰网络之前就拒绝。
    if let UrlVerdict::Refused(reason) = check_url(&input.url, policy) {
        return HttpResult::err("network", reason);
    }

    // 每次调用一个 Client，好让 API 保持请求作用域；将来若想要跨调用的连接池，
    // 可以把它提到 `tauri::State`。
    //
    // 重定向策略在**每一跳**上都重跑 `check_url`。只查初始 URL 会让整道守卫变成摆设：
    // 开着 `follow_redirects`（默认）时，一个回答 `302 Location: http://169.254.169.254/…`
    // 的攻击者可控 URL 一跳就能摸到元数据服务，而 `Policy::limited(10)` 会愉快地跟着跳。
    // `custom` 给出逐跳控制；跳数预算在这里计数，因为 `custom` 是**取代** `limited` 而非
    // 与它组合。
    const MAX_REDIRECTS: usize = 10;
    let redirect = if input.follow_redirects {
        let hops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        reqwest::redirect::Policy::custom(move |attempt| {
            let n = hops.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match check_url(attempt.url().as_str(), policy) {
                UrlVerdict::Allowed => attempt.follow(),
                UrlVerdict::Refused(reason) => attempt.error(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    reason,
                ))
                    as Box<dyn std::error::Error + Send + Sync>),
            }
        })
    } else {
        reqwest::redirect::Policy::none()
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(input.timeout_ms))
        .redirect(redirect)
        .build()
    {
        Ok(c) => c,
        Err(e) => return HttpResult::err("network", format!("client build: {e}")),
    };

    let mut req = client.request(parse_method(&input.method), &input.url);
    for (k, v) in &input.headers {
        if !k.is_empty() {
            req = req.header(k, v);
        }
    }
    if let Some(ref b) = input.body {
        req = req.body(b.clone());
    }

    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, e.to_string());
        }
    };

    let status = res.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let header_pairs: Vec<[String; 2]> = res
        .headers()
        .iter()
        .map(|(k, v)| [k.to_string(), v.to_str().unwrap_or("").to_string()])
        .collect();

    // 响应体上限。必须在读 body **之前**算好：流式读取要靠它决定读多少就停。
    let max_bytes = input.max_response_size_kb.saturating_mul(1024);

    // 读取时护栏：缓冲区上限与响应大小无关，超出的部分直接丢弃。
    let body_text = match read_body_capped(res, max_bytes).await {
        Ok(t) => t,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, format!("read body: {e}"));
        }
    };

    let elapsed = start.elapsed().as_millis();
    let (final_body, truncated) = truncate_body(body_text, max_bytes);

    HttpResult::ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        duration_ms: elapsed,
        size_bytes: final_body.len() as u64,
        headers: header_pairs,
        body: final_body,
        content_type,
        method: input.method.clone(),
        url: input.url.clone(),
        truncated,
    })
}

// 测试——内部辅助函数 + 结果封装。

#[cfg(test)]
#[path = "http_executor_test.rs"]
mod tests;
