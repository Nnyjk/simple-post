//! Shared domain types — 1:1 mirror of `src/types/domain.ts`.
//!
//! These structs are the wire shape that crosses the Tauri IPC boundary
//! (renderer ↔ Rust). Field naming follows `#[serde(rename_all = "camelCase")]`
//! so the React code can keep using its existing `snake_case → camelCase`
//! naming without translation.
//!
//! SQLite persistence layer (`crate::db`) serializes the more complex
//! nested fields (KeyValue[], Body, Auth) into JSON TEXT columns;
//! see `db/endpoints.rs` for the (de)serialization round-trip.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ----- HTTP method ---------------------------------------------------------

/// Mirrors `HttpMethod` on the TS side. Serializes to the upper-case verb
/// string (`"GET"`, `"POST"`, …) the renderer already uses; an unknown
/// verb falls through to `Custom(String)` so a forward-compat write from
/// the UI doesn't blow up on the Rust side.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    /// Any verb not in the standard list (e.g. `PROPFIND`, `LOCK`).
    #[serde(untagged)]
    Custom(String),
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Custom(v) => v.as_str(),
        }
    }
}

/// Canonical parser used by every MCP tool and the request-execution path.
/// Empty / whitespace-only input is rejected with `AppError::Validation`
/// (so the LLM sees a useful message); a known verb is mapped case-insensitively;
/// any token-shape ASCII verb (e.g. `PROPFIND`) flows through as
/// `HttpMethod::Custom` so future / niche HTTP verbs don't lose data.
pub fn parse_http_method(m: &str) -> Result<HttpMethod, crate::error::AppError> {
    use crate::error::AppError;
    let trimmed = m.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("method must not be empty".into()));
    }
    Ok(match trimmed.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        other => HttpMethod::Custom(other.into()),
    })
}

// ----- key / value ----------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub id: String,
    pub key: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ----- body ---------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BodyMode {
    #[default]
    None,
    Json,
    Form,
    Raw,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    pub mode: BodyMode,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

// ----- auth ---------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    #[default]
    None,
    Bearer,
    Basic,
    Apikey,
}

/// Where the apikey is attached. The TS interface declares the field as
/// `in_` because `in` is a Rust/JS reserved word; serde rename below keeps
/// the JSON shape as `"in": "header" | "query"`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
    /// Field name on the wire is `"in"`. See [`ApiKeyLocation`].
    #[serde(rename = "in")]
    pub in_: ApiKeyLocation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub kind: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apikey: Option<ApiKeyAuth>,
}

// ----- endpoint -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub method: HttpMethod,
    /// Path-only URL; the actual host is resolved at send time via
    /// `Environment.baseUrls[baseUrlDefinitionId]`.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: RequestBody,
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub tags: Vec<String>,
    pub sort_order: i64,
    /// Server-side timestamp; defaults to `now_ms()` when the caller
    /// (e.g. the REST `POST /api/endpoints` handler) doesn't supply one.
    /// Tauri commands always set this explicitly so the default is inert
    /// for them.
    #[serde(default = "now_ms")]
    pub created_at: i64,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

/// Partial-update payload. All fields are `Option<T>` so an unset
/// `None` keeps the existing value. `Option<Option<T>>` (double Option)
/// is used where the TS side distinguishes *absent* from *explicit null*
/// — e.g. `docs: null` should clear the column, while `docs` not in the
/// patch object should leave it alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<KeyValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<KeyValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
}

impl EndpointPatch {
    /// True if no field is being changed. Used by the partial-update
    /// path to avoid a no-op SQL write.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.method.is_none()
            && self.url.is_none()
            && self.environment_id.is_none()
            && self.base_url_definition_id.is_none()
            && self.base_url_override.is_none()
            && self.params.is_none()
            && self.headers.is_none()
            && self.body.is_none()
            && self.auth.is_none()
            && self.docs.is_none()
            && self.notes.is_none()
            && self.tags.is_none()
            && self.sort_order.is_none()
    }
}

// ----- base url -----------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefinition {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ----- collection ---------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub module_id: String,
    pub parent_collection_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i64,
    /// Derived in the tree on the renderer side. Persisted for round-trip
    /// fidelity but never read back into the DB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_id: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_collection_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
}

// ----- module -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
    /// Re-parent the module to a different project.
    ///
    /// The renderer deliberately never sends this (`updateModule` in
    /// `crud-slice.ts` is typed `Partial<Omit<Module, 'id' | 'projectId'>>`),
    /// so the only caller is the MCP `module_move` tool. It exists because
    /// a cross-project move must be a real `UPDATE`: the previous
    /// delete-then-create approach destroyed every collection and endpoint
    /// under the module via `ON DELETE CASCADE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

// ----- project ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub base_url_definitions: Vec<BaseUrlDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_base_url_definition_id: Option<String>,
    #[serde(default = "default_project_color")]
    pub color: String,
    /// Server-side timestamp; defaults to `now_ms()` when the caller
    /// (e.g. the REST `POST /api/projects` handler) doesn't supply one.
    /// Tauri commands always set this explicitly so the default is inert
    /// for them.
    #[serde(default = "now_ms")]
    pub created_at: i64,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definitions: Option<Vec<BaseUrlDefinition>>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url_definition_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

// ----- environment --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub base_urls: HashMap<String, String>,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_urls: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

// ----- agent permission ---------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Read,
    #[default]
    ReadCall,
    Full,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::ReadCall => "read_call",
            Permission::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Permission::Read),
            "read_call" => Some(Permission::ReadCall),
            "full" => Some(Permission::Full),
            _ => None,
        }
    }
}

// ----- request settings ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettings {
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub max_response_size_kb: u64,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            follow_redirects: true,
            max_response_size_kb: 5120,
        }
    }
}

// ----- api response (in-process; not persisted in the schema -------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub headers: Vec<[String; 2]>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_json: Option<serde_json::Value>,
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

/// Current wall-clock as unix-milliseconds. Single source of truth
/// for `*_at` columns on the write paths so the values are
/// self-consistent within a single transaction.
///
/// `SystemTime::now()` panics only on clock skew past the unix epoch;
/// treat that as a programmer error rather than user-recoverable.
/// Default project color when a caller doesn't supply one. Mirrors
/// `commands::default_color` so REST and Tauri paths agree. The two
/// live in different modules to avoid a circular dep (commands already
/// depends on domain).
pub fn default_project_color() -> String {
    "#3b82f6".to_string()
}

/// Generate a unique opaque id. Mirrors `commands::uid` so REST and
/// Tauri paths agree. Both functions live here because `commands` already
/// depends on `domain`; pulling the inverse into commands.rs would cycle.
/// The per-process monotonic counter guarantees uniqueness inside the
/// running process even when the wall clock hasn't advanced.
pub fn uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sp_{:x}{:x}", nanos, n)
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

/// Deserialize `null` and missing as *different* states on `Option<T>`:
///   - JSON absent  → `None` outer (`patch.field` not set → don't touch column)
///   - JSON `null`  → `Some(None)`             → write NULL to column
///   - JSON `"foo"` → `Some(Some("foo"))`      → write "foo"
///
/// serde's built-in `Option<T>` collapses absent and `null`, which
/// destroys the "clear column" vs "leave alone" distinction in patch
/// structs. This wrapper restores it.
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// -------------------------------------------------------------------------
// Tests — round-trip serialization for the shapes that cross IPC.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "domain_test.rs"]
mod tests;
