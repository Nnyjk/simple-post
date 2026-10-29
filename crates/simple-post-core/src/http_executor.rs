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

use base64::Engine as _;
use serde::Serialize;

use crate::domain::TlsSettings;

// 线类型——镜像 TS 侧的 `ApiResponse` / `HttpError`。

/// 成功的 HTTP 响应。字段名与 `ApiResponse` 一致，故 React 侧可以复用它现有的响应视图代码
/// 而无需翻译层。
/// `rename_all = "camelCase"` 是必需的属性，不是装饰：没有它 serde 会发出 `status_text` /
/// `duration_ms` / `size_bytes` / `content_type`，而 `src/lib/http.ts` 读的是
/// `statusText` / `durationMs` / `sizeBytes` / `contentType`——每一个都读回 `undefined`，
/// 于是响应栏对每个请求都渲染成 `NaN s` 与 `0 B`，content type 也永远到不了阅读器。
/// `HttpResult` 与 `HttpError` 都带同一个属性，故两个类型的线格式一致。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u128,
    pub size_bytes: u64,
    /// 扁平化为 `[name, value]` 对的响应头（匹配 JS 侧的 `headersToObject` 形状）。
    pub headers: Vec<[String; 2]>,
    /// 这一次**真正发出去**的请求头。
    ///
    /// 不只是调用方交下来的那些：传输层还会补上 `host`、`content-length` 与
    /// `accept-encoding`，而响应详情页要展示的是线上那一份，故这里给的是补齐后的集合。
    /// 名字一律小写，与线路上的写法一致。
    pub request_headers: Vec<[String; 2]>,
    pub body: String,
    /// 响应体的**二进制形态**：原始字节的 base64。
    ///
    /// 响应字节不是合法 UTF-8 时才有它（图片、压缩包、PDF…），此时 `body` 是宽松解码
    /// 后的结果——非法字节已被替换成 U+FFFD，原始字节不可恢复。要预览图片、要下载文件，
    /// 都得走这条通道。
    ///
    /// 字节是合法 UTF-8 时**不给**这条通道：文本响应不需要它，给了只是让每个响应多带
    /// 一份同内容的拷贝。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    pub content_type: String,
    pub method: String,
    pub url: String,
    /// 响应体是否在服务端被裁到 `max_bytes`。
    pub truncated: bool,
}

/// 传输层失败。与 `HttpResponse.status == 0` 不同——后者是前端用来表示 HTTP 层的非 2xx。
/// 本类型表示「请求从未抵达应用层」——DNS、连接、TLS、重置、中止、超时。
/// `rename_all` 当前是空操作（两个字段都是单词），但它必须与 [`HttpResponse`] 用同一个
/// 属性，否则两个类型的线格式会不一致。
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
    /// 请求体的**文本形态**。
    pub body: Option<String>,
    /// 请求体的**二进制形态**：原始字节的 base64。
    ///
    /// 有它时 `body` 缺席，线上发出去的是解码后的字节。multipart 带文件分片时整个
    /// 请求体是字节，故走这条通道；纯文本的请求体仍走 `body`。
    ///
    /// 两者都有时以本字段为准：调用方交下来的 body 形态与字节形态不一致时，发出去的是
    /// 字节那份——它才是能被还原成文件的那份。
    pub body_base64: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
    /// HTTPS 出站设置。**已经是合成过的那一份**：全局默认与端点覆盖的合成由
    /// [`crate::tls::resolve`] 完成，不在这里做，故执行层没有第二套合并规则。
    pub tls: TlsSettings,
}

// 内部实现——小到把它们内联为 `pub(crate)` 能保持公开接口干净。lib.rs 里的 Tauri command 与
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
    // cut 回退到最近的字符边界，免得把一个多字节 rune 切成两半。
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

/// 把字节裁到至多 `max_bytes` 字节。
///
/// 与 [`truncate_body`] 的差别有两条，都是二进制形态逼出来的：
///
/// * 切点落在**字节**上，不落字符边界——原始字节里没有字符边界这回事。
/// * 不追加截断标记——标记是文本，插进字节流里会污染它。是否被裁过由
///   [`ResponsePayload::truncated`] 单独报出。
fn truncate_bytes(bytes: &[u8], max_bytes: u64) -> (Vec<u8>, bool) {
    if max_bytes == 0 || bytes.len() as u64 <= max_bytes {
        return (bytes.to_vec(), false);
    }
    (bytes[..max_bytes as usize].to_vec(), true)
}

/// 响应体的两种形态。
///
/// 文本形态是宽松解码后的字符串；二进制形态是原始字节的 base64。**判据只有一条：字节
/// 是不是合法 UTF-8**——是则只有文本形态，否则两种形态都给（文本那份已被破坏过）。
pub(crate) struct ResponsePayload {
    /// 文本形态。
    pub text: String,
    /// 二进制形态。字节是合法 UTF-8 时为 `None`。
    pub base64: Option<String>,
    /// 响应体有多少字节。
    pub bytes: u64,
    /// 响应体被裁过。
    pub truncated: bool,
}

/// 把读回来的原始字节分成文本与二进制两种形态。
///
/// 文本形态走的是 [`truncate_body`]：宽松解码后再按字符边界裁，与对照读法
/// （`Response::text()` 之后交给同一个函数）逐字节一致。**先裁字节再解码会给出另一个
/// 答案**——落在裁点上的半个多字节字符在两种顺序下解出不同的东西。
///
/// 二进制形态按字节裁：它给的是原始字节，字符边界对它没有意义。
///
/// **形态的判据是「内容是不是文本」，不是「读到的这段字节是不是合法 UTF-8」**：读到的
/// 字节是按上限裁过的，裁点落在某个多字节字符中间时，一个纯文本响应读回来也是非法
/// UTF-8。故先问 Content-Type，再问字节。
pub(crate) fn split_response_payload(
    raw: Vec<u8>,
    max_bytes: u64,
    content_type: &str,
) -> ResponsePayload {
    let is_text = is_textual_content_type(content_type) || std::str::from_utf8(&raw).is_ok();
    let (text, text_truncated) =
        truncate_body(String::from_utf8_lossy(&raw).into_owned(), max_bytes);

    if is_text {
        return ResponsePayload {
            bytes: text.len() as u64,
            text,
            base64: None,
            truncated: text_truncated,
        };
    }

    let (bytes, bytes_truncated) = truncate_bytes(&raw, max_bytes);
    ResponsePayload {
        bytes: bytes.len() as u64,
        text: String::new(),
        base64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        truncated: bytes_truncated,
    }
}

/// 这个 Content-Type 说它的内容是文本吗？
///
/// 主类型是 `text`，或子类型以 `json` / `xml` 结尾，或子类型是那几个明确的文本类型
/// （`javascript`、`x-www-form-urlencoded`、`graphql`）。
///
/// `application/octet-stream`、`image/*`、`application/pdf`、`application/zip` 都不算：
/// 它们的内容是字节，宽松解码会把它们弄坏。
fn is_textual_content_type(content_type: &str) -> bool {
    let main = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if main.starts_with("text/") {
        return true;
    }
    let Some(subtype) = main.split_once('/').map(|(_, s)| s) else {
        return false;
    };
    subtype.ends_with("json")
        || subtype.ends_with("xml")
        || matches!(
            subtype,
            "javascript" | "x-www-form-urlencoded" | "graphql" | "yaml" | "x-yaml"
        )
}

/// 请求体的字节。
///
/// `body_base64` 优先于 `body`：两者都有时以字节那份为准——它才是能被还原成文件的那份。
/// base64 解不开时返回一条说明问题的消息，而不是把空 body 发出去。
pub(crate) fn request_body_bytes(input: &HttpRequestInput) -> Result<Option<Vec<u8>>, String> {
    if let Some(b64) = &input.body_base64 {
        return base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .map(Some)
            .map_err(|e| format!("the request body is not valid base64: {e}"));
    }
    Ok(input.body.as_ref().map(|s| s.as_bytes().to_vec()))
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
/// 读的是**原始字节**，不解码：响应可能是二进制（图片、压缩包），解码会破坏它。
/// 解码与「文本还是二进制」的判定由 [`split_response_payload`] 做。
async fn read_body_capped(
    mut res: reqwest::Response,
    max_bytes: u64,
) -> Result<Vec<u8>, reqwest::Error> {
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
            // read_body_capped 读够了：剩下的字节直接丢弃，连接随之关闭，服务端不会把剩下的内容继续推过来。
            break;
        }
    }

    Ok(buf)
}

// 线上那一份请求头。

/// 传输层在调用方的请求头之外补上的那几条。
///
/// 它们由 HTTP/1.1 与传输实现决定，不是调用方能关掉的，故线上一定有：
///   - `accept`：传输库的默认头，值是 `*/*`。调用方自己写过 `accept` 时不补。
///   - `host`：目标主机名。端口是该 scheme 的默认端口时不写，默认端口之外的才带上。
///   - `content-length`：请求体的字节数。没有请求体时没有这一条。
///   - `accept-encoding`：传输层支持的压缩算法。
///
/// 值是**线路上的原样**（`accept-encoding` 的逗号后没有空格），不是人读的写法：
/// 这一份要拿去与原始字节逐条比对。
/// `http_executor_test.rs` 里的 `the_reported_request_headers_are_the_ones_on_the_wire`
/// 把它们与线路上的字节逐条比对，故这里写错一个值时那条测试会红。
const TRANSPORT_ACCEPT: &str = "*/*";
const TRANSPORT_ACCEPT_ENCODING: &str = "gzip,deflate,br";

/// 这一次真正发出去的请求头。
///
/// 调用方交下来的那些按原样保留（名字转小写，与线路上的写法一致），传输层补的几条
/// 追加在后面。调用方自己写过的同名头不补第二条：线路上发出去的是调用方那一份。
///
/// `body_len` 是**请求体的字节数**。它不能从 `input.body` 的字符串长度取：请求体也可能是
/// base64 形态的二进制，那时字符串长度是字节数的四分之三，而线路上写的是字节数。
pub(crate) fn effective_request_headers(
    input: &HttpRequestInput,
    body_len: Option<usize>,
) -> Vec<[String; 2]> {
    let mut out: Vec<[String; 2]> = input
        .headers
        .iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, v)| [k.to_ascii_lowercase(), v.clone()])
        .collect();

    // 这段先把已有的名字收成一份，后面要往 `out` 里 push，故判定不能借用 `out` 本身。
    let present: Vec<String> = out.iter().map(|e| e[0].clone()).collect();
    let has = |name: &str| present.iter().any(|k| k == name);

    if !has("accept") {
        out.push(["accept".to_string(), TRANSPORT_ACCEPT.to_string()]);
    }

    if !has("accept-encoding") {
        out.push([
            "accept-encoding".to_string(),
            TRANSPORT_ACCEPT_ENCODING.to_string(),
        ]);
    }

    if let Ok(url) = reqwest::Url::parse(&input.url) {
        if !has("host") {
            let host = url.host_str().unwrap_or_default().to_string();
            // `Url::port()` 在端口是该 scheme 的默认端口时给 `None`，正好对应
            // 「默认端口不写」这条规则。
            let host_header = match url.port() {
                Some(p) => format!("{host}:{p}"),
                None => host,
            };
            out.push(["host".to_string(), host_header]);
        }
    }

    if let Some(len) = body_len {
        if !has("content-length") {
            out.push(["content-length".to_string(), len.to_string()]);
        }
    }

    out
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

    // check_url 在建客户端或碰网络之前就拒绝。
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

    // 客户端构造分两步：先施加 HTTPS 设置（证书、跳过校验），再真正建出客户端。
    // 两步都可能失败，错误都归到 `network` 这一档——前端只按 `timeout` 与 `network` 分派，
    // 证书读不到与连不上都是「请求没发出去」。
    let client = match crate::tls::apply_to_builder(
        reqwest::Client::builder()
            .timeout(Duration::from_millis(input.timeout_ms))
            .redirect(redirect),
        &input.tls,
    ) {
        Ok(builder) => match builder.build() {
            Ok(c) => c,
            Err(e) => return HttpResult::err("network", format!("client build: {e}")),
        },
        Err(e) => return HttpResult::err("network", e.to_string()),
    };

    let mut req = client.request(parse_method(&input.method), &input.url);
    for (k, v) in &input.headers {
        if !k.is_empty() {
            req = req.header(k, v);
        }
    }

    // 请求体的字节在**建客户端之前**解出来：`content-length` 要的是字节数，而 base64
    // 形态的字符串长度不是字节数。解不开时报错而不是发一个空 body——那会让服务端收到
    // 一个内容被静默丢掉的请求。
    let body_bytes = match request_body_bytes(input) {
        Ok(b) => b,
        Err(msg) => return HttpResult::err("network", msg),
    };

    if let Some(ref bytes) = body_bytes {
        req = req.body(bytes.clone());
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

    // 读取时的上限：缓冲区上限与响应大小无关，超出的部分直接丢弃。
    let raw_body = match read_body_capped(res, max_bytes).await {
        Ok(t) => t,
        Err(e) => {
            let kind = if e.is_timeout() { "timeout" } else { "network" };
            return HttpResult::err(kind, format!("read body: {e}"));
        }
    };

    let elapsed = start.elapsed().as_millis();
    let payload = split_response_payload(raw_body, max_bytes, &content_type);

    HttpResult::ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        duration_ms: elapsed,
        size_bytes: payload.bytes,
        headers: header_pairs,
        request_headers: effective_request_headers(input, body_bytes.as_ref().map(|b| b.len())),
        body: payload.text,
        body_base64: payload.base64,
        content_type,
        method: input.method.clone(),
        url: input.url.clone(),
        truncated: payload.truncated,
    })
}

// 测试——内部辅助函数 + 结果封装。

#[cfg(test)]
#[path = "http_executor_test.rs"]
mod tests;
