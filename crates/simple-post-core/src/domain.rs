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
//!
//! **No per-field `#[serde(rename)]` anywhere in this file.** The Rust
//! field name *is* the wire name, derived once by `rename_all`. The
//! reason is not purity — a rename is a second source of truth for a
//! field's name, and it is the seam where the two languages drift
//! apart: `ApiKeyAuth` renamed its location field to `in` for
//! TypeScript's benefit, and TypeScript could not even spell `in`,
//! so the two sides disagreed and the value never round-tripped. When
//! a wire name genuinely needs to differ from the Rust name, expand the
//! Rust name instead — `in` is reserved on the TypeScript side, hence
//! `in_location`, and a trailing-underscore workaround does not help
//! because `in_` camelCases straight back to `in`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ----- HTTP method ---------------------------------------------------------

/// Mirrors `HttpMethod` on the TS side. Serializes to the upper-case verb
/// string (`"GET"`, `"POST"`, …) the renderer already uses; an unknown
/// verb falls through to `Custom(String)` so a forward-compat write from
/// the UI doesn't blow up on the Rust side.
///
/// `Deserialize` is hand-written so there is exactly **one** parser for
/// this type. A derived impl accepted `"get"` as `Custom("get")` and
/// round-tripped it back out lowercase, which the database then stored —
/// so an IPC write and an MCP call using the same verb produced two
/// different stored representations. Everything now goes through
/// [`parse_http_method`].
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
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
    /// Serialises as a bare string: the enum's `UPPERCASE` rule renames
    /// the unit variants above, and without `untagged` this one would
    /// become `{"CUSTOM":"PROPFIND"}`. The matching *read* side is the
    /// hand-written `Deserialize` below, which does not need it.
    #[serde(untagged)]
    Custom(String),
}

impl<'de> serde::Deserialize<'de> for HttpMethod {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        parse_http_method(&raw).map_err(serde::de::Error::custom)
    }
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

/// RFC 9110 `token`: the only characters a method may contain. Anything
/// else — a space, a CJK character, a control byte — is rejected here
/// rather than carried all the way to `reqwest`, which would surface as
/// an opaque transport error at send time.
fn is_method_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// Canonical parser used by every MCP tool, the request-execution path
/// **and** `HttpMethod`'s `Deserialize` impl. A known verb is mapped
/// case-insensitively; any other RFC 9110 token (e.g. `PROPFIND`, `LOCK`)
/// flows through as `HttpMethod::Custom`, so future / niche HTTP verbs
/// don't lose data. Empty, whitespace-bearing or non-token input is
/// rejected with `AppError::Validation` so the caller — an LLM, usually —
/// gets a message that says what was wrong.
pub fn parse_http_method(m: &str) -> Result<HttpMethod, crate::error::AppError> {
    use crate::error::AppError;
    let trimmed = m.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation("method must not be empty".into()));
    }
    if !is_method_token(trimmed) {
        return Err(AppError::Validation(format!(
            "method {trimmed:?} is not a valid HTTP token"
        )));
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

/// Where the apikey is attached. Reaches the wire as
/// `inLocation: "header" | "query"` on [`ApiKeyAuth::in_location`];
/// see the module docs for why the obvious `in` is not usable.
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
    /// Serialises as `inLocation`. `in` is a reserved word in
    /// TypeScript, and `in_` is not a fix — `rename_all = "camelCase"`
    /// maps `in_` back to `in` on the wire, so the two languages would
    /// disagree exactly as before. See the module docs.
    pub in_location: ApiKeyLocation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    /// Which scheme is in play. Reads as `kind` on the wire because
    /// that is what the field is called here — `type` is a keyword in
    /// Rust too, and a serde `rename` to paper over that is what used
    /// to make this field disagree with [`ResourceNode::kind`].
    pub kind: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apikey: Option<ApiKeyAuth>,
}

// ----- base url -----------------------------------------------------------

/// One named service in a **project's** service list.
///
/// The list is project-wide: every module sees every service, which is the
/// model the user asked for — 「服务应该绑定到项目，这样每一个模块都会全部
/// 包含这些服务」. There is deliberately no module field and no per-row
/// default flag, so two services in one project cannot share a `name` and
/// an endpoint's `baseUrlDefinitionId` is unambiguous across the whole
/// tree. The module-owned shape those two fields served (`moduleId` /
/// `isDefault`) was shipped, tried and reversed; the columns still exist
/// in the table, neutralised, because dropping them would mean a rebuild
/// would mean a `SCHEMA_VERSION` bump would mean deleting the user's
/// store.
///
/// A row names a service; it never holds a URL. The URL for (service ×
/// module × environment) lives in [`Environment::module_base_urls`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseUrlDefinition {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ----- resource node -------------------------------------------------------
//
// The merged tree: one row per module / collection / endpoint, with the
// endpoint's request configuration split out into `EndpointDetail`.

/// What a [`ResourceNode`] is. Projects are not nodes — a project is the
/// implicit root context, and `root_id` names it.
///
/// The set is closed on purpose. `nodes.type` carries a matching CHECK
/// constraint, and `parse` rejects anything else rather than inventing a
/// variant, so an unknown value can never reach a match arm that
/// silently treats it as something it isn't.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResourceNodeType {
    Module,
    Collection,
    Endpoint,
}

impl ResourceNodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceNodeType::Module => "module",
            ResourceNodeType::Collection => "collection",
            ResourceNodeType::Endpoint => "endpoint",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "module" => Some(ResourceNodeType::Module),
            "collection" => Some(ResourceNodeType::Collection),
            "endpoint" => Some(ResourceNodeType::Endpoint),
            _ => None,
        }
    }

    /// Can a node of this type sit directly under a node of `parent`?
    ///
    /// SQLite cannot express this over one polymorphic `parent_id` —
    /// CHECK constraints cannot read another row — so the matrix lives
    /// here and is enforced on every write by `db::nodes`, with
    /// matching triggers in `schema.sql`.
    ///
    /// An endpoint's parent is deliberately two types, not one. The old
    /// layout only had `endpoints.collection_id REFERENCES
    /// collections(id)`, so a collection was mandatory, and that made
    /// the simplest case — one endpoint, no folder worth creating —
    /// cost a throwaway collection first. A module takes endpoints
    /// directly; a collection still takes them and keeps its own
    /// sub-collections.
    pub fn may_under(self, parent: ResourceNodeType) -> bool {
        match self {
            // A module's parent is the project, which is not a node. The
            // caller expresses that as `parent_id = NULL`, and the
            // absence of a parent is the only legal case.
            ResourceNodeType::Module => false,
            // A collection either opens a module or nests under another
            // collection — nesting is unbounded, which is why the tree
            // renderer recurses.
            ResourceNodeType::Collection => matches!(
                parent,
                ResourceNodeType::Module | ResourceNodeType::Collection
            ),
            // A collection groups an endpoint with its siblings; a module
            // accepts one that needs no folder of its own.
            ResourceNodeType::Endpoint => matches!(
                parent,
                ResourceNodeType::Module | ResourceNodeType::Collection
            ),
        }
    }
}

/// One row of the tree: structure and identity, nothing more.
///
/// Endpoint request configuration lives in [`EndpointDetail`], keyed by
/// `node_id`. Splitting it is not tidiness — `method` / `url` / `tags`
/// mean nothing for a module, and folding them in would give every
/// module and collection row permanently empty columns.
///
/// The mirror in `src/types/domain.ts` carries one field this does not:
/// `expanded?: boolean`, the renderer's fold/open state. It is
/// deliberately absent here — it is neither persisted nor part of a
/// node's identity, so it has no business in a row type. The
/// asymmetry is safe because this struct ignores unknown keys, so a
/// node the renderer hands back with `expanded` set simply has it
/// dropped. `#[serde(deny_unknown_fields)]` would be the wrong tool
/// for closing that gap: it would reject the field outright, and it
/// would also break [`EndpointResource`]'s `#[serde(flatten)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNode {
    pub id: String,
    /// Owning project. Denormalised so a whole tree is one indexed read
    /// and so resolving "which project does this belong to" — the first
    /// step of the per-project permission gate — is a primary-key hit
    /// instead of a walk up the parent chain.
    pub root_id: String,
    /// `None` for a module, whose parent is the project.
    pub parent_id: Option<String>,
    /// Which of the three kinds this row is. Wire name `kind` for the
    /// same reason as [`AuthConfig::kind`]: `type` is a Rust keyword,
    /// and renaming it to `type` on the wire only re-opened the gap
    /// between the Tauri command argument and the REST body.
    pub kind: ResourceNodeType,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub sort_order: i64,
    #[serde(default = "now_ms")]
    pub created_at: i64,
    #[serde(default = "now_ms")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceNodePatch {
    /// Re-parent. `None` leaves the current parent alone; an explicit
    /// `Some(None)` promotes a node to the top of its project, which is
    /// only legal for a module.
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
}

impl ResourceNodePatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// Endpoint request configuration. Joins to [`ResourceNode`] on
/// `node_id`; the tree columns deliberately do not appear here so they
/// have exactly one home.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetail {
    pub node_id: String,
    pub method: HttpMethod,
    /// Path only — the host is resolved at send time from the active
    /// environment's `moduleBaseUrls`, so the same endpoint can be
    /// retargeted between dev / staging / prod without a rewrite.
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// The service this endpoint resolves against. `None` means "use the
    /// project's ★" (`Project::default_base_url_definition_id`), which is
    /// not the same as naming one: the resolution step that reads it is
    /// what makes the difference visible, so the field stays absent rather
    /// than being filled in with the default's id at write time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_definition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetailPatch {
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
}

impl EndpointDetailPatch {
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

/// True when `value` carries no field the wire would emit.
///
/// The patch types answer "is there anything to write?" by asking
/// whether they serialise to an empty object, rather than by listing
/// their own fields. Every patch field already carries
/// `skip_serializing_if = "Option::is_none"`, so that attribute *is*
/// the encoding of "absent means don't touch this column" — deriving
/// the check from it means there is exactly one source of truth and a
/// newly added field cannot drift out of sync with a hand-written
/// field list.
///
/// The three-state case falls out for free: `Some(None)` (clear the
/// column) serialises to an explicit `null`, so the key is present and
/// the patch is correctly *not* empty.
///
/// A serialisation failure is impossible for these types — every field
/// is a scalar, an `Option`, a `Vec`, or a nested struct that is itself
/// `Serialize` — but the fallback is `false` rather than `true` on
/// purpose. Preferring "not empty" means an unexpected failure makes
/// the write happen and the real error surface at the DB layer, instead
/// of being swallowed as a silent no-op.
fn serialises_to_empty_object<T: Serialize>(value: &T) -> bool {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_object().map(|m| m.is_empty()))
        .unwrap_or(false)
}

/// An endpoint as the tree and the editor both see it: the node's
/// structure plus its request configuration.
///
/// Separate from [`ResourceNode`] so a tree read can skip
/// `endpoint_details` entirely — the tree only ever renders
/// name / type / sort_order, and joining the detail table for every
/// node to satisfy that would read a column set most rows do not have.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointResource {
    #[serde(flatten)]
    pub node: ResourceNode,
    pub detail: EndpointDetail,
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
    /// The project's ★ service. An endpoint naming no service of its own
    /// resolves against this one, so a project with `None` here sends
    /// those requests with no base URL rather than borrowing another
    /// project's or another module's. NULL is the state a freshly created
    /// project is in: nothing is seeded, because a service the user did
    /// not name is a service they then have to delete.
    ///
    /// Deliberately **not** `skip_serializing_if`. "This project has no
    /// default baseUrl" is a decision the project makes, not an absence
    /// of data, and the TypeScript mirror declares this field as
    /// required-but-nullable (`defaultBaseUrlDefinitionId: string |
    /// null`). Emitting `null` keeps the two sides telling the same
    /// story; omitting the key made a declared contract field arrive as
    /// `undefined`. Every other optional field on this struct is
    /// genuinely optional in the mirror, so it keeps the skip.
    #[serde(default)]
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
    /// The project's ★. `Some(None)` clears it, which is a real state
    /// rather than an omission: the renderer offers 「无默认服务」 in the
    /// same menu that names one, and an endpoint naming no service then
    /// resolves to no base URL at all.
    #[serde(default, deserialize_with = "double_option")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_base_url_definition_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl ProjectPatch {
    /// No write path calls this yet; it exists so the next caller does
    /// not hand-roll a field list. See [`serialises_to_empty_object`].
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

// ----- environment --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// **DEPRECATED — read for wire compatibility, written by nothing.**
    ///
    /// It was `def_id -> url`, the value layer back when a URL did not
    /// care which module the endpoint sat in. The live layer is
    /// [`Self::module_base_urls`]; `db::migrate` copied every value into
    /// it and is the only reader of the column left in the crate.
    ///
    /// Kept on the struct so a payload from an older client still
    /// deserialises — the mirror declares it optional — but
    /// `db::environments::update` ignores it. Silently dropping it is the
    /// price, and this comment is the record of it.
    #[serde(default)]
    pub base_urls: HashMap<String, String>,
    /// `def_id -> module_id -> url`. **The only layer a URL lives in.**
    ///
    /// Each cell is independent: a missing entry means "this module has
    /// no address for this service", and resolution stops there rather
    /// than falling back to the environment-level map or to another
    /// module's value. That is why the map is keyed by module at all —
    /// two modules can legitimately point the same service at different
    /// hosts, and an empty cell is a real answer rather than a hole.
    #[serde(default)]
    pub module_base_urls: HashMap<String, HashMap<String, String>>,
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
    /// Deprecated with [`Environment::base_urls`]: accepted so an older
    /// payload still deserialises, never written — the column is dead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_urls: Option<HashMap<String, String>>,
    /// Replaces the whole `module_base_urls` map. `db::environments::update`
    /// writes it as one atomic `UPDATE` of the JSON column, so a reader can
    /// never observe half the cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_base_urls: Option<HashMap<String, HashMap<String, String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
}

impl EnvironmentPatch {
    /// No write path calls this yet; it exists so the next caller does
    /// not hand-roll a field list. See [`serialises_to_empty_object`].
    pub fn is_empty(&self) -> bool {
        serialises_to_empty_object(self)
    }
}

// ----- agent permission ---------------------------------------------------

/// Per-project MCP permission. `Read < ReadCall < Full`; the ordering
/// itself lives in [`crate::permissions::permission_at_least`].
///
/// `snake_case`, not `lowercase`: `"lowercase"` maps `ReadCall` to
/// `"readcall"`, a third spelling that matched neither [`Self::as_str`]
/// (which is what `db::permissions` writes and `parse` reads), nor the
/// `agent_permissions` CHECK constraint, nor the `AgentPermission`
/// union on the TypeScript side. That third spelling survived only
/// because the one consumer of the serialised form happened to fall
/// back to a value equal to the correct answer. `snake_case` makes all
/// four agree, and matches its sibling
/// [`crate::permissions::CapabilityPolicy`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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

/// Outbound-request knobs. A singleton, persisted as a JSON blob in the
/// `preferences` KV table rather than in a table of its own.
///
/// `#[serde(default)]` at the container level is load-bearing on the
/// **read** side, not the write side. The stored blob is whatever an
/// earlier build wrote, so the day a fourth field is added here, every
/// existing install's row deserialises without it — and without this
/// attribute `request_settings_get` would fail outright for all of
/// them. The cost lands on the write side: `PUT /api/request_settings`
/// is a full replace, and a body that omits a field now resets that
/// field to its default instead of being rejected. For a settings
/// singleton whose defaults are documented on [`Default`], that is the
/// more useful behaviour; the route's own comment says so too.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
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

// ----- MCP HTTP listener config -------------------------------------------

/// Where the MCP HTTP daemon listens and who it answers to.
///
/// A settings singleton persisted as a JSON blob in `preferences` under
/// [`crate::db::preferences::keys::MCP_HTTP_CONFIG`] — the same
/// "preferences bag holds JSON values" rule every other structured
/// setting follows.
///
/// `#[serde(default)]` at the container level is load-bearing on the
/// **read** side for the same reason it is on [`RequestSettings`]: the
/// stored blob is whatever an earlier build wrote, so a field added
/// later must not make every existing install's row fail to parse.
/// The cost lands on the write side — a body that omits a field resets
/// that field to its default instead of being rejected — which is the
/// more useful behaviour for a settings singleton.
///
/// The security-relevant half is `allow_remote` plus the two allow
/// lists, and they are **fail-closed by construction**: the default
/// (`allow_remote: false`, both lists empty) accepts only loopback
/// `Host` headers and no `Origin` at all. `validate` refuses the one
/// combination that would silently contradict itself — a non-loopback
/// bind with remote access left off, which would listen on the LAN
/// while the guard rejected every request that actually arrived from
/// it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct McpHttpConfig {
    /// Bind address. `127.0.0.1` keeps the daemon local-only.
    pub host: String,
    /// Listen port. `0` means "OS-assigned" — the daemon's own chosen
    /// port is then published through the `mcp.port` record.
    pub port: u16,
    /// Mount path for the JSON-RPC endpoint. Must start with `/`.
    pub path: String,
    /// Master switch for non-loopback access. `false` (the default)
    /// means the `Host` guard only accepts loopback names.
    pub allow_remote: bool,
    /// `Host` values accepted once `allow_remote` is on. A bare `"*"`
    /// accepts any. Empty means *no* non-loopback host is accepted —
    /// not "accept everything".
    pub allowed_hosts: Vec<String>,
    /// `Origin` values accepted. AI clients (curl, Claude Code) send no
    /// `Origin` at all and are therefore never blocked by this; a
    /// browser-style request that *does* send one must match. A bare
    /// `"*"` accepts any.
    pub allowed_origins: Vec<String>,
}

impl Default for McpHttpConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_MCP_HOST.to_string(),
            port: 0,
            path: DEFAULT_MCP_PATH.to_string(),
            allow_remote: false,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
        }
    }
}

/// Default bind address for the MCP HTTP daemon — loopback only.
pub const DEFAULT_MCP_HOST: &str = "127.0.0.1";
/// Default mount path for the MCP JSON-RPC endpoint.
pub const DEFAULT_MCP_PATH: &str = "/mcp";

impl McpHttpConfig {
    /// The mount path in the exact form axum's router wants: always a
    /// leading `/`, never a trailing one. `"/"`, `""` and a missing
    /// slash all collapse onto [`DEFAULT_MCP_PATH`] rather than
    /// producing a router that 404s everything the user can see in the
    /// settings panel.
    pub fn normalized_path(&self) -> String {
        let raw = self.path.trim();
        let trimmed = raw.trim_end_matches('/');
        if trimmed.is_empty() {
            return DEFAULT_MCP_PATH.to_string();
        }
        if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        }
    }

    /// Reject configurations that cannot mean what they say.
    ///
    /// Called on every write (Tauri command and REST route alike) so an
    /// invalid combination can never reach the `preferences` row that
    /// `daemon_cli_args` reads at spawn time.
    pub fn validate(&self) -> Result<(), crate::error::AppError> {
        use crate::error::AppError;

        if self.host.trim().is_empty() {
            return Err(AppError::Validation("host must not be empty".into()));
        }
        // Normalise before checking so `/mcp/` and `/mcp` cannot both
        // pass validation and then disagree at the router.
        if self.path.trim().is_empty() {
            return Err(AppError::Validation(format!(
                "path must not be empty (expected something like {DEFAULT_MCP_PATH:?})"
            )));
        }
        if !self.normalized_path().starts_with('/') {
            return Err(AppError::Validation(format!(
                "path must start with '/' (got {:?})",
                self.path
            )));
        }
        // The self-contradicting combination: bound to the LAN, but the
        // guard configured to reject everything that isn't loopback —
        // the daemon would be unreachable and would read as "MCP is
        // broken" rather than "your settings contradict each other".
        if !self.allow_remote && !is_loopback_host(&self.host) {
            return Err(AppError::Validation(format!(
                "host {:?} is not a loopback address, so allowRemote must also be \
                 enabled — otherwise the daemon listens on a network the Host guard \
                 refuses to answer",
                self.host
            )));
        }
        Ok(())
    }
}

/// Is `host` one of the loopback names the guard always accepts?
///
/// Shared by [`McpHttpConfig::validate`] and the daemon's Host guard —
/// they must agree, or a config could validate and then lock the
/// listener out of answering its own healthz probe.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

// ----- api response (transient; not persisted in the schema) -------------

/// The wire shape a response takes on its way out of Rust.
///
/// It is **not** the shape the renderer ends up holding, and it is not
/// in-process either — it crosses the Tauri IPC boundary like every
/// other type here. The difference is `headers`: this side models them
/// as ordered pairs, because a response may legitimately repeat one
/// (several `Set-Cookie` lines, say) and a map cannot express that. The
/// renderer wants a plain object, so `src/lib/http.ts` folds the pairs
/// through `tuplesToRecord` on arrival — lossy, and the one place in
/// the request path where information is knowingly dropped; duplicates
/// collapse last-one-wins.
///
/// Nothing in Rust describes the post-adapter shape.
/// `ApiResponse` in `src/types/domain.ts` is that description, and it
/// is deliberately a different type rather than a claimed mirror of
/// this one.
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
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

/// Default project color when a caller doesn't supply one. Lives here
/// rather than in `commands/` so the REST and Tauri write paths cannot
/// drift: `commands` already depends on `domain`, so a second copy on
/// the far side of that edge would be free to disagree.
pub fn default_project_color() -> String {
    "#3b82f6".to_string()
}

/// A new primary key, `sp_` followed by the wall clock in hex and a
/// per-process counter, concatenated.
///
/// It is **not** opaque and not unguessable — it encodes the timestamp
/// it was minted at, which is fine for a single-user desktop app whose
/// database is already readable by that user, and is stated here so
/// nobody reaches for it in a context that needs more. The counter is
/// what makes it unique: two keys minted in the same nanosecond would
/// otherwise collide, and a process that restarts within one clock tick
/// of a previous one could repeat a prefix.
///
/// A random suffix would remove both caveats; it is not worth the
/// dependency while the store is local.
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
