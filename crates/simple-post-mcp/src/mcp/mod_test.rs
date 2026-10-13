use super::*;
use serde_json::json;

/// Cheap fake pool for tests. `r2d2::Pool` is generic over the manager,
/// so we can't construct one without a real `SqliteConnectionManager` —
/// tests that need a real DB use `db::init` against a temp file. For
/// smoke tests that only exercise dispatch + arg parsing, a dummy is fine.
fn test_state() -> Arc<AppState> {
    Arc::new(AppState {
        pool: simple_post_core::db::init(&std::env::temp_dir().join("simple-post-mcp-test"))
            .unwrap(),
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    })
}

/// `tools/list` returns every registered tool, including all 5
/// categories. This is the primary regression net — if a worker adds
/// a tool but forgets the dispatch table, this catches it.
#[test]
fn tools_list_returns_all_registered_tools() {
    let names = registered_tool_names();
    assert!(!names.is_empty(), "no tools registered");

    // Spot-check: every category has at least one tool.
    let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    assert!(has("env_"), "missing env_* tools");
    assert!(has("project_"), "missing project_* tools");
    assert!(has("module_"), "missing module_* tools");
    assert!(has("collection_"), "missing collection_* tools");
    assert!(has("endpoint_"), "missing endpoint_* tools");
    assert!(has("base_url_def_"), "missing base_url_def_* tools");
    assert!(has("api_run_request"), "missing api_run_request");
    assert!(has("api_call_endpoint"), "missing api_call_endpoint");
    assert!(has("api_resolve_"), "missing api_resolve_* tools");
    assert!(has("api_validate_"), "missing api_validate_* tools");
    assert!(has("api_parse_response"), "missing api_parse_response");
    assert!(has("api_diff_responses"), "missing api_diff_responses");
    assert!(has("api_extract_jsonpath"), "missing api_extract_jsonpath");

    // Tool count sanity check — spec asks for ~28+ tools; the
    // assertion is intentionally loose so individual workers can add
    // tools without breaking the test, but tight enough to catch
    // accidentally dropping a category.
    assert!(
        names.len() >= 30,
        "tool count too low: {} — did a category get dropped?",
        names.len()
    );
}

/// `REQUIRED_CAPABILITY` must stay **exhaustive** over the registry.
///
/// Regression net for a real defect: `module_move` and
/// `collection_move` were registered as tools but had no capability
/// row. Both gates treat a missing row as "unclassified" — the
/// `tools/list` filter used to keep such tools *visible* and the
/// dispatcher used to *skip* its check, so a `read_only` global
/// policy never gated them. Both now fail closed; this test is what
/// stops the next tool from silently escaping its policy.
#[test]
fn every_registered_tool_has_a_capability() {
    let missing: Vec<&str> = tool_table()
        .iter()
        .filter(|e| required_capability(e.name).is_none())
        .map(|e| e.name)
        .collect();
    assert!(
        missing.is_empty(),
        "tools missing a REQUIRED_CAPABILITY row (they bypass the global \
             policy gate): {missing:?}"
    );
}

/// The exact tool count, asserted where the docs can point at it.
///
/// `AGENTS.md` and `README.md` both state "44 tools". The bijection
/// test above stays green when a tool is added or removed *with* its
/// capability row, so on its own it cannot keep those numbers honest.
/// Bump this constant in the same commit that changes the registry, and
/// the same commit should update the two documents.
const EXPECTED_TOOL_COUNT: usize = 44;

#[test]
fn registered_tool_count_matches_the_documented_figure() {
    assert_eq!(
        tool_table().len(),
        EXPECTED_TOOL_COUNT,
        "tool count changed; update EXPECTED_TOOL_COUNT and the '44 tools' \
             claims in AGENTS.md and README.md"
    );
}

/// The reverse direction: a capability row for a tool that no longer
/// exists is dead configuration that hides a registry rename.
#[test]
fn every_capability_row_names_a_registered_tool() {
    let registered = registered_tool_names();
    let stale: Vec<&str> = REQUIRED_CAPABILITY
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !registered.iter().any(|r| r == n))
        .collect();
    assert!(
        stale.is_empty(),
        "REQUIRED_CAPABILITY rows with no matching tool: {stale:?}"
    );
}

/// Gate B regression net — a permission check must never be discarded.
///
/// `.ok()` on a `check_permission(..).await` result silently swallows a
/// `PermissionDenied`, which turns the per-project gate into a no-op for
/// that tool. It shipped once on `api_call_endpoint`, the one tool that
/// performs an outbound HTTP request. Propagate with `?` instead.
#[test]
fn no_permission_check_result_is_discarded() {
    let offenders = source_files()
        .into_iter()
        .filter_map(|(path, text)| {
            let swallowed = text.lines().any(|l| l.trim_start().starts_with(".ok();"));
            let gates = text.contains("check_permission(");
            (swallowed && gates).then_some(path)
        })
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "these files call check_permission and also end a statement with `.ok();` \
         — confirm each one is a deliberate, non-permission discard: {offenders:?}"
    );
}

/// Gate B regression net — never gate a tool against `"global"`.
///
/// `agent_permissions.project_id` is an FK to `projects(id)`, and project
/// ids are minted `proj_<millis>`, so `"global"` can never have a row. The
/// lookup then falls back to `Permission::default()` = `ReadCall`, while
/// any write tool requires `Full` — so `ReadCall < Full` denies and the
/// tool is permanently dead while still looking correctly registered. This
/// shipped on five tools at once.
///
/// It was not merely a dead-tool bug. When the caller scoped a read with a
/// `collection_id` or `module_id` instead of a `project_id`, the handler
/// passed `"global"` and so consulted **no real project's row at all** —
/// the owning project's real permission was never read, on either side of
/// the gate. `endpoint_list` and `endpoint_search` shipped that way, which
/// is why this test used to carry an allowlist naming them: two `Read`-tier
/// tools that scope every project genuinely have no single owning project,
/// and `"global"` was the only id available to express that.
///
/// Both now resolve the real owner from whatever the caller actually
/// supplied (`nodes::root_id_of` for a module / collection, the id itself
/// for a project) and **refuse an unscoped call** rather than fabricate an
/// id — an unscoped read spans projects, and the per-project gate has no
/// single project to authorise against. So the allowlist is empty and the
/// test is unconditional: there is no longer any correct use of the
/// literal.
#[test]
fn no_gate_is_passed_the_literal_global() {
    let offenders = source_files()
        .into_iter()
        .filter_map(|(path, text)| {
            // Match the *argument*, not one spelling of the call, so
            // `check_permission(&p, "global", ..)` and
            // `check_permission(&p, x.unwrap_or("global"), ..)` are both
            // caught. A call is scanned up to its `;`, because the argument
            // sits after nested parens (`&state.pool`) that a naive
            // "up to the first `)`" scan would stop at.
            let offenders = text
                .match_indices("check_permission(")
                .filter_map(|(i, _)| {
                    let tail = &text[i..];
                    let end = tail.find(';')?;
                    let call = &tail[..end];
                    if !call.contains("\"global\"") {
                        return None;
                    }
                    Some((path.clone(), call.trim().replace('\n', " ")))
                })
                .collect::<Vec<_>>();
            (!offenders.is_empty()).then_some(offenders)
        })
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "these files gate against the literal \"global\", which is not a project id \
         and makes every call consult no real permission row: {offenders:?}"
    );
}

/// Every `.rs` file in this crate, with its contents. The two tests above
/// are source-level rather than behavioural on purpose: the bug they guard
/// is invisible to a passing test suite, because a permanently-denied tool
/// still returns a well-formed `PermissionDenied`.
fn source_files() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // Skip test modules: this file quotes the very call shape it
                // is looking for in its own doc comments, so scanning it
                // would report itself as an offender forever.
                let is_test = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_test.rs") || n == "mod_test.rs");
                if is_test {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), text));
                }
            }
        }
    }
    assert!(!out.is_empty(), "source_files() found no .rs files");
    out
}

/// `dispatch` for an unknown name returns `NotFound`. This is the
/// error path the JSON-RPC handler turns into `is_error: true`.
#[tokio::test]
async fn dispatch_unknown_tool_returns_not_found() {
    let state = test_state();
    let res = dispatch(&state, "does_not_exist", json!({})).await;
    assert!(matches!(
        res,
        Err(simple_post_core::error::AppError::NotFound(_))
    ));
}

/// Argument helpers behave correctly on the boundary cases that bit
/// us in earlier iterations: missing key, wrong type, null.
#[test]
fn arg_helpers_handle_missing_or_wrong_type() {
    let v = json!({ "name": "x", "count": 7, "flag": true });
    assert_eq!(arg_str(&v, "name").unwrap(), "x");
    assert!(arg_str(&v, "missing").is_err());
    assert!(arg_str(&v, "count").is_err());
    assert_eq!(arg_u64(&v, "count").unwrap(), 7);
    assert!(arg_bool(&v, "flag").unwrap());

    let v2 = json!({});
    assert_eq!(arg_str_opt(&v2, "x"), None);
    assert_eq!(arg_u64_opt(&v2, "x"), None);
    assert!(!arg_bool_opt(&v2, "x"));
}

/// `JsonRpcResponse::ok` produces a valid response envelope.
#[test]
fn json_rpc_response_ok_shape() {
    let r = JsonRpcResponse::ok(Some(json!(1)), json!({"x": 1}));
    assert_eq!(r.jsonrpc, "2.0");
    assert_eq!(r.id, json!(1));
    assert!(r.result.is_some());
    assert!(r.error.is_none());
}

/// `JsonRpcResponse::err` produces an error envelope.
#[test]
fn json_rpc_response_err_shape() {
    let r = JsonRpcResponse::err(Some(json!(2)), -32601, "method not found");
    assert_eq!(r.jsonrpc, "2.0");
    assert_eq!(r.id, json!(2));
    assert!(r.result.is_none());
    let e = r.error.unwrap();
    assert_eq!(e.code, -32601);
    assert_eq!(e.message, "method not found");
}
