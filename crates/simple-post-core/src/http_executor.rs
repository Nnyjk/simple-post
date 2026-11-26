//! HTTP 执行层。
//!
//! 纯 I/O——不含业务逻辑。调用方（Tauri command、MCP 守护进程）构造 [`HttpRequestInput`]、
//! 交给 [`execute`]、拿回一个 [`HttpResult`]，其形状与 React 渲染层已经理解的线契约一致
//! （见 `src/lib/http.ts::ApiResponse`）。
//!
//! 本模块是后端里唯一与 `reqwest` 说话的地方。把线类型与真正的网络调用从 `lib.rs` 里拆出来，
//! 使这一层能被 MCP 守护进程复用，而不必拖进 Tauri 的 `#[command]` 宏管线（以及随之而来的
//! 宏命名空间冲突——见 `lib.rs` 里 `http_request` 的 NOTE）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::Serialize;

use crate::domain::TlsSettings;

// 线类型——镜像 TS 侧的 `ApiResponse` / `HttpError`。

/// 成功的 HTTP 响应。字段名与 `ApiResponse` 一致，故 React 侧可以复用它现有的响应视图代码。
///
/// `rename_all = "camelCase"` 是必需的属性：没有它 serde 会发出 `status_text` /
/// `duration_ms` / `size_bytes` / `content_type`，而 `src/lib/http.ts` 读的是
/// `statusText` / `durationMs` / `sizeBytes` / `contentType`——每一个都读回 `undefined`。
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
    pub request_headers: Vec<[String; 2]>,
    pub body: String,
    /// 响应体的**二进制形态**：原始字节的 base64。
    ///
    /// 正文不是文本时才有它（图片、压缩包、PDF…）。判据先问 Content-Type：主类型是
    /// `text/*`、或以 `json` / `xml` 等文本类型结尾时算文本，哪怕字节不是合法 UTF-8；
    /// 其余的才按字节判。此时 `body` 是**空串**，原始字节只能从这条通道取——宽松解码会
    /// 把二进制弄坏，故传输层不给出那份残影。文本正文（含宽松解码后的那份）不给这条通道。
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
/// `rename_all` 当前是空操作（两个字段都是单词），但它必须与 [`HttpResponse`] 用同一个属性。
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

// 传输进度。

/// 一次传输的进度快照。
///
/// 上传阶段只填前两个字段：`sent_bytes` 是已经交给传输层的请求体字节数，`total_bytes` 是请求体
/// 总字节数。服务器把接收进度回传时后两个才有值（见 [`read_body_capped_with_progress`] 认的
/// 那几种标准化响应），渲染层据此在同一根轨道上叠加第二层。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpTransferProgress {
    pub sent_bytes: u64,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_received_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_total_bytes: Option<u64>,
}

/// 进度回调。可能在异步上传与响应读取两处被调用，故要求 `Send + Sync`。
pub type ProgressSink = Box<dyn Fn(HttpTransferProgress) + Send + Sync>;

/// 上传分块的字节数。32 KiB 是进度粒度与系统调用次数之间的折中。
const UPLOAD_CHUNK_BYTES: usize = 32 * 1024;

/// 两帧进度之间的最小间隔。本地回环上一次上传可能只花几毫秒，不节流会把成千上万帧
/// 塞进 IPC；首帧与末帧不受它限制。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(40);

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
    /// 有它时 `body` 缺席，线上发出去的是解码后的字节。multipart 带文件分片时整个请求体是字节，
    /// 故走这条通道；纯文本的请求体仍走 `body`。
    ///
    /// 两者都有时以本字段为准——字节那份才是能被还原成文件的那份。
    pub body_base64: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
    /// HTTPS 出站设置。**已经是合成过的那一份**：全局默认与端点覆盖的合成由
    /// [`crate::tls::resolve`] 完成，不在这里做，故执行层没有第二套合并规则。
    pub tls: TlsSettings,
}

// 内部实现——小到把它们内联为 `pub(crate)` 能保持公开接口干净。lib.rs 里的 Tauri command 与
// MCP 守护进程都调 `execute`；别的都不需要碰这些。

/// 把文本形式的 HTTP 方法解析成 `reqwest::Method`，未知动词回退 GET（前端已经在已知列表上
/// 把过一道）。
pub(crate) fn parse_method(method: &str) -> reqwest::Method {
    // 动词表只有一份：`domain::parse_http_method` 是 MCP 工具与 `HttpMethod` 反序列化
    // 共用的那个解析器。
    crate::domain::parse_http_method(method)
        .ok()
        .and_then(|m| reqwest::Method::from_bytes(m.as_str().as_bytes()).ok())
        .unwrap_or(reqwest::Method::GET)
}

/// 把 body 字符串裁到至多 `max_bytes` 字节（落在 UTF-8 安全的字符边界上）。被裁时加上一个标记。
///
/// 这条契约同时是**流式读取路径**的裁剪判据：流式路径把「读到的原始字节」解码成字符串后交给
/// 本函数，故裁剪点与标记文案只有这一个产生点。
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
/// 文本形态是宽松解码后的字符串；二进制形态是原始字节的 base64。**判据先问 Content-Type，
/// 再问字节**：Content-Type 说它是文本就是文本（哪怕读到的字节非法 UTF-8）；没说它是文本，
/// 才轮到字节说话。
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
/// 文本形态走 [`truncate_body`]：宽松解码后再按字符边界裁。**先裁字节再解码会给出另一个答案**
/// ——落在裁点上的半个多字节字符在两种顺序下解出不同的东西。二进制形态按字节裁。
///
/// **形态的判据是「内容是不是文本」，不是「读到的这段字节是不是合法 UTF-8」**：读到的字节是按
/// 上限裁过的，裁点落在某个多字节字符中间时，一个纯文本响应读回来也是非法 UTF-8。
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
/// 主类型为 `text`，或子类型以 `json` / `xml` 结尾，或属于特定文本类型
/// （包括 `javascript`、`x-www-form-urlencoded` 与 `graphql`）。
/// 二进制类型（如 `application/octet-stream`、`image/*`、`application/pdf`、`application/zip`）
/// 均不算文本：宽松解码会破坏原始字节。
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

/// 流式读取响应体：边读边计数，累计到上限就停，剩下的字节直接丢弃。缓冲区的大小只与上限有关，
/// 与响应大小无关。`max_bytes == 0` 表示不设上限，整个 body 都要读进来。
///
/// 读的是**原始字节**，不解码：响应可能是二进制（图片、压缩包），解码会破坏它。解码与
/// 「文本还是二进制」的判定由 [`split_response_payload`] 做。
///
/// 有进度回调时同一趟还认服务器回传的接收进度，认两种标准化形态，且都不改写响应体本身
/// （原始字节照旧完整保留到 `max_bytes`）：
///
/// * **流式行事件**：`Content-Type` 是 `text/event-stream` / `application/x-ndjson` /
///   `application/ndjson` / `application/jsonl`，或响应头带 `X-Upload-Progress` /
///   `X-Receiving-Progress`。SSE 的 `data:` 行与 NDJSON 行里含 `receivedBytes`
///   （兼容 `received_bytes` / `received`）、可选 `totalBytes`（兼容 `total_bytes` /
///   `total`）的 JSON 对象即算一帧；带 `type` 时必须是进度类取值。
/// * **响应头单值**：`X-Received-Bytes` 或 `Upload-Offset`（可选 `X-Upload-Total-Bytes`）
///   立即上报一帧。
///
/// 普通 API 响应（如 `application/json` 且无上述头）不触发扫描，前端保持单色上传进度条。
async fn read_body_capped_with_progress(
    mut res: reqwest::Response,
    max_bytes: u64,
    content_type: &str,
    sent_bytes: u64,
    total_bytes: u64,
    progress: Option<&Arc<ProgressSink>>,
) -> Result<Vec<u8>, reqwest::Error> {
    let cap = if max_bytes == 0 {
        u64::MAX
    } else {
        max_bytes.saturating_add(BOUNDARY_LOOKAHEAD_BYTES)
    };

    let mut scanner = progress.and_then(|sink| {
        let headers = res.headers();
        let has_line_progress = matches!(
            content_type
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "text/event-stream"
                | "application/x-ndjson"
                | "application/ndjson"
                | "application/jsonl"
        ) || headers.contains_key("x-upload-progress")
            || headers.contains_key("x-receiving-progress");
        let header_progress = header_u64(headers, "x-received-bytes")
            .or_else(|| header_u64(headers, "upload-offset"));
        if let Some(received) = header_progress {
            sink(HttpTransferProgress {
                sent_bytes,
                total_bytes,
                server_received_bytes: Some(received),
                server_total_bytes: header_u64(headers, "x-upload-total-bytes"),
            });
        }
        if !has_line_progress && header_progress.is_none() {
            return None;
        }
        Some(ReceiveProgressScanner {
            sink: sink.clone(),
            sent_bytes,
            total_bytes,
            buffer: String::new(),
        })
    });

    let mut buf: Vec<u8> = Vec::new();
    let mut len: u64 = 0;
    while let Some(chunk) = res.chunk().await? {
        let take = std::cmp::min(chunk.len() as u64, cap - len) as usize;
        buf.extend_from_slice(&chunk[..take]);
        len += take as u64;
        if let Some(scanner) = scanner.as_mut() {
            scanner.feed(&chunk[..take]);
        }
        if len >= cap {
            // read_body_capped 读够了：剩下的字节直接丢弃，连接随之关闭，服务端不会把剩下的内容继续推过来。
            break;
        }
    }
    if let Some(scanner) = scanner.as_mut() {
        scanner.finish();
    }

    Ok(buf)
}

/// 响应头里的一个非负整数。不是整数就当它不存在。
fn header_u64(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// 流式响应体里的标准化接收进度扫描器。**只读**：原始字节由调用方另存一份。
///
/// 行切在块中间是常态，故不足一行的尾巴留在 `buffer` 里等下一块。进度事件本身很小，
/// 超过 [`MAX_PROGRESS_LINE_BYTES`] 还没换行的那行不是进度事件，直接丢掉，避免把一整段
/// 大 body 攒在内存里。
struct ReceiveProgressScanner {
    sink: Arc<ProgressSink>,
    sent_bytes: u64,
    total_bytes: u64,
    buffer: String,
}

/// 一行进度事件的上限。超过它仍没有换行的内容按「不是进度事件」处理。
const MAX_PROGRESS_LINE_BYTES: usize = 64 * 1024;

impl ReceiveProgressScanner {
    fn feed(&mut self, bytes: &[u8]) {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer.drain(..=pos);
            self.on_line(&line);
        }
        if self.buffer.len() > MAX_PROGRESS_LINE_BYTES {
            self.buffer.clear();
        }
    }

    /// 流尾没有换行的那一行也要看：服务端可能把最后一帧写在 EOF 上而不补换行。
    fn finish(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.buffer);
        self.on_line(&line);
    }

    fn on_line(&self, line: &str) {
        let line = line.trim();
        // SSE 的字段前缀。其它字段（`event:` / `id:`）在这一层没有语义。
        let payload = line.strip_prefix("data:").unwrap_or(line).trim();
        if payload.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            return;
        };
        let Some(object) = value.as_object() else {
            return;
        };
        if let Some(kind) = object.get("type").and_then(|v| v.as_str()) {
            if !matches!(
                kind,
                "progress"
                    | "upload_progress"
                    | "upload-progress"
                    | "receiving_progress"
                    | "receiving-progress"
                    | "receive_progress"
                    | "receive-progress"
            ) {
                return;
            }
        }
        let Some(received) = pick_u64(object, &["receivedBytes", "received_bytes", "received"])
        else {
            return;
        };
        let total = pick_u64(object, &["totalBytes", "total_bytes", "total"]);
        (self.sink)(HttpTransferProgress {
            sent_bytes: self.sent_bytes,
            total_bytes: self.total_bytes,
            server_received_bytes: Some(received),
            server_total_bytes: total,
        });
    }
}

/// 按候选键顺序取第一个非负整数。
fn pick_u64(object: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(|v| v.as_u64()))
}

/// 把请求体切成 32 KiB 的块交给 `reqwest`，并沿途报告发送进度。
///
/// 首帧在切第一块时发出，末帧在切最后一块时发出；中间帧受 [`PROGRESS_INTERVAL`] 节流。
fn upload_stream(
    bytes: Arc<Vec<u8>>,
    sink: Arc<ProgressSink>,
    total: u64,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> {
    let mut offset = 0usize;
    let mut last_emit: Option<Instant> = None;
    futures_util::stream::poll_fn(move |_| {
        if offset >= bytes.len() {
            return std::task::Poll::Ready(None);
        }
        let end = (offset + UPLOAD_CHUNK_BYTES).min(bytes.len());
        let chunk = bytes[offset..end].to_vec();
        offset = end;
        let now = Instant::now();
        let due = last_emit.is_none_or(|at| now.duration_since(at) >= PROGRESS_INTERVAL);
        if due || offset == bytes.len() {
            last_emit = Some(now);
            sink(HttpTransferProgress {
                sent_bytes: offset as u64,
                total_bytes: total,
                server_received_bytes: None,
                server_total_bytes: None,
            });
        }
        std::task::Poll::Ready(Some(Ok(chunk)))
    })
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
/// 调用方交下来的那些按原样保留（名字转小写），传输层补的几条追加在后面。调用方自己写过的同名头
/// 不补第二条：线路上发出去的是调用方那一份。
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

/// 判定为绝不该通过服务端代理抵达的 IP：loopback、RFC1918、link-local（含 169.254.169.254 处的
/// 云元数据）、运营商级 NAT、以及 IPv6 unique-local。
/// 此处**不**解析主机名：解析会在检查与拨号之间开出一个 DNS rebinding 窗口。
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

/// 建出这一次请求的客户端构造器：重定向策略（逐跳重跑 `check_url`）与 HTTPS 设置。
///
/// 不设总超时：`execute` 自己加 `.timeout(...)`（整次请求的预算），SSE 流式路径不要总超时
/// ——它的 `timeout_ms` 作用于建连与块间空闲（见 `crate::stream`）。
pub(crate) fn base_client_builder(
    input: &HttpRequestInput,
    policy: OutboundPolicy,
) -> Result<reqwest::ClientBuilder, String> {
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

    // 先施加 HTTPS 设置（证书、跳过校验），再交给调用方加超时并 build。
    crate::tls::apply_to_builder(reqwest::Client::builder().redirect(redirect), &input.tls)
        .map_err(|e| e.to_string())
}

/// 经 `reqwest` 发出真实 HTTP 请求，绕开浏览器 CORS。纯异步 I/O；无共享状态。
/// 前端调 `lib.rs` 里的 `http_request` Tauri command，由它转发到这里。MCP 守护进程
/// （`crate::mcp`）经它自己的 `execute_http` 转发层路由请求。
pub async fn execute(input: &HttpRequestInput) -> HttpResult {
    execute_with_progress(input, None).await
}

/// 带进度的 [`execute`]。`progress` 缺席时与 [`execute`] 逐字节同路。
pub async fn execute_with_progress(
    input: &HttpRequestInput,
    progress: Option<ProgressSink>,
) -> HttpResult {
    execute_with_policy_and_progress(input, OutboundPolicy::from_env(), progress).await
}

/// 带显式策略的 [`execute`]。测试直接用它；由环境驱动的 [`execute`] 是生产入口。
pub async fn execute_with_policy(input: &HttpRequestInput, policy: OutboundPolicy) -> HttpResult {
    execute_with_policy_and_progress(input, policy, None).await
}

/// 带显式策略与进度的 [`execute`]。
///
/// 有请求体且给了 `progress` 时，请求体经 [`upload_stream`] 分块写出：每块报告一次已发送字节，
/// 并在请求头上显式补齐 `content-length`（流式 body 没有长度提示，缺了它服务端会把请求读成
/// chunked）。没有请求体或没有回调时仍走一次性的 `bytes` body，线形状与从前一致。
pub async fn execute_with_policy_and_progress(
    input: &HttpRequestInput,
    policy: OutboundPolicy,
    progress: Option<ProgressSink>,
) -> HttpResult {
    let start = Instant::now();
    let progress = progress.map(Arc::new);

    // check_url 在建客户端或碰网络之前就拒绝。
    if let UrlVerdict::Refused(reason) = check_url(&input.url, policy) {
        return HttpResult::err("network", reason);
    }

    // 每次调用一个 Client，好让 API 保持请求作用域。重定向与 HTTPS 设置的构造抽在
    // `base_client_builder` 里，SSE 流式路径用的是同一份，故两条路不会分叉。
    let client = match base_client_builder(input, policy) {
        Ok(builder) => match builder
            .timeout(Duration::from_millis(input.timeout_ms))
            .build()
        {
            Ok(c) => c,
            Err(e) => return HttpResult::err("network", format!("client build: {e}")),
        },
        Err(e) => return HttpResult::err("network", e),
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
    let body_len = body_bytes.as_ref().map(|b| b.len());
    let body_total = body_len.unwrap_or(0) as u64;

    match (body_bytes, progress.as_ref()) {
        (Some(bytes), Some(sink)) if !bytes.is_empty() => {
            // 流式 body 没有 `size_hint`，`reqwest` 不会自己补 `content-length`；调用方自己
            // 写过同名头时不再追加第二条（与 `effective_request_headers` 同一条规则）。
            let caller_set_length = input
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
            if !caller_set_length {
                req = req.header(reqwest::header::CONTENT_LENGTH, body_total.to_string());
            }
            sink(HttpTransferProgress {
                sent_bytes: 0,
                total_bytes: body_total,
                server_received_bytes: None,
                server_total_bytes: None,
            });
            req = req.body(reqwest::Body::wrap_stream(upload_stream(
                Arc::new(bytes),
                sink.clone(),
                body_total,
            )));
        }
        (Some(bytes), _) => req = req.body(bytes),
        (None, _) => {}
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
    let raw_body = match read_body_capped_with_progress(
        res,
        max_bytes,
        &content_type,
        body_total,
        body_total,
        progress.as_ref(),
    )
    .await
    {
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
        request_headers: effective_request_headers(input, body_len),
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
