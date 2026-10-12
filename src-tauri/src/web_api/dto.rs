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
/// Mirrors what every `commands::*_create` does: take the user-facing
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

/// Body for `POST /api/environments/:envId/module-baseurls`. `url = null`
/// clears that one cell. This is the only layer a URL lives in — there is
/// no layer above it and no fallback to another module's value.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetModuleBaseUrlBody {
    pub def_id: String,
    /// The module whose address for this service is being written. The
    /// same service can point at two hosts for two modules, so this is
    /// part of the address rather than a filter.
    pub module_id: String,
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

/// Body for `POST /api/projects/:projectId/baseurls`. The project is in
/// the path because the service list belongs to the project; only the
/// attributes of one service ride in the body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefCreateBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// Body for `POST /api/projects/:projectId/baseurls/default`. `defId = null`
/// takes the project's ★ away entirely — the state a freshly created
/// project is in.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefSetDefaultBody {
    #[serde(default)]
    pub def_id: Option<String>,
}

// ----- resource tree -------------------------------------------------------

/// Body for `POST /api/nodes`.
///
/// An endpoint arrives as one payload carrying both halves rather than
/// two requests: a node without its configuration cannot be opened, and
/// two calls would leave a window where the tree holds an endpoint that
/// errors when selected. `db::nodes::create` enforces the pairing.
#[derive(Debug, Deserialize)]
pub struct NodeCreateBody {
    #[serde(flatten)]
    pub node: NodeCreateNodeBody,
    /// Present for an endpoint, absent otherwise. A mismatch — either
    /// way round — is rejected by `db::nodes`, not here, so the rule
    /// lives in one place.
    ///
    /// `CreateEndpointDetailOpts`, not `EndpointDetail`: every field in
    /// it is optional because an endpoint born from the tree's "new
    /// request" action carries only what the user has typed so far — the
    /// Tauri command has always accepted that shape (see the type's own
    /// doc comment), and deserialising straight into `EndpointDetail`
    /// here instead made `method` — which has no default — a required
    /// field on the REST transport only. The same click therefore created
    /// an endpoint in the desktop app and returned 400 over `serve-web`.
    /// Reusing the command's type is what keeps the two transports from
    /// drifting apart again.
    #[serde(default)]
    pub detail: Option<crate::commands::CreateEndpointDetailOpts>,
}

/// The structural half of a `POST /api/nodes` body.
///
/// Not `ResourceNode`. That is the *stored* row, where `rootId` is always
/// present because the database guarantees it — but a create call has no
/// reason to know the owning project when it names a parent, and the
/// renderer sends `rootId: null` to say exactly that. Deserialising the
/// stored shape made "let the server work it out" the one thing a client
/// could not express (`400 invalid type: null, expected a string`) on
/// this transport alone; `root_id: Option<String>` here is the same
/// contract as the Tauri command's argument of that name.
///
/// `id` is required because `ensure_server_id` fills it before this is
/// parsed, and the timestamps are absent because the server stamps them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCreateNodeBody {
    pub id: String,
    /// `None` / empty means "derive it from `parentId`".
    #[serde(default)]
    pub root_id: Option<String>,
    /// `None` for a module, whose parent is the project.
    #[serde(default)]
    pub parent_id: Option<String>,
    pub kind: crate::domain::ResourceNodeType,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

impl NodeCreateNodeBody {
    /// The row `db::nodes::create` will write. `root_id` is threaded in
    /// rather than read from the body so a caller cannot pass a node and
    /// a project that disagree about who owns it.
    pub(crate) fn into_node(self, root_id: &str) -> crate::domain::ResourceNode {
        let now = crate::domain::now_ms();
        crate::domain::ResourceNode {
            id: self.id,
            root_id: root_id.to_string(),
            parent_id: self.parent_id,
            kind: self.kind,
            name: self.name,
            description: self.description,
            sort_order: self.sort_order.unwrap_or_default(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Body for `POST /api/nodes/move`.
///
/// `nodeIds` is in the order the nodes should appear at the
/// destination, so a multi-select drag keeps the user's pick order.
///
/// `parentId` replaces the old `parentKind` + `parentId` pair: which
/// table the destination lived in is now a question about the parent's
/// `type`, and `db::nodes::move_many` answers it. `null` means the top
/// of the project and is legal only for a module.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMoveBody {
    pub node_ids: Vec<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub index: i32,
}

#[cfg(test)]
#[path = "dto_test.rs"]
mod tests;
