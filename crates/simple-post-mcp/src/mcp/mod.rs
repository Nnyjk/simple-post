//! MCP daemon — top-level entry, dispatch table, shared types.
//!
//! File layout:
//!
//! ```text
//!   mcp/
//!     mod.rs             ← you are here (entry + dispatch + shared types)
//!     env_ops.rs         ← 8 env_* tools
//!     project_ops.rs     ← project / module / collection / endpoint / baseUrlDef
//!     http_ops.rs        ← api_run_request, api_call_endpoint
//!     variable_fill.rs   ← api_resolve_template / endpoint / validate
//!     response_parse.rs  ← api_parse_response / diff / extract_jsonpath
//! ```
//!
//! ## Where the storage types come from
//!
//! Every domain type, DB function, HTTP executor, variable resolver, response
//! parser, and permission gate lives in another module:
//!
//! | concern            | module                                    |
//! |--------------------|-------------------------------------------|
//! | domain types       | `simple_post_core::domain::*`                        |
//! | storage            | `simple_post_core::db::*`                            |
//! | HTTP execution     | `simple_post_core::http_executor::*`                 |
//! | var + baseUrl fill | `crate::variable_fill::*`                 |
//! | response parse     | `crate::response_parse::*`                |
//! | permission         | `simple_post_core::permissions::*`                   |
//! | metrics            | `simple_post_core::mcp_metrics::*`                   |
//! | errors             | `simple_post_core::error::AppError`                  |
//!
//! This module is the glue — nothing domain-shaped is defined here, only
//! the dispatch wiring and the JSON-RPC envelope types.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod env_ops;
pub mod http_ops;
pub mod project_ops;
pub mod response_parse;
pub mod variable_fill;

use simple_post_core::permissions::{resolve_capability, Capability, CapabilityPolicy};

// =========================================================================
// Public entry: serve_http
// =========================================================================

/// Bind the MCP HTTP daemon on `bind:port` and spawn the axum server.
///
/// `port = 0` requests an OS-assigned port (useful for tests + the dev
/// supervisor that records the chosen port in a file). The returned
/// `u16` is the *actual* port — pass it back to the renderer / proxy so
/// they can connect.
///
/// This is the single entry point `lib.rs::run_mcp` calls.
pub async fn serve_http(
    pool: simple_post_core::db::DbPool,
    port: u16,
    bind: &str,
) -> Result<u16, simple_post_core::error::AppError> {
    let listener = tokio::net::TcpListener::bind((bind, port))
        .await
        .map_err(|e| simple_post_core::error::AppError::Http(format!("bind {bind}:{port}: {e}")))?;
    let actual_port = listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| simple_post_core::error::AppError::Http(format!("local_addr: {e}")))?;

    let state = Arc::new(AppState {
        pool,
        started_at: Instant::now(),
        metrics: Arc::new(simple_post_core::mcp_metrics::McpMetrics::new()),
    });

    tokio::spawn(async move {
        if let Err(e) = crate::daemon_http::serve(listener, state).await {
            tracing::error!(error = %e, "MCP daemon exited with error");
        }
    });

    Ok(actual_port)
}

// =========================================================================
// App-wide state
// =========================================================================

/// Per-daemon state shared across every axum handler.
pub struct AppState {
    /// DB pool. `r2d2::Pool` is cheap to clone (Arc inside) — `AppState`
    /// is also wrapped in an Arc, and the router hands out a clone of
    /// *that* to each handler; we clone the pool again when actually
    /// touching it.
    pub pool: simple_post_core::db::DbPool,
    /// Wall-clock anchor for uptime reporting.
    pub started_at: Instant,
    /// Live counters — bumped on every handled request so
    /// `/mcp/healthz` and the `mcp_status` Tauri command can show them.
    pub metrics: Arc<simple_post_core::mcp_metrics::McpMetrics>,
}

// =========================================================================
// JSON-RPC envelope (re-exported by daemon_http.rs)
// =========================================================================

/// JSON-RPC 2.0 response envelope. Exactly one of `result` / `error`
/// is `Some` — the other is `None` and `#[serde(skip_serializing_if)]`
/// drops it from the wire.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Build a successful response.
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response (transport-level, not tool-level).
    pub fn err(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: id.unwrap_or(Value::Null),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// =========================================================================
// MCP tool call result
// =========================================================================

/// The shape `tools/call` returns in its `result` field. Mirrors the MCP
/// `CallToolResult` schema (subset — we only emit text content today).
#[derive(Debug, Serialize)]
pub struct CallToolResult {
    pub content: Vec<CallToolContent>,
    #[serde(skip_serializing_if = "is_false")]
    pub is_error: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CallToolContent {
    Text { text: String },
}

impl CallToolResult {
    /// Convenience: success result wrapping a JSON value (serialized as text).
    pub fn json(value: impl Serialize) -> Self {
        let text = serde_json::to_string(&value)
            .unwrap_or_else(|e| format!("{{\"__serialize_error\":\"{}\"}}", e));
        Self {
            content: vec![CallToolContent::Text { text }],
            is_error: false,
        }
    }

    /// Convenience: success result wrapping a plain string.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// Convenience: error result. Per spec, `is_error: true` is returned as
    /// a *successful* tool invocation so the LLM can read the message and
    /// continue the conversation.
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            content: vec![CallToolContent::Text { text: text.into() }],
            is_error: true,
        }
    }
}

// =========================================================================
// Tool descriptor
// =========================================================================

/// Static description of a registered tool — name, doc string, and the JSON
/// Schema for its `arguments`. Returned by `tools/list`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

// =========================================================================
// Tool function signature
// =========================================================================

/// Every tool funnels through this signature. `args` is the full
/// `arguments` object the client sent (or `Value::Null` if none). The
/// handler is responsible for picking out the fields it cares about and
/// mapping missing / wrong-typed ones into `AppError::Validation`.
///
/// The `'a` HRTB ties the returned future's lifetime to the borrow on
/// `&AppState` — without it the boxed `dyn Future` would default to
/// `'static` and the `&AppState` borrow would be rejected.
pub type ToolFn = for<'a> fn(
    state: &'a AppState,
    args: Value,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<CallToolResult, simple_post_core::error::AppError>>
            + Send
            + 'a,
    >,
>;

/// Wrap an `async fn tool_xxx(&AppState, Value) -> Result<...>` into the
/// boxed-future `ToolFn` shape the dispatch table stores.
///
/// Use at every `func:` slot in `table()`:
///
/// ```ignore
/// ToolEntry { name: ..., func: tool_fn!(tool_xxx), ... }
/// ```
///
/// `#[macro_export]` puts it at the crate root, so call sites just write
/// `crate::tool_fn!(...)`. The macro body uses `$crate::mcp::AppState` /
/// `CallToolResult` and `simple_post_core::error::AppError` so it doesn't depend on
/// any `use` imports at the call site. `serde_json::Value` is the one
/// unqualified path — every submodule already imports it.
#[macro_export]
macro_rules! tool_fn {
    ($f:path) => {{
        fn __boxed<'a>(
            state: &'a $crate::mcp::AppState,
            args: serde_json::Value,
        ) -> ::std::pin::Pin<
            Box<
                dyn ::std::future::Future<
                        Output = ::std::result::Result<
                            $crate::mcp::CallToolResult,
                            ::simple_post_core::error::AppError,
                        >,
                    > + ::std::marker::Send
                    + 'a,
            >,
        > {
            Box::pin($f(state, args))
        }
        __boxed
    }};
}

// =========================================================================
// Dispatch table
// =========================================================================

/// One row of the dispatch table. Hand-rolled instead of a `HashMap` so the
/// lookup is branchless and the order of `tools/list` is stable.
pub(crate) struct ToolEntry {
    name: &'static str,
    description: &'static str,
    /// JSON Schema for the `arguments` object. Kept as a `Value` literal so
    /// it survives `cargo check` without a `schemars` round-trip.
    schema: Value,
    func: ToolFn,
}

/// The full tool registry. Order here is the order `tools/list` returns
/// them — we group by category so humans reading `/mcp/tools` see a
/// clean list.
fn tool_table() -> &'static [ToolEntry] {
    static TABLE: std::sync::OnceLock<Vec<ToolEntry>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = Vec::with_capacity(64);

        // env_ops: 8 tools
        v.extend(env_ops::table());

        // project_ops: project + module + collection + endpoint + baseUrl
        v.extend(project_ops::table());

        // http_ops: 2 tools
        v.extend(http_ops::table());

        // variable_fill (MCP adapter): 3 tools
        v.extend(variable_fill::table());

        // response_parse (MCP adapter): 3 tools
        v.extend(response_parse::table());

        v
    })
}

/// Snapshot of all tool names (for `/mcp/healthz` + sanity checks).
///
/// **NOTE:** This returns the full unfiltered registry (every tool the
/// crate ships), not the user-visible filtered set. Callers that want
/// the visibility-after-policy set should use [`tool_descriptors`]
/// instead. Used by healthz + metrics — neither needs the user's view.
pub fn registered_tool_names() -> Vec<String> {
    tool_table().iter().map(|e| e.name.to_string()).collect()
}

/// Map a tool name to the global [`Capability`] class it requires.
///
/// This table is **exhaustive**: every entry in [`tool_table`] must appear
/// here. Both capability gates fail *closed* on a missing entry, so a tool
/// added to the registry without a capability row is denied rather than
/// silently exempted. `every_registered_tool_has_a_capability` is the
/// regression net that keeps the two sets in sync.
///
/// (The per-project [`simple_post_core::permissions::required_permission`]
/// table is a separate, coarser gate — it defaults unknown tools to
/// `Permission::Full`. This table is what enforces the user's global
/// `read_only` / `safe_write` / `full_access` choice, so it cannot afford
/// a default.)
pub const REQUIRED_CAPABILITY: &[(&str, Capability)] = &[
    // env
    ("env_list", Capability::Read),
    ("env_get_active", Capability::Read),
    ("env_create", Capability::Create),
    ("env_update", Capability::Update),
    ("env_delete", Capability::Delete),
    ("env_set_var", Capability::Update),
    ("env_set_baseurl", Capability::Update),
    ("env_set_active", Capability::Update),
    // project
    ("project_list", Capability::Read),
    ("project_get", Capability::Read),
    ("project_create", Capability::Create),
    ("project_update", Capability::Update),
    ("project_delete", Capability::Delete),
    // module
    ("module_list", Capability::Read),
    ("module_create", Capability::Create),
    ("module_update", Capability::Update),
    ("module_delete", Capability::Delete),
    ("module_move", Capability::Update),
    // collection
    ("collection_list", Capability::Read),
    ("collection_create", Capability::Create),
    ("collection_update", Capability::Update),
    ("collection_delete", Capability::Delete),
    ("collection_move", Capability::Update),
    // endpoint
    ("endpoint_list", Capability::Read),
    ("endpoint_get", Capability::Read),
    ("endpoint_search", Capability::Read),
    ("endpoint_create", Capability::Create),
    ("endpoint_update", Capability::Update),
    ("endpoint_delete", Capability::Delete),
    ("endpoint_duplicate", Capability::Create),
    ("endpoint_move", Capability::Update),
    // baseUrl definitions
    ("base_url_def_list", Capability::Read),
    ("base_url_def_create", Capability::Create),
    ("base_url_def_update", Capability::Update),
    ("base_url_def_delete", Capability::Delete),
    ("base_url_def_set_default", Capability::Update),
    // HTTP execution — outbound network calls. Highest trust.
    ("api_run_request", Capability::Send),
    ("api_call_endpoint", Capability::Send),
    // Read-side variable / endpoint helpers
    ("api_resolve_template", Capability::Read),
    ("api_resolve_endpoint", Capability::Read),
    ("api_validate_endpoint", Capability::Read),
    ("api_parse_response", Capability::Read),
    ("api_diff_responses", Capability::Read),
    ("api_extract_jsonpath", Capability::Read),
];

/// O(n) lookup over the small `REQUIRED_CAPABILITY` table. Returns
/// `None` for unrecognised names — callers must handle the conservative
/// "deny" path themselves. The dispatcher uses this on every
/// `tools/call`; the table is intentionally linear (one row per
/// registered tool) so a hash map would be premature optimisation.
pub fn required_capability(name: &str) -> Option<Capability> {
    REQUIRED_CAPABILITY
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Snapshot of every tool's full descriptor (for `tools/list` +
/// `/mcp/tools`). Filters out tools whose required [`Capability`] is
/// not covered by the **current** [`CapabilityPolicy`] — this is the
/// single source of truth for the LLM-visible inventory. A misconfigured
/// policy should never leak a write tool to the AI.
///
/// The default policy when none is set is [`CapabilityPolicy::ReadOnly`]
/// (safe-by-default). A user who hasn't visited the settings panel yet
/// sees only the read-only inventory.
pub async fn tool_descriptors(state: &AppState) -> Vec<ToolDescriptor> {
    let policy = resolve_capability(&state.pool, None)
        .await
        .unwrap_or(CapabilityPolicy::default());
    tool_table()
        .iter()
        .filter(|e| {
            // Fail **closed**: a registered tool with no capability row is a
            // registry bug, and hiding it is safer than exposing a write
            // tool to an AI client that the user's policy never approved.
            required_capability(e.name)
                .map(|req| policy.satisfies(req))
                .unwrap_or(false)
        })
        .map(|e| ToolDescriptor {
            name: e.name.to_string(),
            description: e.description.to_string(),
            input_schema: e.schema.clone(),
        })
        .collect()
}

/// Dispatch a `tools/call` by name. Unknown name → `AppError::NotFound`
/// (mapped to `is_error: true` by the JSON-RPC handler so the LLM sees
/// the missing tool name).
///
/// Applies the global capability gate as a **second** check (the first
/// is `tool_descriptors` for `tools/list`). A direct `tools/call` to
/// a write tool while the policy is `ReadOnly` returns
/// `AppError::PermissionDenied` — the LLM never reaches the handler.
pub async fn dispatch(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<CallToolResult, simple_post_core::error::AppError> {
    // Resolve the handler *first* so an unregistered name still returns
    // `NotFound` rather than being masked by the capability gate.
    let entry = tool_table()
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| {
            simple_post_core::error::AppError::NotFound(format!("tool '{name}' not registered"))
        })?;

    // Capability pre-flight: deny before the handler runs, so a
    // misconfigured LLM that calls `tools/call` directly without
    // re-reading `tools/list` still cannot mutate. Resolves the
    // effective policy the same way `tool_descriptors` does — global
    // preference + (no project here) = just the preference.
    //
    // Fails **closed**: a registered tool with no `REQUIRED_CAPABILITY`
    // row is denied. See that table's docs — the omission is always a
    // registry bug, never an implicit "allow".
    match required_capability(name) {
        Some(req) => {
            let policy = resolve_capability(&state.pool, None).await?;
            if !policy.satisfies(req) {
                return Err(simple_post_core::error::AppError::PermissionDenied(
                    format!(
                        "tool '{name}' requires {:?}, current policy is {:?}",
                        req, policy
                    ),
                ));
            }
        }
        None => {
            return Err(simple_post_core::error::AppError::PermissionDenied(
                format!(
                    "tool '{name}' has no capability classification; \
                     refusing to execute (registry bug — add it to REQUIRED_CAPABILITY)"
                ),
            ));
        }
    }
    (entry.func)(state, args).await
}

// =========================================================================
// Argument helpers (used by every tool impl)
// =========================================================================

/// Pull a required string field out of `args`. Missing or wrong-typed → error.
pub fn arg_str<'a>(
    args: &'a Value,
    key: &str,
) -> Result<&'a str, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-string '{key}'"))
    })
}

/// Pull an optional string field. Returns `None` if missing or null.
pub fn arg_str_opt(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(String::from)
}

/// Pull a required u64 field.
pub fn arg_u64(args: &Value, key: &str) -> Result<u64, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_u64).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-u64 '{key}'"))
    })
}

/// Pull an optional u64 field.
pub fn arg_u64_opt(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

/// Pull a required boolean field.
pub fn arg_bool(args: &Value, key: &str) -> Result<bool, simple_post_core::error::AppError> {
    args.get(key).and_then(Value::as_bool).ok_or_else(|| {
        simple_post_core::error::AppError::Validation(format!("missing or non-bool '{key}'"))
    })
}

/// Pull an optional boolean field. Defaults to `false` if missing.
pub fn arg_bool_opt(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

// =========================================================================
// Permission helper — thin wrapper over simple_post_core::permissions so call sites
// don't need to import it directly.
// =========================================================================

/// Look up the project's stored permission level and compare against the
/// level required by `tool_name`. On deny, returns `AppError::PermissionDenied`.
pub async fn check_permission(
    pool: &simple_post_core::db::DbPool,
    project_id: &str,
    tool_name: &str,
) -> Result<(), simple_post_core::error::AppError> {
    let required = simple_post_core::permissions::required_permission(tool_name);
    simple_post_core::db::with_conn(pool, |conn| {
        simple_post_core::permissions::require(conn, project_id, required)
    })
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
