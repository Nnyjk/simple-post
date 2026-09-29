//! All Tauri commands exposed to the renderer.
//!
//! Each function is a `#[tauri::command]` thin wrapper around the
//! corresponding `crate::db::...` data-access function. Argument shapes
//! are kept aligned with the TS contract in `src/types/domain.ts` and
//! the call sites in `src/lib/tauri*.ts`. Renames on the wire follow
//! Tauri 2 defaults (`camelCase`).
//!
//! Authority split:
//!   * Mutations write through `crate::db::with_tx` so partial state
//!     never escapes.
//!   * Reads go through `crate::db::with_conn` (read-only transaction).
//!   * The `mcp_status` command is the one exception — it pulls from
//!     `crate::daemon_supervisor::McpDaemonHandle`, not the DB. The DTO
//!     shape used by both sides is declared here: see [`McpStatusDto`].
//!
//! Patch types come from `crate::domain` (the canonical place for
//! shared shapes). Create-options and move-targets that don't fit the
//! domain crate stay here as small adapter DTOs.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;
use crate::permissions::{CapabilityPolicy, Permission};

// ---------------------------------------------------------------------------
// Create-options and move-target DTOs. Patch types live in `crate::domain`
// and are pulled in via `use crate::domain::*`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectOpts {
    pub description: Option<String>,
    pub color: Option<String>,
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
    fn into_detail(self, node_id: &str) -> EndpointDetail {
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

// ===========================================================================
// region: projects
// ===========================================================================

#[tauri::command]
pub fn projects_list(state: State<DbPool>) -> Result<Vec<Project>, AppError> {
    crate::db::with_conn(&state, crate::db::projects::list)
}

#[tauri::command]
pub fn projects_get(state: State<DbPool>, id: String) -> Result<Project, AppError> {
    crate::db::with_conn(&state, |c| crate::db::projects::get(c, &id))
}

#[tauri::command]
pub fn projects_create(
    state: State<DbPool>,
    name: String,
    opts: Option<CreateProjectOpts>,
) -> Result<Project, AppError> {
    let opts = opts.unwrap_or_default();
    let now = crate::domain::now_ms();
    let project = Project {
        id: uid(),
        name,
        description: opts.description,
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: opts
            .color
            .unwrap_or_else(crate::domain::default_project_color),
        created_at: now,
        updated_at: now,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::projects::create(c, &project)?;
        Ok(project.clone())
    })
}

#[tauri::command]
pub fn projects_update(
    state: State<DbPool>,
    id: String,
    patch: ProjectPatch,
) -> Result<Project, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::projects::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn projects_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::projects::delete(c, &id))
}

// ===========================================================================
// region: nodes
// ===========================================================================
//
// Replaces `modules_*`, `collections_*` and `endpoints_*` — nineteen Tauri
// commands that existed only because the tree was three tables. The
// handlers below are the Tauri half of the same surface `web_api/routes/
// nodes.rs` serves over REST; both go through `crate::db::nodes` so the
// two transports cannot diverge.
//
// Two commands are not about structure and keep their own handlers:
// `nodes_duplicate` and the endpoint-detail pair.

/// Every node under one project, or the whole database when no root is
/// named.
///
/// `kind` narrows to a single type and `query` runs the substring search
/// (which joins `endpoint_details`, since `url` lives there). `query`
/// wins over `kind` — a search across one type is a filter the caller
/// can apply, and a backend that guessed between them would be guessing.
#[tauri::command]
pub fn nodes_list(
    state: State<DbPool>,
    root_id: Option<String>,
    kind: Option<String>,
    query: Option<String>,
) -> Result<Vec<ResourceNode>, AppError> {
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        return crate::db::with_conn(&state, |c| crate::db::nodes::search(c, &q));
    }
    let kind = match kind.filter(|k| !k.is_empty()) {
        Some(k) => Some(
            ResourceNodeType::parse(&k)
                .ok_or_else(|| AppError::Validation(format!("unknown node type {k:?}")))?,
        ),
        None => None,
    };
    // A type filter without a root would scan the whole database and
    // throw most of it away. Say so rather than quietly returning the
    // tree for a different project than the caller has active.
    if kind.is_some() && root_id.is_none() {
        return Err(AppError::Validation("type filter requires rootId".into()));
    }
    crate::db::with_conn(&state, |c| match (kind, root_id.as_deref()) {
        (Some(k), Some(root)) => crate::db::nodes::list_by_root_and_type(c, root, k),
        (None, Some(root)) => crate::db::nodes::list_by_root(c, root),
        (None, None) => crate::db::nodes::list_all(c),
        // Unreachable: guarded above.
        (Some(_), None) => Ok(Vec::new()),
    })
}

#[tauri::command]
pub fn nodes_get(state: State<DbPool>, id: String) -> Result<ResourceNode, AppError> {
    crate::db::with_conn(&state, |c| crate::db::nodes::get(c, &id))
}

/// Create a node. An endpoint arrives as one call carrying both halves —
/// the structural node and its request configuration — because they are
/// written in a single transaction and a tree where either can be missing
/// is a tree the reader cannot open.
///
/// `root_id` is only required for a module. For a collection or an
/// endpoint it is derived from the parent, so the two can never disagree.
#[tauri::command]
pub fn nodes_create(
    state: State<DbPool>,
    root_id: Option<String>,
    parent_id: Option<String>,
    kind: String,
    name: String,
    description: Option<String>,
    detail: Option<CreateEndpointDetailOpts>,
) -> Result<ResourceNode, AppError> {
    let kind = ResourceNodeType::parse(&kind)
        .ok_or_else(|| AppError::Validation(format!("unknown node type {kind:?}")))?;
    let now = crate::domain::now_ms();
    let mut node = ResourceNode {
        id: uid(),
        root_id: root_id.unwrap_or_default(),
        parent_id,
        kind,
        name,
        description,
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    if node.root_id.is_empty() {
        if let Some(parent) = node.parent_id.clone() {
            node.root_id =
                crate::db::with_conn(&state, |c| crate::db::nodes::root_id_of(c, &parent))?;
        }
    }
    let detail = detail.map(|d| d.into_detail(&node.id));
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::create(c, &node, detail.as_ref())
    })
}

/// Rename / re-describe / re-order one node. Moving is a separate command
/// — see [`nodes_move`] — because it has to keep the type matrix and the
/// subtree's `root_id` honest.
#[tauri::command]
pub fn nodes_update(
    state: State<DbPool>,
    id: String,
    patch: ResourceNodePatch,
) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::update(c, &id, patch.clone()))
}

/// Delete a node and its whole subtree.
#[tauri::command]
pub fn nodes_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::delete(c, &id))
}

/// Move one or many nodes under a new parent at a new sibling position.
///
/// `parent_id` of `None` means "top of the project" and is only legal for
/// a module — the old `parentKind` field is gone because the destination
/// parent's own type answers the question. `node_ids` is in the order the
/// nodes should land, so a multi-select drag keeps the user's pick order.
#[tauri::command]
pub fn nodes_move(
    state: State<DbPool>,
    node_ids: Vec<String>,
    parent_id: Option<String>,
    index: i32,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::nodes::move_many(c, &node_ids, parent_id.as_deref(), index)
    })
}

/// Copy an endpoint under the same parent, immediately after it.
///
/// Endpoints only: a "copy" of a module or collection would be an empty
/// shell, and copying a subtree is a different feature, not a duplicate.
#[tauri::command]
pub fn nodes_duplicate(state: State<DbPool>, id: String) -> Result<ResourceNode, AppError> {
    crate::db::with_tx(&state, |c| crate::db::nodes::duplicate(c, &id))
}

/// The request configuration of one endpoint node.
#[tauri::command]
pub fn endpoint_detail_get(
    state: State<DbPool>,
    node_id: String,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_conn(&state, |c| crate::db::endpoint_details::get(c, &node_id))
}

/// Patch an endpoint's request configuration. Separate from
/// [`nodes_update`] so renaming an endpoint does not rewrite the JSON
/// columns that did not change.
#[tauri::command]
pub fn endpoint_detail_update(
    state: State<DbPool>,
    node_id: String,
    patch: EndpointDetailPatch,
) -> Result<EndpointDetail, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoint_details::update(c, &node_id, patch.clone())
    })
}

// ===========================================================================
// region: environments
// ===========================================================================

#[tauri::command]
pub fn environments_list(
    state: State<DbPool>,
    project_id: Option<String>,
) -> Result<Vec<Environment>, AppError> {
    crate::db::with_conn(&state, |c| match project_id.as_deref() {
        Some(project_id) => crate::db::environments::list_by_project(c, project_id),
        None => crate::db::environments::list_all(c),
    })
}

#[tauri::command]
pub fn environments_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateEnvironmentOpts>,
) -> Result<Environment, AppError> {
    let opts = opts.unwrap_or_default();
    let env = Environment {
        id: uid(),
        project_id,
        name,
        base_urls: opts.base_urls.unwrap_or_default(),
        variables: opts.variables.unwrap_or_default(),
        is_active: opts.make_active.unwrap_or(false),
    };
    crate::db::with_tx(&state, |c| {
        crate::db::environments::create(c, &env)?;
        Ok(env.clone())
    })
}

#[tauri::command]
pub fn environments_update(
    state: State<DbPool>,
    id: String,
    patch: EnvironmentPatch,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn environments_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::delete(c, &id))
}

#[tauri::command]
pub fn environments_set_var(
    state: State<DbPool>,
    env_id: String,
    key: String,
    value: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_var(c, &env_id, &key, value.as_deref())
    })
}

/// Rename an environment variable in one transaction.
///
/// The renderer used to model a rename as `set_var(old, None)` followed
/// by `set_var(new, value)` — two commands, two transactions, with a
/// window in between where the variable does not exist. Anything
/// interrupting that window (crash, or the second command failing)
/// destroyed the variable. One call, one `BEGIN IMMEDIATE`, no window.
#[tauri::command]
pub fn environments_rename_var(
    state: State<DbPool>,
    env_id: String,
    from_key: String,
    to_key: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::rename_var(c, &env_id, &from_key, &to_key)
    })
}

#[tauri::command]
pub fn environments_set_baseurl(
    state: State<DbPool>,
    env_id: String,
    def_id: String,
    url: Option<String>,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_base_url(c, &env_id, &def_id, url.as_deref())
    })
}

#[tauri::command]
pub fn environments_set_active(
    state: State<DbPool>,
    env_id: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::set_active(c, &env_id))
}

// ===========================================================================
// region: base_url_defs
// ===========================================================================

#[tauri::command]
pub fn base_url_defs_list(
    state: State<DbPool>,
    project_id: String,
) -> Result<Vec<BaseUrlDefinition>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::base_url_defs::list_by_project(c, &project_id)
    })
}

#[tauri::command]
pub fn base_url_defs_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateBaseUrlDefOpts>,
) -> Result<BaseUrlDefinition, AppError> {
    let opts = opts.unwrap_or_default();
    let def = BaseUrlDefinition {
        id: uid(),
        name,
        description: opts.description,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::create(c, &def, &project_id)?;
        Ok(def.clone())
    })
}

#[tauri::command]
pub fn base_url_defs_update(
    state: State<DbPool>,
    id: String,
    patch: BaseUrlDefPatch,
) -> Result<BaseUrlDefinition, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::update(c, &id, patch.name.clone(), patch.description.clone())
    })
}

#[tauri::command]
pub fn base_url_defs_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::base_url_defs::delete(c, &id))
}

#[tauri::command]
pub fn base_url_defs_set_default(
    state: State<DbPool>,
    project_id: String,
    def_id: Option<String>,
) -> Result<Project, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::base_url_defs::set_default(c, &project_id, def_id.as_deref())?;
        crate::db::projects::get(c, &project_id)
    })
}

// ===========================================================================
// region: request_settings
// ===========================================================================

/// Preferences KV key under which the singleton `RequestSettings` row is
/// stored. Matches the JSON-encoded value via `serde_json`.
const REQUEST_SETTINGS_KEY: &str = "request_settings";

/// App-wide request defaults (timeout, redirects, max body size). The
/// DB stores these as a JSON-encoded value in the `preferences` table
/// under the `REQUEST_SETTINGS_KEY` row. The `get` command always
/// returns a row, falling back to `RequestSettings::default()` if the
/// row is missing.
#[tauri::command]
pub fn request_settings_get(state: State<DbPool>) -> Result<RequestSettings, AppError> {
    let stored: Option<RequestSettings> = crate::db::with_conn(&state, |c| {
        crate::db::preferences::get(c, REQUEST_SETTINGS_KEY)
    })?;
    Ok(stored.unwrap_or_default())
}

#[tauri::command]
pub fn request_settings_update(
    state: State<DbPool>,
    patch: RequestSettingsPatch,
) -> Result<RequestSettings, AppError> {
    crate::db::with_tx(&state, |c| {
        let mut settings: RequestSettings =
            crate::db::preferences::get(c, REQUEST_SETTINGS_KEY)?.unwrap_or_default();
        if let Some(t) = patch.timeout_ms {
            settings.timeout_ms = t;
        }
        if let Some(f) = patch.follow_redirects {
            settings.follow_redirects = f;
        }
        if let Some(m) = patch.max_response_size_kb {
            settings.max_response_size_kb = m;
        }
        crate::db::preferences::set(c, REQUEST_SETTINGS_KEY, &settings)?;
        Ok(settings)
    })
}

// ===========================================================================
// region: db status
// ===========================================================================

/// Unix-ms timestamp of the last destructive schema rebuild, or `None`
/// when this workspace has never been reset.
///
/// Written by [`crate::db::init`] only when it had to replace a
/// `store.db` written by an older layout — there is no migration path,
/// so the previous file is dropped rather than upgraded. The renderer
/// reads this once during bootstrap and announces the reset.
///
/// Without it the user opens the app to an empty tree with no
/// explanation, which is indistinguishable from never having had any
/// projects — and they have no way to tell data loss from an empty
/// workspace.
#[tauri::command]
pub fn db_recreated_at(state: State<DbPool>) -> Result<Option<i64>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::preferences::get(c, crate::db::preferences::keys::DB_RECREATED_AT)
    })
}

// ===========================================================================
// region: permissions
// ===========================================================================

#[tauri::command]
pub fn permissions_get(state: State<DbPool>, project_id: String) -> Result<Permission, AppError> {
    crate::db::with_conn(&state, |c| crate::db::permissions::get(c, &project_id))
}

#[tauri::command]
pub fn permissions_set(
    state: State<DbPool>,
    project_id: String,
    permission: Permission,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::permissions::set(c, &project_id, permission)
    })
}

// ===========================================================================
// region: mcp_status
// ===========================================================================

/// Snapshot of the MCP daemon. Reads from the in-memory handle held
/// in Tauri state — does NOT touch the DB. The handle's `snapshot()`
/// must return [`McpStatusDto`].
#[tauri::command]
pub fn mcp_status(
    state: State<crate::daemon_supervisor::McpDaemonHandle>,
) -> Result<McpStatusDto, AppError> {
    Ok(state.snapshot())
}

/// Absolute path of the `simple-post-mcp` binary that AI clients
/// (Claude Desktop, Cursor, …) launch in their `mcpServers` config.
///
/// The renderer surfaces this string verbatim in the Connection panel's
/// "copy proxy path" button so the user can paste it into their client.
/// We resolve from `current_exe()` (the running Tauri process) and
/// substitute the trailing `simple-post(.exe)` segment with
/// `simple-post-mcp(.exe)` — the two binaries ship in the same
/// workspace `target/<profile>/` directory after `cargo build
/// --workspace`.
///
/// In a development checkout the simple-post Tauri binary lives at
/// `src-tauri/target/<profile>/simple-post(.exe)` while simple-post-mcp
/// is at the workspace root `target/<profile>/simple-post-mcp(.exe)`;
/// this command returns the same directory as `current_exe` so the
/// hint is correct for installed / packaged builds. There is no
/// npm-wrapped path, so this directory is the only one to point at.
///
/// Returns `"<unresolved current_exe>"` when the lookup fails for any
/// reason; the renderer treats that string as a non-actionable
/// placeholder rather than blocking the UI.
#[tauri::command]
pub fn mcp_daemon_proxy_path() -> String {
    use std::path::PathBuf;

    fn build(_exe_name: &str, proxy_name: &str) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let parent = exe.parent()?;
        Some(parent.join(proxy_name))
    }

    let path = if cfg!(windows) {
        build("simple-post.exe", "simple-post-mcp.exe")
    } else {
        build("simple-post", "simple-post-mcp")
    };

    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unresolved current_exe>".to_string())
}

/// Restart the embedded MCP daemon. Idempotent at the user-action
/// level — the panel disables the button while a restart is in flight
/// so two rapid clicks only result in one restart.
///
/// We swallow `AppError` into `{ ok: false, pid: null, port: null }`
/// rather than propagating it. The renderer's restart action already
/// has its own try/catch that flips the badge to `crashed` on
/// failure, so a uniform `{ ok: bool }` envelope keeps the call site
/// shape identical to the old `Promise<void>` style.
#[tauri::command]
pub async fn mcp_daemon_restart(
    state: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
) -> Result<RestartResult, AppError> {
    match state.restart(&app_handle).await {
        Ok(snapshot) => Ok(RestartResult {
            ok: true,
            pid: snapshot.pid,
            port: snapshot.port,
        }),
        Err(e) => {
            eprintln!("[mcp_daemon_restart] failed: {e:?}");
            Ok(RestartResult {
                ok: false,
                pid: None,
                port: None,
            })
        }
    }
}

// ===========================================================================
// region: mcp_auto_start (preference flag)
// ===========================================================================

/// Read the persisted `mcp.auto_start` preference. The value defaults
/// to `false` (MCP daemon does NOT auto-start at launch) — the
/// rationale lives on `db::preferences::keys::MCP_AUTO_START`.
///
/// Read directly from the DB rather than via the in-memory store, so
/// the value is always the persisted truth even if the user toggled it
/// in another window since the app booted.
#[tauri::command]
pub fn mcp_auto_start_get(state: State<'_, DbPool>) -> Result<bool, AppError> {
    crate::db::with_conn(&state, crate::db::preferences::mcp_auto_start)
}

/// Persist the `mcp.auto_start` preference and immediately try to act
/// on it:
///   - `value == true`  → `ensure_started()` (idempotent; OK if already running)
///   - `value == false` → leave the daemon as-is; the user can stop it
///                          from the panel via the existing restart button
///
/// We do **not** shut the daemon down when the user disables
/// auto-start mid-session — `auto_start` is about whether the daemon
/// starts on launch, not a runtime kill switch. Stopping mid-flight is
/// a separate user action (the "重启/停止" button in the panel).
#[tauri::command]
pub async fn mcp_auto_start_set(
    state: State<'_, DbPool>,
    daemon: State<'_, crate::daemon_supervisor::McpDaemonHandle>,
    app_handle: tauri::AppHandle,
    value: bool,
) -> Result<bool, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::preferences::set(c, crate::db::preferences::keys::MCP_AUTO_START, &value)
    })?;
    if value {
        // Best-effort start; surface the error so the UI can flip the
        // toggle back if launch failed.
        daemon.ensure_started(&app_handle).await?;
    }
    Ok(value)
}

// ===========================================================================
// Helpers
// ===========================================================================
//
// `uid` and the default project color live in `crate::domain` (re-exported
// from `simple-post-core::domain`). The `db` crate also re-uses them, so
// keeping a single definition avoids drift between REST, Tauri command,
// and SQLite test paths. The local helpers below used to live here before
// commit 3 extracted the shared low-level layer.

// =========================================================================
// MCP capability policy (commit 1: feat(mcp): three-tier capability policy)
// =========================================================================
//
// Three Tauri commands + matching REST endpoints (see `web_api.rs`).
// The capability policy is the **global** lever that filters `tools/list`
// for the AI; per-project permissions in the `permissions` table still
// gate individual mutations. Both layers fail closed, and an unregistered
// tool is treated as unclassified rather than permitted.

/// 返回当前全局能力策略，形状与 Web REST 完全一致。
#[tauri::command]
pub async fn mcp_capability_get(
    pool: tauri::State<'_, DbPool>,
) -> Result<McpCapabilityDto, AppError> {
    let policy = crate::db::with_conn(pool.inner(), crate::db::preferences::mcp_policy)?;
    Ok(McpCapabilityDto { policy })
}

/// 更新全局能力策略并返回与 Web REST 相同的对象。
#[tauri::command]
pub async fn mcp_capability_set(
    policy: String,
    pool: tauri::State<'_, DbPool>,
) -> Result<McpCapabilityDto, AppError> {
    let parsed = CapabilityPolicy::parse(&policy).ok_or_else(|| {
        AppError::Validation(format!(
            "unknown mcp policy '{policy}' (expected read_only | safe_write | full_access)"
        ))
    })?;
    crate::db::with_conn(pool.inner(), |c| {
        crate::db::preferences::set_mcp_policy(c, parsed)
    })?;
    Ok(McpCapabilityDto { policy: parsed })
}
/// 返回当前能力策略下可见的工具名称。
#[tauri::command]
pub async fn mcp_capability_list_tools(
    pool: tauri::State<'_, DbPool>,
) -> Result<Vec<String>, AppError> {
    use std::sync::Arc;
    use std::time::Instant;
    let state = crate::mcp::AppState {
        pool: pool.inner().clone(),
        started_at: Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    };
    Ok(crate::mcp::tool_descriptors(&state)
        .await
        .into_iter()
        .map(|t| t.name)
        .collect())
}

// ===========================================================================
// Endpoint-scoped send — the one place a configured endpoint becomes an
// outbound HTTP request.
// ===========================================================================

/// Per-call overrides the renderer may have on screen but not yet saved.
///
/// Mirrors `simple_post_mcp::variable_fill::EndpointOverrides`; the UI's
/// base-URL picker is ephemeral, so the selection has to travel with the
/// request rather than being read back off the stored endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSendOverrides {
    pub url: Option<String>,
    pub method: Option<HttpMethod>,
    /// Which environment supplies the `{{var}}` pool and `base_urls`.
    ///
    /// The renderer's "ENV" pill writes `activeEnvironmentId` in Zustand
    /// only — `setEnvironmentActive`, which would persist `is_active`,
    /// has no caller anywhere in `src/`. So the database's active flag
    /// stays whatever the seed set (`env_dev` for the shipped fixtures)
    /// and never follows the user. Without this field, picking `staging`
    /// and pressing Send would show staging's variables in the URL bar
    /// while the request went out carrying the dev token.
    pub environment_id: Option<String>,
    pub headers: Option<Vec<KeyValue>>,
    pub params: Option<Vec<KeyValue>>,
    pub body: Option<RequestBody>,
    /// Which base-URL definition to resolve against, instead of the one
    /// persisted on the endpoint. `None` keeps the endpoint's own value;
    /// `Some("")` means "no base URL".
    pub base_url_definition_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSendSettings {
    pub timeout_ms: Option<u64>,
    pub follow_redirects: Option<bool>,
    pub max_response_size_kb: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSendArgs {
    pub endpoint_id: String,
    #[serde(default)]
    pub overrides: Option<EndpointSendOverrides>,
    #[serde(default)]
    pub settings: Option<EndpointSendSettings>,
}

impl From<EndpointSendOverrides> for simple_post_mcp::variable_fill::EndpointOverrides {
    fn from(o: EndpointSendOverrides) -> Self {
        Self {
            url: o.url,
            method: o.method,
            headers: o.headers,
            params: o.params,
            body: o.body,
            base_url_definition_id: o.base_url_definition_id,
        }
    }
}

/// Resolve a stored endpoint and execute it, without the caller ever
/// naming a URL.
///
/// This is the counterpart of the old `/api/http/send` shape, where the
/// browser posted a finished URL. There, any authenticated caller could
/// name any target, which made the `serve-web` / Docker deployment an
/// open proxy (cloud metadata at 169.254.169.254 included). Here the URL
/// is derived from rows the user themselves configured, so the request
/// surface collapses to "send endpoint X".
///
/// Both distribution shapes call this, so URL composition lives in Rust
/// exactly once: the TS `buildFetchInit` path is now only used for the
/// cURL preview and the on-screen resolved URL.
pub async fn send_endpoint_by_id(
    pool: &DbPool,
    args: &EndpointSendArgs,
) -> Result<crate::http_executor::HttpResult, AppError> {
    let input = resolve_send_input(pool, args)?;
    Ok(crate::http_executor::execute(&input).await)
}

/// The non-I/O half of [`send_endpoint_by_id`]: turn stored rows plus the
/// caller's unsaved draft into a concrete request.
///
/// Split out so the URL composition can be asserted without dialling
/// anything, mirroring the "http_executor is pure I/O" split the rest of
/// the crate uses. Returns the request the backend will actually issue.
pub fn resolve_send_input(
    pool: &DbPool,
    args: &EndpointSendArgs,
) -> Result<crate::http_executor::HttpRequestInput, AppError> {
    let endpoint: EndpointDetail = crate::db::with_conn(pool, |c| {
        crate::db::endpoint_details::get(c, &args.endpoint_id)
    })?;
    // One lookup. The old layout walked endpoint → collection → module →
    // project; `root_id` is denormalised onto every node precisely so
    // resolving the owning project — the first step of the per-project
    // permission gate — is a primary-key hit.
    let project_id =
        crate::db::with_conn(pool, |c| crate::db::nodes::root_id_of(c, &endpoint.node_id))?;

    // Env selection must mirror the renderer's rule exactly, or Send
    // silently produces a base-less URL or — worse — the wrong
    // credentials.
    //
    //   1. what the caller is looking at, sent as `overrides.environmentId`
    //   2. the endpoint's own `environment_id`
    //   3. the project's active env, else its first env
    //
    // Step 1 is load-bearing and is NOT redundant with step 3: the "ENV"
    // pill is a pure Zustand write (`setActiveEnvironment`), and
    // `setEnvironmentActive` — the only thing that would flip `is_active`
    // in the database — has no caller in `src/`. Relying on step 3 alone
    // means the user picks `staging`, the URL bar shows staging's
    // variables, and the request goes out with the dev token.
    //
    // A dangling env id (deleted while the tab was open) is still
    // tolerated: the request just runs without variable substitution.
    let requested_env = args
        .overrides
        .as_ref()
        .and_then(|o| o.environment_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| endpoint.environment_id.clone())
        .filter(|id| !id.is_empty());
    let env: Option<Environment> = match requested_env.as_deref() {
        Some(env_id) => {
            crate::db::with_conn(pool, |c| crate::db::environments::get(c, env_id)).ok()
        }
        None => {
            // Active env of the project, else its first env — the same
            // fallback `bootstrap-slice` uses to seed `activeEnvironmentId`.
            let candidates = crate::db::with_conn(pool, |c| {
                crate::db::environments::list_by_project(c, &project_id)
            })
            .unwrap_or_default();
            candidates
                .iter()
                .find(|e| e.is_active)
                .or_else(|| candidates.first())
                .cloned()
        }
    };
    let project: Option<Project> = if project_id.is_empty() {
        None
    } else {
        crate::db::with_conn(pool, |c| crate::db::projects::get(c, &project_id)).ok()
    };

    // A missing project means the endpoint has no definition list and no
    // default, which is exactly what an empty `Project` encodes. Built as
    // a local rather than a `const` so the borrow outlives this call.
    let empty_project = Project {
        id: String::new(),
        name: String::new(),
        description: None,
        base_url: None,
        base_url_definitions: Vec::new(),
        default_base_url_definition_id: None,
        color: String::new(),
        created_at: 0,
        updated_at: 0,
    };
    let project_ref: &Project = project.as_ref().unwrap_or(&empty_project);

    let overrides = args.overrides.clone().map(Into::into);
    let resolved = simple_post_mcp::variable_fill::resolve_endpoint(
        &endpoint,
        env.as_ref(),
        project_ref,
        overrides.as_ref(),
    )?;

    let s = args.settings.clone().unwrap_or_default();
    Ok(crate::http_executor::HttpRequestInput {
        method: resolved.method.clone(),
        url: resolved.url.clone(),
        headers: resolved.headers.clone(),
        body: resolved.body.clone(),
        timeout_ms: s.timeout_ms.unwrap_or(30_000),
        follow_redirects: s.follow_redirects.unwrap_or(true),
        max_response_size_kb: s.max_response_size_kb.unwrap_or(10_240),
    })
}

#[cfg(test)]
mod endpoint_send_tests {
    use super::*;

    /// DB in a temp dir, isolated per test, carrying the minimal fixture
    /// chain the send path exercises:
    /// `p_demo → m_auth → c_profile → e_user_get`, plus the `def_default`
    /// base URL definition and the dev/staging environments. The atomic
    /// counter disambiguates when several tests construct their pool
    /// inside the same millisecond (cargo runs tests in parallel).
    ///
    /// These rows are built here through the public CRUD API rather than
    /// a seed file: Simple Post ships no seed data, and
    /// `simple_post_core::db::test_support` is `pub(crate)`, so a
    /// dependent crate has no way to reach it. Each test therefore states
    /// exactly the rows it depends on.
    fn seeded_pool(tag: &str) -> (DbPool, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "simple-post-endpoint-send-{}-{}-{}",
            std::process::id(),
            tag,
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let pool = crate::db::init(&dir).unwrap();

        use crate::domain::{
            AuthConfig, AuthType, BaseUrlDefinition, EndpointDetail, Environment, HttpMethod,
            KeyValue, Project, RequestBody, ResourceNode, ResourceNodeType,
        };

        let node = |id: &str, kind: ResourceNodeType, parent: Option<&str>| ResourceNode {
            id: id.into(),
            root_id: "p_demo".into(),
            parent_id: parent.map(String::from),
            kind,
            name: "n".into(),
            description: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        };

        crate::db::with_tx(&pool, |c| {
            crate::db::projects::create(
                c,
                &Project {
                    id: "p_demo".into(),
                    name: "Demo API".into(),
                    description: None,
                    default_base_url_definition_id: Some("def_default".into()),
                    base_url_definitions: vec![],
                    base_url: None,
                    color: "#60a5fa".into(),
                    created_at: 1,
                    updated_at: 1,
                },
            )?;
            crate::db::base_url_defs::create(
                c,
                &BaseUrlDefinition {
                    id: "def_default".into(),
                    name: "default".into(),
                    description: None,
                },
                "p_demo",
            )?;
            crate::db::nodes::create(c, &node("m_auth", ResourceNodeType::Module, None), None)?;
            crate::db::nodes::create(
                c,
                &node("c_profile", ResourceNodeType::Collection, Some("m_auth")),
                None,
            )?;
            // The endpoint's two halves land together — a node without
            // its detail row is a tree the reader cannot open.
            crate::db::nodes::create(
                c,
                &node("e_user_get", ResourceNodeType::Endpoint, Some("c_profile")),
                Some(&EndpointDetail {
                    node_id: "e_user_get".into(),
                    method: HttpMethod::Get,
                    url: "/users/{{userId}}".into(),
                    environment_id: None,
                    base_url_definition_id: None,
                    base_url_override: None,
                    params: vec![KeyValue {
                        id: "p1".into(),
                        key: "include".into(),
                        value: "profile,permissions".into(),
                        enabled: true,
                        description: None,
                    }],
                    headers: vec![],
                    body: RequestBody::default(),
                    auth: AuthConfig {
                        kind: AuthType::Bearer,
                        bearer: Some("{{token}}".into()),
                        basic: None,
                        apikey: None,
                    },
                    docs: None,
                    notes: None,
                    tags: vec![],
                }),
            )?;

            // Both environments deliberately share one base URL and differ
            // only in `variables`, so an assertion on the resolved bearer
            // token is really testing *which* environment was selected.
            let base_urls = || -> std::collections::HashMap<String, String> {
                [(
                    "def_default".to_string(),
                    "http://localhost:4000/demo".to_string(),
                )]
                .into()
            };
            for (env_id, name, token, is_active) in [
                ("env_dev", "dev", "eyJhbGciOiJIUzI1NiJ9.dev", true),
                (
                    "env_staging",
                    "staging",
                    "eyJhbGciOiJIUzI1NiJ9.staging",
                    false,
                ),
            ] {
                crate::db::environments::create(
                    c,
                    &Environment {
                        id: env_id.into(),
                        project_id: "p_demo".into(),
                        name: name.into(),
                        base_urls: base_urls(),
                        variables: [("token".to_string(), token.to_string())].into(),
                        is_active,
                    },
                )?;
            }
            Ok(())
        })
        .expect("seed fixture chain");

        (pool, dir)
    }

    /// Regression net for a real bug: `resolve_send_input` read only
    /// `endpoint.environment_id`, so an endpoint without one resolved to a
    /// base-less path and Send failed with
    /// `invalid URL '/users/{{userId}}': relative URL without a base`.
    /// The renderer's rule is "endpoint's env, else the project's active
    /// env"; this pins the second half of that rule.
    ///
    /// `e_user_get` carries an enabled `include` param, which
    /// `resolve_endpoint` appends — so the expected URL is base + path +
    /// query. It deliberately has no `environment_id` of its own.
    #[test]
    fn resolves_base_url_from_the_projects_active_env() {
        let (pool, _dir) = seeded_pool("active-env");

        let args = EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: None,
            settings: None,
        };
        let input = resolve_send_input(&pool, &args).expect("must resolve");

        // env_dev is the active env and its baseUrls["def_default"] is
        // http://localhost:4000/demo by construction above.
        assert_eq!(
            input.url, "http://localhost:4000/demo/users/{{userId}}?include=profile%2Cpermissions",
            "base URL + path + params must be composed server-side"
        );
        assert_eq!(input.method, "GET");
    }

    /// The draft path from the URL input travels as `overrides.url` and
    /// replaces the stored path, while the base URL and params still come
    /// from the database.
    #[test]
    fn draft_path_override_keeps_the_stored_base_url() {
        let (pool, _dir) = seeded_pool("draft");
        let args = EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: Some(EndpointSendOverrides {
                url: Some("/users/42".into()),
                ..Default::default()
            }),
            settings: Some(EndpointSendSettings {
                timeout_ms: Some(1234),
                follow_redirects: Some(false),
                max_response_size_kb: Some(7),
            }),
        };
        let input = resolve_send_input(&pool, &args).expect("must resolve");
        assert_eq!(
            input.url,
            "http://localhost:4000/demo/users/42?include=profile%2Cpermissions"
        );
        assert_eq!(input.timeout_ms, 1234);
        assert!(!input.follow_redirects);
        assert_eq!(input.max_response_size_kb, 7);
    }

    /// A draft that already carries a scheme wins outright — the join
    /// helper treats an absolute path as self-contained.
    #[test]
    fn absolute_draft_url_wins_over_the_base_url() {
        let (pool, _dir) = seeded_pool("absolute");
        let args = EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: Some(EndpointSendOverrides {
                url: Some("https://other.example.com/x".into()),
                ..Default::default()
            }),
            settings: None,
        };
        let input = resolve_send_input(&pool, &args).expect("must resolve");
        assert_eq!(
            input.url,
            "https://other.example.com/x?include=profile%2Cpermissions"
        );
    }

    /// Settings default sanely when the renderer omits them.
    #[test]
    fn settings_have_documented_defaults() {
        let (pool, _dir) = seeded_pool("defaults");
        let args = EndpointSendArgs {
            endpoint_id: "e_user_get".into(),
            overrides: None,
            settings: None,
        };
        let input = resolve_send_input(&pool, &args).expect("must resolve");
        assert_eq!(input.timeout_ms, 30_000);
        assert!(input.follow_redirects);
        assert_eq!(input.max_response_size_kb, 10_240);
    }

    /// The caller's environment selection wins over the database's
    /// `is_active` flag.
    ///
    /// Regression net for a silent wrong-credential request. The "ENV"
    /// pill writes `activeEnvironmentId` in Zustand only —
    /// `setEnvironmentActive`, the only thing that would flip `is_active`,
    /// has no caller in `src/`. So the DB always says `env_dev` while the
    /// user may be looking at `env_staging`.
    ///
    /// The dev and staging environments deliberately share one base URL
    /// and differ only in `variables`, so the assertion is on the
    /// resolved bearer token, not the URL — that is the credential which
    /// would have gone out wrong.
    #[test]
    fn caller_environment_selection_wins_over_the_active_flag() {
        let (pool, _dir) = seeded_pool("caller-env");

        let token_of = |input: &crate::http_executor::HttpRequestInput| -> String {
            input
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };

        // Sanity: the fixture chain really does have env_dev active.
        let envs = crate::db::with_conn(&pool, |c| {
            crate::db::environments::list_by_project(c, "p_demo")
        })
        .unwrap();
        assert_eq!(
            envs.iter().find(|e| e.is_active).map(|e| e.id.as_str()),
            Some("env_dev")
        );

        // No override: falls back to the project's active env, i.e. dev.
        let default = resolve_send_input(
            &pool,
            &EndpointSendArgs {
                endpoint_id: "e_user_get".into(),
                overrides: None,
                settings: None,
            },
        )
        .unwrap();
        assert!(
            token_of(&default).ends_with(".dev"),
            "unselected should use the active env (dev), got {:?}",
            token_of(&default)
        );

        // Override with staging: its token must win.
        let staging = resolve_send_input(
            &pool,
            &EndpointSendArgs {
                endpoint_id: "e_user_get".into(),
                overrides: Some(EndpointSendOverrides {
                    environment_id: Some("env_staging".into()),
                    ..Default::default()
                }),
                settings: None,
            },
        )
        .unwrap();
        assert!(
            token_of(&staging).ends_with(".staging"),
            "the selected environment must supply the credential, got {:?}",
            token_of(&staging)
        );
    }

    /// An unknown endpoint fails before any network I/O.
    #[test]
    fn unknown_endpoint_is_not_found() {
        let (pool, _dir) = seeded_pool("missing");
        let args = EndpointSendArgs {
            endpoint_id: "e_nope".into(),
            overrides: None,
            settings: None,
        };
        assert!(matches!(
            resolve_send_input(&pool, &args),
            Err(AppError::NotFound(_))
        ));
    }
}
