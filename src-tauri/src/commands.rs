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

/// Set / clear a single variable on an environment. `value = None`
/// removes the key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetVarArgs {
    pub env_id: String,
    pub key: String,
    pub value: Option<String>,
}

/// Set / clear a single baseUrl slot on an environment.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSetBaseUrlArgs {
    pub env_id: String,
    pub def_id: String,
    pub url: Option<String>,
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
    crate::db::with_conn(&state, |c| crate::db::projects::list(c))
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
        color: opts.color.unwrap_or_else(default_color),
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
    crate::db::with_tx(&state, |c| crate::db::projects::update(c, &id, patch.clone()))
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
    project_id: String,
) -> Result<Vec<Module>, AppError> {
    crate::db::with_conn(&state, |c| crate::db::modules::list_by_project(c, &project_id))
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
    crate::db::with_tx(&state, |c| crate::db::modules::update(c, &id, patch.clone()))
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
        Some(mid) => crate::db::collections::list_by_module(c, mid),
        // No `list_all` overload in the current db module — when the
        // caller doesn't pin a module, return empty rather than
        // inventing a new SQL path here. The renderer normally knows
        // which module it's rendering.
        None => Ok(Vec::new()),
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
    crate::db::with_tx(&state, |c| crate::db::collections::update(c, &id, patch.clone()))
}

#[tauri::command]
pub fn collections_delete(
    state: State<DbPool>,
    id: String,
) -> Result<(), AppError> {
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
        Some(cid) => crate::db::endpoints::list_by_collection(c, cid),
        // No `list_all` overload in the current db module — see the
        // matching note on `collections_list` above.
        None => Ok(Vec::new()),
    })
}

#[tauri::command]
pub fn endpoints_get(
    state: State<DbPool>,
    id: String,
) -> Result<Endpoint, AppError> {
    crate::db::with_conn(&state, |c| crate::db::endpoints::get(c, &id))
}

/// Substring search. `query` is the user's text; matches against
/// name / url / description. The DB-layer `search` is more general
/// (also accepts method / tags filters) — the Tauri surface exposes
/// just the substring filter the UI needs.
#[tauri::command]
pub fn endpoints_search(
    state: State<DbPool>,
    query: String,
) -> Result<Vec<Endpoint>, AppError> {
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
    crate::db::with_tx(&state, |c| crate::db::endpoints::update(c, &id, patch.clone()))
}

#[tauri::command]
pub fn endpoints_delete(
    state: State<DbPool>,
    id: String,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::endpoints::delete(c, &id))
}

/// Duplicate an endpoint, returning the new id. The copy lands at the
/// end of the same `collection` (caller reorders if needed).
#[tauri::command]
pub fn endpoints_duplicate(
    state: State<DbPool>,
    id: String,
) -> Result<String, AppError> {
    crate::db::with_tx(&state, |c| crate::db::endpoints::duplicate(c, &id))
}

#[tauri::command]
pub fn endpoints_move(
    state: State<DbPool>,
    endpoint_ids: Vec<String>,
    target: EndpointMoveTarget,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::endpoints::move_many(
            c,
            &endpoint_ids,
            &target.collection_id,
            target.index,
        )
    })
}

// ===========================================================================
// region: environments
// ===========================================================================

#[tauri::command]
pub fn environments_list(
    state: State<DbPool>,
    project_id: String,
) -> Result<Vec<Environment>, AppError> {
    crate::db::with_conn(&state, |c| {
        crate::db::environments::list_by_project(c, &project_id)
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
    crate::db::with_tx(&state, |c| crate::db::environments::update(c, &id, patch.clone()))
}

#[tauri::command]
pub fn environments_delete(
    state: State<DbPool>,
    id: String,
) -> Result<(), AppError> {
    crate::db::with_tx(&state, |c| crate::db::environments::delete(c, &id))
}

#[tauri::command]
pub fn environments_set_var(
    state: State<DbPool>,
    args: EnvSetVarArgs,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_var(c, &args.env_id, &args.key, args.value.as_deref())
    })
}

#[tauri::command]
pub fn environments_set_baseurl(
    state: State<DbPool>,
    args: EnvSetBaseUrlArgs,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_base_url(
            c,
            &args.env_id,
            &args.def_id,
            args.url.as_deref(),
        )
    })
}

#[tauri::command]
pub fn environments_set_active(
    state: State<DbPool>,
    env_id: String,
) -> Result<Environment, AppError> {
    crate::db::with_tx(&state, |c| {
        crate::db::environments::set_active(c, &env_id)
    })
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
pub fn base_url_defs_delete(
    state: State<DbPool>,
    id: String,
) -> Result<(), AppError> {
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
pub fn request_settings_get(
    state: State<DbPool>,
) -> Result<RequestSettings, AppError> {
    let stored: Option<RequestSettings> =
        crate::db::with_conn(&state, |c| {
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
        let mut settings: RequestSettings = crate::db::preferences::get(c, REQUEST_SETTINGS_KEY)?
            .unwrap_or_default();
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
pub fn permissions_get(
    state: State<DbPool>,
    project_id: String,
) -> Result<Permission, AppError> {
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

/// Absolute path of the `simple-post-mcp-proxy` binary that AI clients
/// (Claude Desktop, Cursor, …) launch in their `mcpServers` config.
///
/// The renderer surfaces this string verbatim in the Connection panel's
/// "copy proxy path" button so the user can paste it into their client.
/// We resolve from `current_exe()` (the running Tauri process) and
/// substitute the trailing `simple-post(.exe)` segment with
/// `simple-post-mcp-proxy(.exe)` — the two binaries always ship in the
/// same directory.
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
        build("simple-post.exe", "simple-post-mcp-proxy.exe")
    } else {
        build("simple-post", "simple-post-mcp-proxy")
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
pub fn mcp_auto_start_get(
    state: State<'_, DbPool>,
) -> Result<bool, AppError> {
    crate::db::with_conn(&state, |c| crate::db::preferences::mcp_auto_start(c))
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

pub(crate) fn uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    // Per-process monotonic counter — guarantees uniqueness within the
    // running process even when the wall clock hasn't advanced.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sp_{:x}{:x}", nanos, n)
}

pub(crate) fn default_color() -> String {
    "#3b82f6".to_string()
}

// =========================================================================
// MCP capability policy (commit 1: feat(mcp): three-tier capability policy)
// =========================================================================
//
// Three Tauri commands + matching REST endpoints (see `web_api.rs`).
// The capability policy is the **global** lever that filters `tools/list`
// for the AI; per-project permissions in the `permissions` table still
// gate individual mutations. The two layers compose — see
// `docs/mcp-permission-policy.md`.

/// Return the current global capability policy. Frontend reads this on
/// settings hydration and on every reconnect so the UI always reflects
/// the source of truth (sqlite `preferences.mcp.policy`).
#[tauri::command]
pub async fn mcp_capability_get(pool: tauri::State<'_, DbPool>) -> Result<CapabilityPolicy, AppError> {
    crate::db::with_conn(pool.inner(), |c| crate::db::preferences::mcp_policy(c))
}

/// Set the global capability policy. The frontend sends the wire form
/// (`"read_only"` / `"safe_write"` / `"full_access"`) for symmetry with
/// the REST `PUT /api/mcp/capability` endpoint.
#[tauri::command]
pub async fn mcp_capability_set(policy: String, pool: tauri::State<'_, DbPool>) -> Result<CapabilityPolicy, AppError> {
    let parsed = CapabilityPolicy::parse(&policy)
        .ok_or_else(|| AppError::Validation(format!("unknown mcp policy '{policy}' (expected read_only | safe_write | full_access)")))?;
    crate::db::with_conn(pool.inner(), |c| crate::db::preferences::set_mcp_policy(c, parsed))?;
    Ok(parsed)
}

/// Return the tool names visible under the **current** capability
/// policy. The frontend uses it to render the inventory list; the
/// capability-aware `tools/list` over JSON-RPC uses the same code path
/// via `mcp::tool_descriptors`.
#[tauri::command]
pub async fn mcp_capability_list_tools(pool: tauri::State<'_, DbPool>) -> Result<Vec<String>, AppError> {
    use std::sync::Arc;
    use std::time::Instant;
    let state = crate::mcp::AppState {
        pool: pool.inner().clone(),
        started_at: Instant::now(),
        metrics: Arc::new(crate::mcp_metrics::McpMetrics::new()),
    };
    Ok(crate::mcp::tool_descriptors(&state).await.into_iter().map(|t| t.name).collect())
}