//! 响应解析 + diff + JSONPath 抽取。纯变换，无 I/O。
//!
//!   * [`parse`] —— 把 `(body, content_type, status, headers)` 折成
//!     [`ParsedResponse`]（解析好的 JSON、字节数、状态文本）。MCP 的
//!     `api_parse_response` 与渲染进程侧的预览都用它，省得在浏览器里再 parse 一遍。
//!   * [`diff`] —— 在 JSON 结构层比较两个 [`ApiResponse`]，给出新增 / 删除 / 变更的
//!     JSONPath 字符串，变更项另附 before/after 字面值。
//!   * [`extract_jsonpath`] —— 对响应体跑一条 JSONPath 表达式，返回首个匹配（或
//!     `None`）。
//!
//! diff 是手写的递归遍历，不引 `json-patch`。渲染进程已经自己画视觉 diff 了；这一层
//! 只产出一份任何 UI 都能渲染的结构摘要（新增 / 删除 / 变更三个列表）。

use serde_json::Value;
use serde_json_path::JsonPathExt;

use serde::Serialize;
use simple_post_core::domain::ApiResponse;

// -------------------------------------------------------------------------
// 公共类型
// -------------------------------------------------------------------------

/// 响应的解析视图：在原始输入之上补几个派生字段，下游不必再解析一遍。
#[derive(Debug, Clone)]
pub struct ParsedResponse {
    /// JSON 内容的解析结果，否则 `None`。
    pub body_json: Option<Value>,
    /// **截断后** body 的字节长度，与线上形状一致。
    pub size_bytes: u64,
    /// 状态码的标准 reason phrase（如 200 → `"OK"`）；非标准码时为空。
    pub status_text: String,
}

/// 某个 JSONPath 上的一组 before → after。`from` / `to` 直接取自该路径处的 JSON 树；
/// 基本类型变化（字符串 / 数字 / 布尔）算作普通「变更」项，而不是删除 + 新增。
#[derive(Debug, Clone, Serialize)]
pub struct ChangedPath {
    pub path: String,
    pub from: Value,
    pub to: Value,
}

/// 两个 [`ApiResponse`] 的 diff 摘要。空集合表示「该层无变化」。`status_text` 与
/// 请求头 diff 不在范围内 —— 在意的调用方自己比那两个字段。
#[derive(Debug, Clone, Default)]
pub struct ResponseDiff {
    /// 只在 `b` 里存在的 JSONPath。
    pub added_paths: Vec<String>,
    /// 只在 `a` 里存在的 JSONPath。
    pub removed_paths: Vec<String>,
    /// `a` 与 `b` 之间叶子值不同的 JSONPath。
    pub changed_paths: Vec<ChangedPath>,
}

// -------------------------------------------------------------------------
// parse
// -------------------------------------------------------------------------

/// 解析原始 HTTP 响应，给出渲染进程 / MCP 工具想要的结构化视图。
///
///   * `body_json` 只有在 `content_type` 含 `application/json` **且** body 解析得通时
///     才是 `Some`；不是 JSON（或 JSON 有语法错）一律 `None` —— 从不报错。
///   * `size_bytes` 是传入 body 的字节长度；要限长由调用方先截断。
///   * `status_text` 是规范 reason phrase（如 `"OK"`、`"Not Found"`），非标准码为 `""`。
pub fn parse(
    body: &str,
    content_type: &str,
    status: u16,
    _headers: &[(String, String)],
) -> ParsedResponse {
    let body_json = if content_type
        .to_ascii_lowercase()
        .contains("application/json")
    {
        serde_json::from_str(body).ok()
    } else {
        None
    };
    let size_bytes = body.len() as u64;
    let status_text = status_reason(status).to_string();
    ParsedResponse {
        body_json,
        size_bytes,
        status_text,
    }
}

/// 状态码对应的规范 HTTP reason phrase。等同 `http::StatusCode::canonical_reason()`，
/// 但不值得为此把 `http` crate 拖进来。集合外的码返回 `""`（渲染进程本就把空值当作
/// 「非标准」，并回退到自己的措辞）。
pub fn status_reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "",
    }
}

// -------------------------------------------------------------------------
// diff —— 手写递归 JSON 遍历
// -------------------------------------------------------------------------

/// 在 JSON 层比较两个 [`ApiResponse`]。
///
///   * 两侧都能解析成 JSON → 跑递归比较。
///   * 两侧都解析不了 → 当字符串比，不同就把 `$` 报成一条「变更」。
///   * 只有一侧能解析 → 解析成功的那侧全部路径算「删除」（或「新增」，看它在哪侧），
///     `$` 算「变更」。
///
/// 三个列表都排序（新增/删除按字典序，变更按 JSONPath 字符串），diff 因此跨次运行
/// 稳定，单测也好断言。
pub fn diff(a: &ApiResponse, b: &ApiResponse) -> ResponseDiff {
    let a_json = a.body_json.clone();
    let b_json = b.body_json.clone();
    match (a_json, b_json) {
        (Some(av), Some(bv)) => diff_values(&av, &bv),
        (None, None) => {
            let mut d = ResponseDiff::default();
            if a.body != b.body {
                d.changed_paths.push(ChangedPath {
                    path: "$".to_string(),
                    from: Value::String(a.body.clone()),
                    to: Value::String(b.body.clone()),
                });
                d.changed_paths.sort_by(|x, y| x.path.cmp(&y.path));
            }
            d
        }
        (Some(av), None) => {
            // `av` 的全部路径算「删除」，body 本身算「变更」。
            let mut d = ResponseDiff::default();
            let mut paths: Vec<String> = Vec::new();
            collect_paths(&av, "$", &mut paths);
            d.removed_paths = paths;
            d.changed_paths.push(ChangedPath {
                path: "$".to_string(),
                from: av,
                to: Value::String(b.body.clone()),
            });
            sort_diff(&mut d);
            d
        }
        (None, Some(bv)) => {
            let mut d = ResponseDiff::default();
            let mut paths: Vec<String> = Vec::new();
            collect_paths(&bv, "$", &mut paths);
            d.added_paths = paths;
            d.changed_paths.push(ChangedPath {
                path: "$".to_string(),
                from: Value::String(a.body.clone()),
                to: bv,
            });
            sort_diff(&mut d);
            d
        }
    }
}

fn sort_diff(d: &mut ResponseDiff) {
    d.added_paths.sort();
    d.removed_paths.sort();
    d.changed_paths.sort_by(|x, y| x.path.cmp(&y.path));
}

/// 两棵 `serde_json::Value` 的递归 diff。路径写法与我们关心的情形下的 JSONPath 一致：
/// 根是 `$`，对象成员是 `$.key`，数组元素是 `$[i]`。
fn diff_values(a: &Value, b: &Value) -> ResponseDiff {
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut changed: Vec<ChangedPath> = Vec::new();
    walk(a, b, "$", &mut added, &mut removed, &mut changed);

    let mut d = ResponseDiff {
        added_paths: added,
        removed_paths: removed,
        changed_paths: changed,
    };
    sort_diff(&mut d);
    d
}

fn walk(
    a: &Value,
    b: &Value,
    path: &str,
    added: &mut Vec<String>,
    removed: &mut Vec<String>,
    changed: &mut Vec<ChangedPath>,
) {
    // 同类型且同值 → 叶子相等。
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(am), Value::Object(bm)) => {
            // 只在 `b` 里的键 → 新增。
            for (k, v) in bm {
                let child = format!("{path}.{k}");
                match am.get(k) {
                    Some(av) => walk(av, v, &child, added, removed, changed),
                    None => {
                        // 整棵子树新增；收集每个叶子路径，渲染进程才能逐个高亮。
                        let mut leaves: Vec<String> = Vec::new();
                        collect_paths(v, &child, &mut leaves);
                        added.extend(leaves);
                    }
                }
            }
            // 只在 `a` 里的键 → 删除。
            for (k, v) in am {
                if !bm.contains_key(k) {
                    let child = format!("{path}.{k}");
                    let mut leaves: Vec<String> = Vec::new();
                    collect_paths(v, &child, &mut leaves);
                    removed.extend(leaves);
                }
            }
        }
        (Value::Array(av), Value::Array(bv)) => {
            let max = av.len().max(bv.len());
            for i in 0..max {
                let child = format!("{path}[{i}]");
                match (av.get(i), bv.get(i)) {
                    (Some(x), Some(y)) => walk(x, y, &child, added, removed, changed),
                    (Some(x), None) => {
                        // 尾部被截掉。
                        let mut leaves: Vec<String> = Vec::new();
                        collect_paths(x, &child, &mut leaves);
                        removed.extend(leaves);
                    }
                    (None, Some(y)) => {
                        let mut leaves: Vec<String> = Vec::new();
                        collect_paths(y, &child, &mut leaves);
                        added.extend(leaves);
                    }
                    (None, None) => break,
                }
            }
        }
        // 类型变了（如字符串 ↔ 数字）或同为基本类型但值不同 → 叶子变更。
        _ => {
            changed.push(ChangedPath {
                path: path.to_string(),
                from: a.clone(),
                to: b.clone(),
            });
        }
    }
}

/// 收集 `v` 下每个 JSONPath 叶子，各自加上 `prefix`。把「整棵子树新增/删除」摊成
/// 渲染进程要的扁平列表。
fn collect_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, vv) in map {
                let child = format!("{prefix}.{k}");
                collect_paths(vv, &child, out);
            }
        }
        Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let child = format!("{prefix}[{i}]");
                collect_paths(vv, &child, out);
            }
        }
        _ => out.push(prefix.to_string()),
    }
}

// -------------------------------------------------------------------------
// extract_jsonpath
// -------------------------------------------------------------------------

/// 对 `body` 跑一条 JSONPath 表达式，返回首个匹配，或 `None`。
///
/// 以下情形返回 `None` 而不是 `Err`：body 不是 JSON；body 解析失败；表达式解析失败；
/// 零匹配。调用方要的是「首个匹配」或「没有」，不是一个带类型的错误。
pub fn extract_jsonpath(body: &str, content_type: &str, path: &str) -> Option<Value> {
    if !content_type
        .to_ascii_lowercase()
        .contains("application/json")
    {
        return None;
    }
    // 防的是 `serde_json_path` 0.1 那个宽松解析器：它会把 "$$ bad path" / "@@nope"
    // 也变成一条能匹配到根的查询。RFC 9535 要求 JSONPath 以 `$` 开头；路径里出现裸
    // 空白也一并拒绝，免得把用户笔误静默解析成「全都匹配」。
    let trimmed = path.trim();
    if !trimmed.starts_with('$') {
        return None;
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let value: Value = serde_json::from_str(body).ok()?;
    let query = value.json_path(trimmed).ok()?;
    // `as_node` 只有恰好一个匹配时才返回 Some；零个或多个都落到 `as_node_list`，
    // 好让调用方仍拿到东西，而不是一个静默的空结果。
    if let Some(node) = query.as_node() {
        return Some(node.clone());
    }
    let list = query.as_node_list()?;
    list.first().map(|n| (*n).clone())
}

// -------------------------------------------------------------------------
// 测试
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "response_parse_test.rs"]
mod tests;
