//! Response parsing + diff + JSONPath extraction.
//!
//! Pure transforms — no I/O. Three public functions:
//!
//!   * [`parse`] — turn a raw `(body, content_type, status, headers)`
//!     tuple into a [`ParsedResponse`] (parsed JSON, byte size, status
//!     text). Used by the MCP `api_parse_response` tool and by the
//!     renderer-side preview that wants `bodyJson` without re-parsing
//!     in the browser.
//!   * [`diff`] — compare two [`ApiResponse`] values and report what
//!     changed at the JSON-structure level. JSONPath strings for
//!     added / removed / changed paths, plus the literal
//!     before/after values for changed paths.
//!   * [`extract_jsonpath`] — run a JSONPath expression against a
//!     response body, return the first match (or `None`).
//!
//! Diff implementation is a hand-rolled recursive walk — no
//! `json-patch` dep. The renderer already does its own visual diff;
//! this layer just produces a structural summary that any UI can
//! render (added / removed / changed lists).

use serde_json::Value;
use serde_json_path::JsonPathExt;

use crate::domain::ApiResponse;
use serde::Serialize;

// -------------------------------------------------------------------------
// Public types
// -------------------------------------------------------------------------

/// Parsed view of a response. Adds a few derived fields on top of
/// the raw input so downstream consumers don't have to re-parse.
#[derive(Debug, Clone)]
pub struct ParsedResponse {
    /// Parsed body for JSON content (else `None`).
    pub body_json: Option<Value>,
    /// Byte length of the *truncated* body, matches the wire shape.
    pub size_bytes: u64,
    /// Standard HTTP status reason phrase for the status code (e.g.
    /// `"OK"` for 200). Empty when the code is non-standard.
    pub status_text: String,
}

/// One `before → after` pair at a single JSONPath. The "from" /
/// "to" values are taken verbatim from the JSON tree at that path;
/// a primitive type change (string vs number vs bool) shows up as
/// a normal "changed" entry, not as remove+add.
#[derive(Debug, Clone, Serialize)]
pub struct ChangedPath {
    pub path: String,
    pub from: Value,
    pub to: Value,
}

/// Diff summary between two [`ApiResponse`] values. Empty collections
/// mean "no change at that level". `status_text` / header diffs are
/// out of scope here — callers that care compare those fields
/// directly.
#[derive(Debug, Clone, Default)]
pub struct ResponseDiff {
    /// JSONPath expressions that exist only in `b`.
    pub added_paths: Vec<String>,
    /// JSONPath expressions that exist only in `a`.
    pub removed_paths: Vec<String>,
    /// JSONPath expressions whose leaf value differs between `a` and `b`.
    pub changed_paths: Vec<ChangedPath>,
}

// -------------------------------------------------------------------------
// parse
// -------------------------------------------------------------------------

/// Parse a raw HTTP response. Returns the structured view the
/// renderer / MCP tool consumer wants.
///
/// Behavior:
///   * `body_json` is `Some` only when `content_type` contains
///     `application/json` AND the body parses cleanly. A body
///     that isn't JSON (or JSON with a syntax error) yields
///     `body_json = None` — never an error.
///   * `size_bytes` is the byte length of the input body as given
///     (caller is responsible for truncating first if there's a
///     size cap).
///   * `status_text` is the canonical reason phrase (e.g. `"OK"`,
///     `"Not Found"`) or `""` when the status is non-standard.
pub fn parse(
    body: &str,
    content_type: &str,
    status: u16,
    _headers: &[(String, String)],
) -> ParsedResponse {
    let body_json = if content_type.to_ascii_lowercase().contains("application/json") {
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

/// Canonical HTTP reason phrase for a status code. Mirrors
/// `http::StatusCode::canonical_reason()` without dragging the
/// `http` crate in for this one use. Returns `""` for codes that
/// aren't in the standard set (the renderer already treats empty
/// as "non-standard" and falls back to its own phrasing).
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
// diff — hand-rolled recursive JSON walker.
// -------------------------------------------------------------------------

/// Compare two [`ApiResponse`] values at the JSON level.
///
/// Rules:
///   * If both bodies parse as JSON, run the recursive comparison.
///   * If neither parses, compare bodies as strings — if they
///     differ, report `$` as a single "changed" entry.
///   * If one parses and the other doesn't, the parsed side wins:
///     all its paths become "removed" (or "added", depending on
///     which side has them), and `$` becomes "changed".
///
/// All three lists are sorted (added/removed lexicographically;
/// changed by the JSONPath string) so the diff is stable across
/// runs and easy to assert on in the test suite.
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
            // All of `av`'s paths are "removed"; body itself is "changed".
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

/// Recursive diff of two `serde_json::Value` trees. The path
/// notation matches JSONPath for the cases we care about:
///   `$` for the root,
///   `$.key` for object members,
///   `$[i]` for array elements.
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
    // Same JSON type AND same value → leaf equality.
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(am), Value::Object(bm)) => {
            // Keys only in `b` → added.
            for (k, v) in bm {
                let child = format!("{path}.{k}");
                match am.get(k) {
                    Some(av) => walk(av, v, &child, added, removed, changed),
                    None => {
                        // Whole subtree added; collect every leaf path
                        // so the renderer can highlight each one.
                        let mut leaves: Vec<String> = Vec::new();
                        collect_paths(v, &child, &mut leaves);
                        added.extend(leaves);
                    }
                }
            }
            // Keys only in `a` → removed.
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
                        // Tail removed.
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
        // Type changed (e.g. string ↔ number) or both primitives
        // with different values → leaf change.
        _ => {
            changed.push(ChangedPath {
                path: path.to_string(),
                from: a.clone(),
                to: b.clone(),
            });
        }
    }
}

/// Collect every JSONPath leaf under `v`, prefixing each with
/// `prefix`. Used to expand "subtree added/removed" into the
/// flat list the renderer wants.
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

/// Run a JSONPath expression against `body` and return the first
/// match, or `None`.
///
/// Returns `None` (not `Err`) when:
///   * the body isn't JSON,
///   * the body fails to parse,
///   * the JSONPath expression fails to parse,
///   * the query yields zero matches.
///
/// This deliberately mirrors the lenient semantics of the existing
/// MCP `extract_jsonpath_stub` — callers want "first match" or
/// "nothing", not a typed error.
pub fn extract_jsonpath(
    body: &str,
    content_type: &str,
    path: &str,
) -> Option<Value> {
    if !content_type.to_ascii_lowercase().contains("application/json") {
        return None;
    }
    // Guard against `serde_json_path` 0.1's lenient parser, which
    // happily turns "$$ bad path" / "@@nope" into a query that matches
    // the root. Per RFC 9535 a JSONPath must start with `$`; we also
    // reject bare whitespace inside the path so we don't silently
    // resolve user typos to "everything".
    let trimmed = path.trim();
    if !trimmed.starts_with('$') {
        return None;
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let value: Value = serde_json::from_str(body).ok()?;
    let query = value.json_path(trimmed).ok()?;
    // `as_node` returns Some only when there's exactly one match.
    // For zero or many matches we fall through to `as_node_list` so
    // callers still get something rather than a silent empty result.
    if let Some(node) = query.as_node() {
        return Some(node.clone());
    }
    let list = query.as_node_list()?;
    list.first().map(|n| (*n).clone())
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn api(status: u16, body: &str) -> ApiResponse {
        let body_json = serde_json::from_str::<Value>(body).ok();
        ApiResponse {
            status,
            status_text: status_reason(status).to_string(),
            duration_ms: 0,
            size_bytes: body.len() as u64,
            headers: vec![],
            body: body.to_string(),
            body_json,
            content_type: "application/json".into(),
            error: None,
            method: None,
            url: None,
        }
    }

    fn api_text(status: u16, body: &str) -> ApiResponse {
        ApiResponse {
            status,
            status_text: status_reason(status).to_string(),
            duration_ms: 0,
            size_bytes: body.len() as u64,
            headers: vec![],
            body: body.to_string(),
            body_json: None,
            content_type: "text/plain".into(),
            error: None,
            method: None,
            url: None,
        }
    }

    // ---- parse ---------------------------------------------------------

    #[test]
    fn parse_json_body_extracts_body_json() {
        let r = parse(r#"{"a":1}"#, "application/json; charset=utf-8", 200, &[]);
        assert_eq!(r.body_json, Some(json!({"a": 1})));
        assert_eq!(r.size_bytes, 7);
        assert_eq!(r.status_text, "OK");
    }

    #[test]
    fn parse_non_json_body_has_no_body_json() {
        let r = parse("hello", "text/plain", 200, &[]);
        assert!(r.body_json.is_none());
        assert_eq!(r.size_bytes, 5);
    }

    #[test]
    fn parse_empty_body_has_zero_size() {
        let r = parse("", "application/json", 204, &[]);
        // Body is empty → no parse, but size and status text are set.
        assert!(r.body_json.is_none());
        assert_eq!(r.size_bytes, 0);
        assert_eq!(r.status_text, "No Content");
    }

    #[test]
    fn parse_invalid_json_yields_none_not_panic() {
        // A JSON content-type header should not turn a malformed body
        // into a hard error — the renderer falls back to "raw text".
        let r = parse("{not valid", "application/json", 200, &[]);
        assert!(r.body_json.is_none());
    }

    #[test]
    fn parse_non_standard_status_has_empty_reason() {
        let r = parse("", "text/plain", 999, &[]);
        assert_eq!(r.status_text, "");
    }

    #[test]
    fn parse_content_type_match_is_case_insensitive() {
        let r = parse(r#"{"x":1}"#, "Application/JSON", 200, &[]);
        assert_eq!(r.body_json, Some(json!({"x": 1})));
    }

    // ---- diff: added ---------------------------------------------------

    #[test]
    fn diff_added_paths_listed() {
        let a = api(200, r#"{"keep":1}"#);
        let b = api(200, r#"{"keep":1,"new_key":"v","nested":{"x":1}}"#);
        let d = diff(&a, &b);
        assert!(d.added_paths.contains(&"$.new_key".to_string()));
        assert!(d.added_paths.contains(&"$.nested.x".to_string()));
        assert!(d.removed_paths.is_empty());
        assert!(d.changed_paths.is_empty());
    }

    // ---- diff: removed ------------------------------------------------

    #[test]
    fn diff_removed_paths_listed() {
        let a = api(200, r#"{"keep":1,"gone":2,"nested":{"x":1,"y":2}}"#);
        let b = api(200, r#"{"keep":1}"#);
        let d = diff(&a, &b);
        assert!(d.removed_paths.contains(&"$.gone".to_string()));
        assert!(d.removed_paths.contains(&"$.nested.x".to_string()));
        assert!(d.removed_paths.contains(&"$.nested.y".to_string()));
        assert!(d.added_paths.is_empty());
        assert!(d.changed_paths.is_empty());
    }

    // ---- diff: changed ------------------------------------------------

    #[test]
    fn diff_changed_paths_carry_from_to() {
        let a = api(200, r#"{"n":1,"s":"old"}"#);
        let b = api(200, r#"{"n":2,"s":"new"}"#);
        let d = diff(&a, &b);
        let n = d
            .changed_paths
            .iter()
            .find(|c| c.path == "$.n")
            .expect("$.n should be marked changed");
        assert_eq!(n.from, json!(1));
        assert_eq!(n.to, json!(2));
        let s = d
            .changed_paths
            .iter()
            .find(|c| c.path == "$.s")
            .expect("$.s should be marked changed");
        assert_eq!(s.from, json!("old"));
        assert_eq!(s.to, json!("new"));
    }

    #[test]
    fn diff_type_change_is_changed_not_removed_added() {
        // `n` flips from number to string — that's a single change,
        // not "remove number, add string".
        let a = api(200, r#"{"n":1}"#);
        let b = api(200, r#"{"n":"1"}"#);
        let d = diff(&a, &b);
        assert_eq!(d.changed_paths.len(), 1);
        assert_eq!(d.changed_paths[0].path, "$.n");
        assert!(d.added_paths.is_empty());
        assert!(d.removed_paths.is_empty());
    }

    #[test]
    fn diff_array_index_paths() {
        let a = api(200, r#"[1,2,3]"#);
        let b = api(200, r#"[1,9,3,4]"#);
        let d = diff(&a, &b);
        let c = d
            .changed_paths
            .iter()
            .find(|c| c.path == "$[1]")
            .expect("$[1] changed");
        assert_eq!(c.from, json!(2));
        assert_eq!(c.to, json!(9));
        assert!(d.added_paths.contains(&"$[3]".to_string()));
    }

    #[test]
    fn diff_array_shrink_marks_tail_removed() {
        let a = api(200, r#"[1,2,3,4]"#);
        let b = api(200, r#"[1,2]"#);
        let d = diff(&a, &b);
        assert!(d.removed_paths.contains(&"$[2]".to_string()));
        assert!(d.removed_paths.contains(&"$[3]".to_string()));
    }

    #[test]
    fn diff_non_json_bodies_compare_as_strings() {
        let a = api_text(200, "hello");
        let b = api_text(200, "world");
        let d = diff(&a, &b);
        assert_eq!(d.changed_paths.len(), 1);
        assert_eq!(d.changed_paths[0].path, "$");
        assert_eq!(d.changed_paths[0].from, json!("hello"));
        assert_eq!(d.changed_paths[0].to, json!("world"));
    }

    #[test]
    fn diff_identical_bodies_have_no_changes() {
        let a = api(200, r#"{"x":1}"#);
        let b = api(200, r#"{"x":1}"#);
        let d = diff(&a, &b);
        assert!(d.added_paths.is_empty());
        assert!(d.removed_paths.is_empty());
        assert!(d.changed_paths.is_empty());
    }

    #[test]
    fn diff_one_side_only_json_marks_body_changed_and_paths_added_or_removed() {
        let a = api(200, r#"{"a":1}"#);
        let b = api_text(200, "<html>oops</html>");
        let d = diff(&a, &b);
        // $ is the body level; every JSON path is "removed" (was in a).
        assert_eq!(d.changed_paths.len(), 1);
        assert_eq!(d.changed_paths[0].path, "$");
        assert!(d.removed_paths.contains(&"$.a".to_string()));
        assert!(d.added_paths.is_empty());

        let a2 = api_text(200, "<html>oops</html>");
        let b2 = api(200, r#"{"a":1}"#);
        let d2 = diff(&a2, &b2);
        assert!(d2.added_paths.contains(&"$.a".to_string()));
        assert!(d2.removed_paths.is_empty());
    }

    // ---- extract_jsonpath --------------------------------------------

    #[test]
    fn extract_jsonpath_root() {
        let v = extract_jsonpath(r#"{"a":1}"#, "application/json", "$");
        assert_eq!(v, Some(json!({"a": 1})));
    }

    #[test]
    fn extract_jsonpath_nested_object_field() {
        let v = extract_jsonpath(
            r#"{"data":{"user":{"name":"alice"}}}"#,
            "application/json",
            "$.data.user.name",
        );
        assert_eq!(v, Some(json!("alice")));
    }

    #[test]
    fn extract_jsonpath_array_index() {
        let v = extract_jsonpath(
            r#"{"items":[{"id":1},{"id":2},{"id":3}]}"#,
            "application/json",
            "$.items[1].id",
        );
        assert_eq!(v, Some(json!(2)));
    }

    #[test]
    fn extract_jsonpath_wildcard_returns_first_match() {
        let v = extract_jsonpath(
            r#"{"xs":[10,20,30]}"#,
            "application/json",
            "$.xs[*]",
        );
        // Wildcard is supported by serde_json_path; we return the
        // first match.
        assert_eq!(v, Some(json!(10)));
    }

    #[test]
    fn extract_jsonpath_no_match_returns_none() {
        let v = extract_jsonpath(r#"{"a":1}"#, "application/json", "$.b");
        assert!(v.is_none());
    }

    #[test]
    fn extract_jsonpath_invalid_path_returns_none() {
        // serde_json_path 0.1 leniently accepts lots of "garbage"
        // strings (e.g. `$$ bad path`, `@@nope`) and quietly resolves
        // them to the root document. To honor the "invalid path ⇒
        // None" contract without pulling in a stricter parser, we
        // require the path to begin with `$` and to NOT contain raw
        // whitespace — RFC 9535 only allows whitespace as a SEGMENT
        // separator, which we don't support here. Paths that fail this
        // gate (or that the underlying parser rejects) return None.
        let v = extract_jsonpath(r#"{"a":1}"#, "application/json", "$$ bad path");
        assert!(v.is_none(), "expected None, got {v:?}");
        let v = extract_jsonpath(r#"{"a":1}"#, "application/json", "totally not a path");
        assert!(v.is_none(), "expected None, got {v:?}");
    }

    #[test]
    fn extract_jsonpath_non_json_returns_none() {
        let v = extract_jsonpath("hello", "text/plain", "$");
        assert!(v.is_none());
    }

    #[test]
    fn extract_jsonpath_malformed_body_returns_none() {
        let v = extract_jsonpath("{not json", "application/json", "$");
        assert!(v.is_none());
    }
}