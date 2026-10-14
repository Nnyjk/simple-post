// ---------------------------------------------------------------------------
// Create-options and move-target DTOs. Patch types live in `crate::domain`
// and are pulled in via `use crate::domain::*`.
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

use crate::domain::*;
use crate::permissions::CapabilityPolicy;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectOpts {
    pub description: Option<String>,
    pub color: Option<String>,
    // No `baseUrl`. It was introduced together with the "new project
    // seeds a default service holding a URL" model, and that model is
    // gone: `create_with_defaults` writes the project, a default
    // environment and a default module — and no service. A base URL
    // belongs to (environment × module × service), so it needs a service
    // name the user has not chosen yet; the user fills the cartesian grid
    // in settings instead.
}

/// Tauri 与 Web 共用的能力策略返回形状。
#[derive(Debug, Clone, Serialize)]
pub struct McpCapabilityDto {
    pub policy: CapabilityPolicy,
}

/// The endpoint request configuration a `nodes_create` call carries
/// alongside its structural node.
///
/// One optional block, all fields optional: an endpoint created from the
/// tree's "new request" action starts empty and is filled in afterwards,
/// and every column here has a sensible default for that case.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointDetailOpts {
    pub method: Option<HttpMethod>,
    pub url: Option<String>,
    pub environment_id: Option<String>,
    pub base_url_definition_id: Option<String>,
    pub body: Option<RequestBody>,
    pub auth: Option<AuthConfig>,
    pub tags: Option<Vec<String>>,
    pub headers: Option<Vec<KeyValue>>,
    pub params: Option<Vec<KeyValue>>,
    pub docs: Option<String>,
    pub notes: Option<String>,
}

impl CreateEndpointDetailOpts {
    /// Expand into the row `db::nodes::create` writes. `node_id` is
    /// threaded in rather than left to the caller so the detail cannot
    /// be built pointing at a node that was never inserted.
    ///
    /// `pub` rather than `pub(crate)`: `POST /api/nodes` (this crate)
    /// and the `nodes_create` Tauri command (`src-tauri`) accept the
    /// same opts type and expand it through here, so both transports
    /// default a half-filled endpoint identically. It was `pub(crate)`
    /// while this type lived in `commands`; the crate split made the
    /// second caller a genuine cross-crate one.
    pub fn into_detail(self, node_id: &str) -> EndpointDetail {
        EndpointDetail {
            node_id: node_id.to_string(),
            method: self.method.unwrap_or_default(),
            url: self.url.unwrap_or_default(),
            environment_id: self.environment_id,
            base_url_definition_id: self.base_url_definition_id,
            base_url_override: None,
            params: self.params.unwrap_or_default(),
            headers: self.headers.unwrap_or_default(),
            body: self.body.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            docs: self.docs,
            notes: self.notes,
            tags: self.tags.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnvironmentOpts {
    pub base_urls: Option<std::collections::HashMap<String, String>>,
    pub variables: Option<std::collections::HashMap<String, String>>,
    pub make_active: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBaseUrlDefOpts {
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettingsPatch {
    pub timeout_ms: Option<u64>,
    pub follow_redirects: Option<bool>,
    pub max_response_size_kb: Option<u64>,
}

// ---------------------------------------------------------------------------
// MCP daemon DTO — single source of truth for the daemon's wire shape.
// `crate::daemon_supervisor::McpDaemonHandle::snapshot()` returns this
// type; the `mcp_status` Tauri command forwards it verbatim.
// ---------------------------------------------------------------------------

/// Snapshot of the embedded MCP daemon. Returned by `mcp_status` and
/// periodically polled by the UI to show "is the AI bridge running?".
///
/// The wire shape matches `src/lib/mcp-types.ts::McpUiState` exactly.
/// `status` is the source-of-truth lifecycle string (mirrors the
/// internal `DaemonStatus` enum via `as_str()`); `running` is kept
/// as a derived boolean so any caller still consuming the old shape
/// keeps working without a forced migration.
///
/// This whole module used to live in `src-tauri/src/commands/dto.rs`,
/// i.e. inside the Tauri crate. That put every wire DTO in the
/// dependency cone of the GUI runtime, which is why the server shape
/// had to reach through `crate::commands::…` for a plain struct. None
/// of these types are Tauri-specific — they are `Serialize` /
/// `Deserialize` bags — so they moved down here, below both hosts.
/// `src-tauri/src/commands/dto.rs` re-exports this module, so the old
/// `crate::commands::Foo` paths keep resolving to the same types.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    /// Lifecycle state, one of `"stopped" | "starting" | "running" |
    /// "restarting" | "crashed" | "unhealthy"`. New field; consumers
    /// that only read `running` continue to work because we set
    /// `running = (status == "running")`.
    pub status: String,
    /// Back-compat boolean. `true` iff `status == "running"`. Older
    /// callers can keep reading this without learning the lifecycle
    /// enum; the badge/UI logic has migrated to `status`.
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    /// Number of currently-active MCP client connections. The metrics
    /// module does not expose a counter for this yet, so the
    /// supervisor writes `0` until the daemon learns to emit it via
    /// stderr; UI surfaces this as a "0 connections" line.
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl McpStatusDto {
    /// Safe "not running" snapshot. `registered_tools` is empty because
    /// no daemon means no tools surface — callers learn the tool list
    /// on the next poll after the daemon comes back up.
    pub fn stopped() -> Self {
        Self {
            status: "stopped".to_string(),
            running: false,
            pid: None,
            port: None,
            uptime_ms: 0,
            requests_handled: 0,
            current_connections: 0,
            registered_tools: Vec::new(),
            last_error: None,
        }
    }
}

/// Returned by the `mcp_daemon_restart` Tauri command. Distinct from
/// `McpStatusDto` because the restart path is the one place we
/// deliberately swallow the underlying `AppError` (so the UI can
/// render a uniform `{ ok: false, ... }` envelope without an extra
/// `.catch` on every call site).
///
/// F-D4 fix: `pid` and `port` deliberately serialize as JSON `null` on
/// the failure path (rather than being skipped) so the response shape
/// is stable: `{ ok, pid, port }` always. This lines up with the
/// renderer's `RestartResult` type in `src/lib/mcp-types.ts`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResult {
    pub ok: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
}
