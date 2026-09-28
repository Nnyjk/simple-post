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
    let v = extract_jsonpath(r#"{"xs":[10,20,30]}"#, "application/json", "$.xs[*]");
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
