// -------------------------------------------------------------------------
// Wire DTOs (camelCase to match the TS side) + wire helpers.
// -------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthDto {
    pub status: &'static str,
    pub version: &'static str,
    pub shape: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSendBody {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub status: String,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub uptime_ms: u64,
    pub requests_handled: u64,
    pub current_connections: u64,
    pub registered_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl From<crate::commands::McpStatusDto> for McpStatusDto {
    fn from(s: crate::commands::McpStatusDto) -> Self {
        // The Tauri command DTO already has the right shape — copy
        // field-by-field so a future rename on either side doesn't
        // silently desync.
        Self {
            status: s.status,
            running: s.running,
            pid: s.pid,
            port: s.port,
            uptime_ms: s.uptime_ms,
            requests_handled: s.requests_handled,
            current_connections: s.current_connections,
            registered_tools: s.registered_tools,
            last_error: s.last_error,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PreferencesPutBody {
    /// Single key to set. Maps directly to a `preferences` table row.
    pub key: String,
    /// JSON-encoded value. Stored verbatim by `db::preferences::set`.
    pub value: Value,
}

// -------------------------------------------------------------------------
// Wire helpers — kept tiny, but server-injected fields all flow through
// here so the SPA vs REST contract stays 1:1 with the Tauri command path.
// -------------------------------------------------------------------------

/// Fill in a server-generated `id` when the caller (SPA) omits one.
/// Mirrors what every `commands.rs::*_create` does: take the user-facing
/// fields, then stamp `id = uid()`. Without this, every REST create
/// fails with `[validation] missing field 'id'` because the domain
/// structs all require `id` while the SPA only ships name / description
/// / color / etc.
pub fn ensure_server_id(body: &mut Value) {
    let needs_fill = match body.get("id") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    };
    if needs_fill {
        body["id"] = json!(crate::domain::uid());
    }
}

// -------------------------------------------------------------------------
// Wire DTOs for the resource CRUD surface. Keep this block small —
// every shape that's already in `domain::*` is reused directly so
// the wire contract stays 1:1 with the TS side.
// -------------------------------------------------------------------------

/// Body for `POST /api/endpoints`. Mirrors [`domain::Endpoint`] but
/// omits the timestamps — the server fills them with `now_ms()` so
/// callers don't have to know the millisecond-clock invariant.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointWrite {
    /// Optional — server fills via `ensure_server_id` when omitted.
    #[serde(default)]
    pub id: String,
    pub collection_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub method: crate::domain::HttpMethod,
    pub url: String,
    #[serde(default)]
    pub environment_id: Option<String>,
    #[serde(default)]
    pub base_url_definition_id: Option<String>,
    #[serde(default)]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub params: Vec<crate::domain::KeyValue>,
    #[serde(default)]
    pub headers: Vec<crate::domain::KeyValue>,
    #[serde(default)]
    pub body: crate::domain::RequestBody,
    #[serde(default)]
    pub auth: crate::domain::AuthConfig,
    #[serde(default)]
    pub docs: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub sort_order: i64,
}

/// Body for `POST /api/endpoints/search`. All fields optional —
/// omitted filters don't constrain the result. Mirrors the args of
/// `db::endpoints::search` (q, method, tags) so the same SQL runs.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EndpointsSearchBody {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Body for `POST /api/endpoints/move`. `endpoint_ids` is the
/// caller-ordered list (drag-and-drop preserves selection order).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointMoveBody {
    pub endpoint_ids: Vec<String>,
    pub collection_id: String,
    pub index: i32,
}

/// Body for `POST /api/collections/move`. `parent_kind` is one of
/// `"module"` (reparent to top-level under a module) or
/// `"collection"` (reparent to be a child of a sibling collection).
/// Unknown values surface as `AppError::Validation` from the DB
/// layer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMoveBody {
    pub collection_ids: Vec<String>,
    pub parent_kind: String,
    pub parent_id: String,
    pub index: i32,
}

/// Body for `POST /api/modules/move`. `target_index` is the zero-based
/// slot the moved module lands at; clamping happens in the DB layer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleMoveBody {
    pub module_id: String,
    pub target_index: i32,
}

/// Body for `POST /api/environments/:envId/vars`. `value = null`
/// deletes the variable (mirrors `db::environments::set_var`'s
/// "None removes the key" semantics).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetVarBody {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Body for `POST /api/environments/:envId/vars/rename`. Carries both
/// halves of the rename so the backend can do it in one transaction —
/// a client must NOT compose this as a delete followed by a set, which
/// leaves a window where the variable does not exist.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvRenameVarBody {
    pub from_key: String,
    pub to_key: String,
}

/// Body for `POST /api/environments/:envId/baseurls`. `url = null`
/// removes the slot.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetBaseUrlBody {
    pub def_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// Body for `POST /api/environments/active`. `envId` lives in the
/// body (not the URL) so the literal route `/active` can act as the
/// "set the active environment" verb without colliding with
/// `/api/environments/:envId`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveEnvBody {
    pub env_id: String,
}

/// Body for `PUT /api/permissions/:projectId`. The `permission`
/// string is parsed via [`domain::Permission::parse`] — a typo at the
/// call site fails with `AppError::Validation` instead of writing a
/// bogus row.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionPutBody {
    pub permission: String,
}

/// Body for `POST /api/projects/:projectId/baseurls/default`.
/// `def_id = null` clears the project's default pointer.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefSetDefaultBody {
    #[serde(default)]
    pub def_id: Option<String>,
}
