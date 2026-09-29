// ===========================================================================
// Endpoint-scoped send — the one place a configured endpoint becomes an
// outbound HTTP request.
// ===========================================================================

use serde::Deserialize;

use crate::db::DbPool;
use crate::domain::*;
use crate::error::AppError;

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
pub(super) fn resolve_send_input(
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
