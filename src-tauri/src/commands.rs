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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModuleOpts {
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCollectionOpts {
    pub description: Option<String>,
    pub parent_collection_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEndpointOpts {
    pub description: Option<String>,
    pub method: Option<HttpMethod>,
    pub url: Option<String>,
    pub environment_id: Option<String>,
    pub base_url_definition_id: Option<String>,
    pub body: Option<RequestBody>,
    pub auth: Option<AuthConfig>,
    pub tags: Option<Vec<String>>,
    pub headers: Option<Vec<KeyValue>>,
    pub params: Option<Vec<KeyValue>>,
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

/// Target for moving a collection between siblings / parents. `parentKind`
/// is one of `"module"` (top-level under a module) or `"collection"`
/// (nested under another collection).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMoveTarget {
    /// `"module"` | `"collection"`
    pub parent_kind: String,
    pub parent_id: String,
    pub index: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointMoveTarget {
    pub collection_id: String,
    pub index: i32,
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
// region: modules
// ===========================================================================

#[tauri::command]
pub fn modules_list(
    state: State<DbPool>,
    project_id: Option<String>,
) -> Result<Vec<Module>, AppError> {
    crate::db::with_conn(&state, |c| match project_id.as_deref() {
        Some(project_id) => crate::db::modules::list_by_project(c, project_id),
        None => crate::db::modules::list_all(c),
    })
}

#[tauri::command]
pub fn modules_create(
    state: State<DbPool>,
    project_id: String,
    name: String,
    opts: Option<CreateModuleOpts>,
) -> Result<Module, AppError> {
    let opts = opts.unwrap_or_default();
    let module = Module {
        id: uid(),
        project_id,
        name,
        description: opts.description,
        sort_order: 0,
        expanded: None,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::modules::create(c, &module)?;
        Ok(module.clone())
    })
}

#[tauri::command]
pub fn modules_update(
    state: State<DbPool>,
    id: String,
    patch: ModulePatch,
) -> Result<Module, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::modules::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn modules_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::modules::delete(c, &id))
}

/// Reorder a module within its project. `target_index` is the
/// zero-based position in the post-move list.
#[tauri::command]
pub fn modules_move(
    state: State<DbPool>,
    module_id: String,
    target_index: i32,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::modules::r#move(c, &module_id, target_index)
    })
}

// ===========================================================================
// region: collections
// ===========================================================================

#[tauri::command]
pub fn collections_list(
    state: State<DbPool>,
    module_id: Option<String>,
) -> Result<Vec<Collection>, AppError> {
    crate::db::with_conn(&state, |c| match module_id.as_deref() {
        Some(module_id) => crate::db::collections::list_by_module(c, module_id),
        None => crate::db::collections::list_all(c),
    })
}

#[tauri::command]
pub fn collections_create(
    state: State<DbPool>,
    module_id: String,
    name: String,
    opts: Option<CreateCollectionOpts>,
) -> Result<Collection, AppError> {
    let opts = opts.unwrap_or_default();
    let collection = Collection {
        id: uid(),
        module_id,
        parent_collection_id: opts.parent_collection_id,
        name,
        description: opts.description,
        sort_order: 0,
        expanded: None,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::collections::create(c, &collection)?;
        Ok(collection.clone())
    })
}

#[tauri::command]
pub fn collections_update(
    state: State<DbPool>,
    id: String,
    patch: CollectionPatch,
) -> Result<Collection, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::collections::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn collections_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::collections::delete(c, &id))
}

/// Move one or many collections to a new sibling position. The store
/// uses this for both single drags and multi-select reorders; we
/// delegate ordering entirely to `db::collections::move_many` so the
/// DB keeps a contiguous `sort_order`.
#[tauri::command]
pub fn collections_move(
    state: State<DbPool>,
    collection_ids: Vec<String>,
    target: CollectionMoveTarget,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::collections::move_many(
            c,
            &collection_ids,
            &target.parent_kind,
            &target.parent_id,
            target.index,
        )
    })
}

// ===========================================================================
// region: endpoints
// ===========================================================================

#[tauri::command]
pub fn endpoints_list(
    state: State<DbPool>,
    collection_id: Option<String>,
) -> Result<Vec<Endpoint>, AppError> {
    crate::db::with_conn(&state, |c| match collection_id.as_deref() {
        Some(collection_id) => crate::db::endpoints::list_by_collection(c, collection_id),
        None => crate::db::endpoints::list_all(c),
    })
}

#[tauri::command]
pub fn endpoints_get(state: State<DbPool>, id: String) -> Result<Endpoint, AppError> {
    crate::db::with_conn(&state, |c| crate::db::endpoints::get(c, &id))
}

/// Substring search. `query` is the user's text; matches against
/// name / url / description. The DB-layer `search` is more general
/// (also accepts method / tags filters) — the Tauri surface exposes
/// just the substring filter the UI needs.
#[tauri::command]
pub fn endpoints_search(state: State<DbPool>, query: String) -> Result<Vec<Endpoint>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::endpoints::search(c, Some(&query), None, None)
    })
}

#[tauri::command]
pub fn endpoints_create(
    state: State<DbPool>,
    collection_id: String,
    name: String,
    opts: Option<CreateEndpointOpts>,
) -> Result<Endpoint, AppError> {
    let opts = opts.unwrap_or_default();
    let now = crate::domain::now_ms();
    let endpoint = Endpoint {
        id: uid(),
        collection_id,
        name,
        description: opts.description,
        method: opts.method.unwrap_or_default(),
        url: opts.url.unwrap_or_default(),
        environment_id: opts.environment_id,
        base_url_definition_id: opts.base_url_definition_id,
        base_url_override: None,
        params: opts.params.unwrap_or_default(),
        headers: opts.headers.unwrap_or_default(),
        body: opts.body.unwrap_or_default(),
        auth: opts.auth.unwrap_or_default(),
        docs: None,
        notes: None,
        tags: opts.tags.unwrap_or_default(),
        sort_order: 0,
        created_at: now,
        updated_at: now,
    };
    crate::db::with_tx(&state, |c| {
        crate::db::endpoints::create(c, &endpoint)?;
        Ok(endpoint.clone())
    })
}

#[tauri::command]
pub fn endpoints_update(
    state: State<DbPool>,
    id: String,
    patch: EndpointPatch,
) -> Result<Endpoint, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoints::update(c, &id, patch.clone())
    })
}

#[tauri::command]
pub fn endpoints_delete(state: State<DbPool>, id: String) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::endpoints::delete(c, &id))
}

/// Duplicate an endpoint, returning the new id. The copy lands at the
/// end of the same `collection` (caller reorders if needed).
#[tauri::command]
pub fn endpoints_duplicate(state: State<DbPool>, id: String) -> Result<String, AppError> {
    crate::db::with_tx(&state, |c| crate::db::endpoints::duplicate(c, &id))
}

#[tauri::command]
pub fn endpoints_move(
    state: State<DbPool>,
    endpoint_ids: Vec<String>,
    target: EndpointMoveTarget,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoints::move_many(c, &endpoint_ids, &target.collection_id, target.index)
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
/// hint is correct for installed / packaged builds. Commit 5 will
/// revisit this when the npm-wrapped path becomes the canonical
/// install (it should point users at `npx -y @simple-post/mcp-server`
/// in that case).
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
// region: mcp_auto_start (preference flag, see distribution-modes §5)
// ===========================================================================

/// Read the persisted `mcp.auto_start` preference. The value defaults
/// to `false` (MCP daemon does NOT auto-start at launch) — see
/// `db::preferences::keys::MCP_AUTO_START` and
/// `docs/distribution-modes-2026-09-18.md` §5 for the rationale.
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
// gate individual mutations. The two layers compose — see
// `docs/mcp-permission-policy.md`.

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
    let endpoint: Endpoint =
        crate::db::with_conn(pool, |c| crate::db::endpoints::get(c, &args.endpoint_id))?;
    let project_id = crate::db::endpoints::project_id_of(pool, &endpoint.collection_id)?;

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

    /// Seeded DB in a temp dir, isolated per test. The atomic counter
    /// disambiguates when several tests construct their pool inside the
    /// same millisecond (cargo runs tests in parallel).
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
        (crate::db::init(&dir).unwrap(), dir)
    }

    /// Regression net for a real bug: `resolve_send_input` read only
    /// `endpoint.environment_id`, so every seeded endpoint — all of which
    /// have none — resolved to a base-less path and Send failed with
    /// `invalid URL '/users/{{userId}}': relative URL without a base`.
    /// The renderer's rule is "endpoint's env, else the project's active
    /// env"; this pins the second half of that rule.
    ///
    /// `e_user_get` carries an enabled `include` param, which
    /// `resolve_endpoint` appends — so the expected URL is base + path +
    /// query.
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
        // http://localhost:4000/demo in the fixture.
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
    /// The three seeded environments deliberately share one base URL and
    /// differ only in `variables`, so the assertion is on the resolved
    /// bearer token, not the URL — that is the credential that would have
    /// gone out wrong.
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

        // Sanity: the seed really does have env_dev active.
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
