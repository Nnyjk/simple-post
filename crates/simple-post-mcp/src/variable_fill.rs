//! Variable-fill + baseUrl resolution + auth injection.
//!
//! Mirrors `src/lib/url.ts` (`resolveEndpointBaseUrl`, `resolveVars`)
//! and `src/lib/http-build.ts` (`buildFetchInit`) on the renderer
//! side. Keeping the logic here means the MCP daemon and the
//! Tauri command path share one resolver instead of drifting
//! apart — same as the `buildFetchInit` / `buildCurlCommand` shared
//! layer the renderer uses today.
//!
//! Two layers of API:
//!
//!   * [`resolve_template`] — `{{var}}` substitution against a
//!     variable map. The low-level building block.
//!   * [`resolve_endpoint`] — combine an [`EndpointDetail`] + active
//!     `Environment` + `Project` + optional [`EndpointOverrides`]
//!     into a fully-formed [`ResolvedEndpoint`] ready for the HTTP
//!     sender.

use std::collections::HashMap;

use serde::Deserialize;

use simple_post_core::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, EndpointDetail, Environment,
    HttpMethod, Project, RequestBody,
};
use simple_post_core::error::AppError;

// -------------------------------------------------------------------------
// Public types
// -------------------------------------------------------------------------

/// `{{var}}` substitution result. `missing` is sorted/deduped so the
/// caller can show a stable "unresolved variables" list to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTemplate {
    pub resolved: String,
    pub missing: Vec<String>,
}

/// Override values supplied by the caller at send time. Each field
/// is `Option<T>` so "not overridden" stays distinct from "overridden
/// to None / empty". Drives ad-hoc curl-style tweaks from the
/// renderer without rewriting the stored endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointOverrides {
    pub url: Option<String>,
    pub method: Option<HttpMethod>,
    pub headers: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub params: Option<Vec<simple_post_core::domain::KeyValue>>,
    pub body: Option<RequestBody>,
    /// Which base-URL definition to resolve against, instead of the one
    /// persisted on the endpoint.
    ///
    /// The renderer's base-URL picker is ephemeral: a user can switch
    /// definitions without saving the endpoint, so the stored
    /// `detail.base_url_definition_id` is not authoritative at send
    /// time. Without this field the browser would have to pre-join the
    /// base URL and the whole point of moving URL composition server-side
    /// would be lost.
    ///
    /// `None` keeps the endpoint's own value. `Some(id)` overrides it;
    /// an empty string means "no base URL", which is how the renderer
    /// represents an endpoint being sent against a bare path.
    pub base_url_definition_id: Option<String>,
    /// Which module's **cell** in `env.moduleBaseUrls` this endpoint
    /// reads.
    ///
    /// Services belong to the project, not to a module, so the module is
    /// not a filter over which services exist — every module sees the
    /// project's whole list. It selects the value instead: a URL lives at
    /// `(environment × service × module)`, and two modules may point the
    /// same service at different hosts.
    ///
    /// Supplied by the caller rather than derived here because this
    /// function has no database handle — it is pure, and the module
    /// lives in the `nodes` table. `None` means "no tree context": there
    /// is no cell to read, so only the per-endpoint
    /// `base_url_override` can produce a base URL.
    pub module_id: Option<String>,
}

/// What `resolve_endpoint` produces. Already has every `{{var}}`
/// substituted, the baseUrl joined in, and the auth config reduced
/// to its post-resolve form. The HTTP sender (`simple_post_core::http_executor`)
/// consumes this directly.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub params: Vec<(String, String)>,
    pub body: Option<String>,
    pub auth: AuthConfig,
    pub missing_vars: Vec<String>,
}

// -------------------------------------------------------------------------
// resolve_template — the {{var}} substitution primitive.
// -------------------------------------------------------------------------

/// Substitute every `{{name}}` marker in `template` against `vars`.
/// Unknown markers are left as `{{name}}` (not silently turned into
/// the string "undefined" or an empty value) so the request preview
/// shows what's missing.
///
/// Non-alphanumerics in the marker name (e.g. `{{ my-var }}`) are
/// not supported — the regex matches `\w+` which is `[A-Za-z0-9_]+`,
/// the same set the renderer accepts. This is intentional: the
/// TS-side regex (`/\\{\\{(\\w+)\\}\\}/g`) is the source of truth.
pub fn resolve_template(template: &str, vars: &HashMap<String, String>) -> ResolvedTemplate {
    if template.is_empty() {
        return ResolvedTemplate {
            resolved: String::new(),
            missing: Vec::new(),
        };
    }

    let mut missing: Vec<String> = Vec::new();
    let mut seen_missing: HashMap<String, ()> = HashMap::new();

    let resolved = template_replace(template, |key| match vars.get(key) {
        Some(v) => v.clone(),
        None => {
            if seen_missing.insert(key.to_string(), ()).is_none() {
                missing.push(key.to_string());
            }
            format!("{{{{{key}}}}}")
        }
    });

    ResolvedTemplate { resolved, missing }
}

/// Walk `{{...}}` markers in `template` and let `repl` decide what
/// each one becomes. The inner scanner is non-recursive by design
/// (`{{{{a}}}}` → `{{a}}` on the first pass, then expanded if `a`
/// resolves) so a doubled-up marker can never blow the stack.
fn template_replace<F: FnMut(&str) -> String>(template: &str, mut repl: F) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find the matching `}}`.
            if let Some(end_rel) = find_close(template, i + 2) {
                let inner = &template[i + 2..i + 2 + end_rel];
                // Marker content must be a bare identifier (`\w+`),
                // matching the renderer regex. Anything else (e.g.
                // `{{ foo }}` with whitespace, `{{ foo.bar }}`)
                // passes through verbatim — same as the TS side.
                if is_word(inner) {
                    out.push_str(&repl(inner));
                } else {
                    out.push_str(&template[i..i + 2 + end_rel + 2]);
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }
        // Push one UTF-8 char (not byte) so we don't slice a rune.
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    out
}

fn find_close(s: &str, from: usize) -> Option<usize> {
    // Search for `}}` at a char boundary starting at `from`.
    let mut i = from;
    while i + 1 < s.len() {
        if s.as_bytes()[i] == b'}' && s.as_bytes()[i + 1] == b'}' {
            return Some(i - from);
        }
        i = next_char_boundary(s, i);
    }
    None
}

fn is_word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// Extract every `{{name}}` marker name from `s`. Used by
/// `resolve_endpoint` to gather all variables mentioned anywhere in
/// the URL / headers / params / body / auth fields, so the caller
/// can list "missing vars" without re-scanning.
pub fn list_missing_vars(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end_rel) = find_close(s, i + 2) {
                let inner = &s[i + 2..i + 2 + end_rel];
                if is_word(inner) && seen.insert(inner.to_string(), ()).is_none() {
                    out.push(inner.to_string());
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }
        i = next_char_boundary(s, i);
    }
    out
}

// -------------------------------------------------------------------------
// Base URL resolution — see src/lib/url.ts::resolveEndpointBaseUrl.
// -------------------------------------------------------------------------

/// Where the effective baseUrl came from. Used by picker-style
/// surfaces to label the current selection ("模块默认" /
/// "鉴权服务" / "自定义"). The MCP daemon uses the same enum to
/// explain its choice back to the renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseUrlSource {
    /// `detail.baseUrlOverride` won.
    Override,
    /// A service was named explicitly — the endpoint's own
    /// `baseUrlDefinitionId`, or the caller's `forced_def_id`.
    Definition,
    /// The project's ★ service
    /// (`Project.defaultBaseUrlDefinitionId`) won.
    Default,
    /// Nothing matched; the caller is responsible for whatever
    /// fallback (often `""`).
    None,
}

/// Compute the effective baseUrl for an endpoint, given the
/// env that supplies its `moduleBaseUrls[defId][moduleId]` and the
/// project that declares the services.
///
/// Precedence (highest first):
///   1. `endpoint.base_url_override`  — per-endpoint custom mode. `None`
///      falls through to the service chain; `Some(host)` pins that
///      host; `Some("")` means "no prefix at all" — the endpoint is in
///      自定义 mode and the URL field already holds the whole address.
///   2. `detail.base_url_definition_id`, then the project's ★
///      (`project.default_base_url_definition_id`).
///   3. `""` — nothing.
///
/// This three-argument form carries no module context, so step 2 has
/// no cell to read and resolves to `""` unless the per-endpoint
/// override applies. Callers that know the module must use
/// [`resolve_base_url_with`].
///
/// `env.module_base_urls` is keyed by `BaseUrlDefinition.id` then by
/// module id, NOT by name. That keeps rename operations cheap and the
/// resolution deterministic even if two services share a name. The env
/// is taken from `detail.environment_id` — when no env is selected
/// (None) the lookup short-circuits to "".
pub fn resolve_base_url(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
) -> (String, BaseUrlSource, Option<String>) {
    resolve_base_url_with(detail, env, project, None, None)
}

/// [`resolve_base_url`] with a caller-supplied definition choice and the
/// endpoint's module.
///
/// `forced_def_id` takes precedence over `detail.base_url_definition_id`
/// but still yields to `detail.base_url_override`, which is a per-endpoint
/// host (or the explicit "no prefix" choice) and was always the strongest
/// signal. `Some("")` means "no base URL" — the renderer sends that when
/// the user cleared the picker.
///
/// `module_id` selects **which cell of the value layer** this call reads,
/// not which services exist: services are project-scoped, and
/// `env.module_base_urls[def_id][module_id]` is the only place a URL
/// lives. `None` therefore resolves to `""` rather than guessing a
/// module's cell for another.
///
/// It is supplied rather than derived here so this function stays pure:
/// the module lives in the `nodes` table, which this module deliberately
/// knows nothing about. Callers thread it through
/// [`EndpointOverrides::module_id`].
///
/// Mirrors `resolveEndpointBaseUrl` in `src/lib/url.ts`; the two must
/// agree or the on-screen preview and the request that actually goes out
/// describe different URLs.
pub fn resolve_base_url_with(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
    forced_def_id: Option<&str>,
    module_id: Option<&str>,
) -> (String, BaseUrlSource, Option<String>) {
    // `Some("")` is a deliberate choice, not an absent value: the endpoint
    // is in 自定义 mode and the URL field carries the whole address, so
    // prepending the environment's base URL would corrupt it. Mirrors
    // `resolveEndpointBaseUrl` in src/lib/url.ts — the two must agree or the
    // preview (frontend) and the request (backend) disagree about the URL.
    if let Some(ovr) = detail.base_url_override.as_deref() {
        return (ovr.to_string(), BaseUrlSource::Override, None);
    }
    let forced = forced_def_id.map(|s| s.to_string());
    if matches!(forced.as_deref(), Some("")) {
        return (String::new(), BaseUrlSource::None, None);
    }

    // Which service answers: the caller's pick, else the endpoint's own,
    // else the project's ★. The endpoint naming none is the normal case,
    // not a gap — that is what the ★ exists for.
    let explicit = forced.or_else(|| detail.base_url_definition_id.clone());
    // Decided before `explicit` is consumed by the match below: this is
    // about *which row answered*, not which value, so a forced choice and
    // the endpoint's own pick both report `Definition`.
    let was_explicit = explicit.is_some();
    let def_id = match explicit {
        Some(def_id) => def_id,
        None => match project.default_base_url_definition_id.clone() {
            Some(def_id) => def_id,
            // No ★: the endpoint goes out with no base URL, full stop.
            // There is no layer above this one to borrow from and no
            // service to guess.
            None => return (String::new(), BaseUrlSource::None, None),
        },
    };
    // The id has to name a service the project actually holds. A miss
    // covers a deleted service, an id copied from another project, and a
    // stale endpoint pointer; all three mean "no base URL" rather than
    // "quietly aim at a different host". Deliberately NOT a fall back to
    // the ★.
    if !project.base_url_definitions.iter().any(|d| d.id == def_id) {
        return (String::new(), BaseUrlSource::None, None);
    }

    // No module, no cell. Services are shared by every module but their
    // addresses are not, so there is nothing to read.
    let module_id = match module_id {
        Some(module_id) => module_id,
        None => return (String::new(), BaseUrlSource::None, None),
    };

    // The one and only place a URL lives. Missing or empty means this
    // (environment × service × module) cell is unconfigured, which is the
    // same answer as naming nothing: `source: none`. There is deliberately
    // no fallback to `env.base_urls` (dead column) nor to another
    // module's cell — an empty cell is a real answer, not a hole.
    match env
        .and_then(|e| e.module_base_urls.get(&def_id))
        .and_then(|cells| cells.get(module_id))
    {
        Some(u) if !u.is_empty() => {
            let source = if was_explicit {
                BaseUrlSource::Definition
            } else {
                BaseUrlSource::Default
            };
            (u.clone(), source, Some(def_id))
        }
        _ => (String::new(), BaseUrlSource::None, None),
    }
}

/// Concatenate `base_url` and `path` safely. Treats `path` as
/// already-absolute if it carries a scheme, and collapses duplicate
/// slashes at the join boundary.
///
///   join_base_url("https://api.example.com", "/users/1")
///     → "https://api.example.com/users/1"
///   join_base_url("https://api.example.com/", "users/1")
///     → "https://api.example.com/users/1"
///   join_base_url("", "/users/1")
///     → "/users/1"
///   join_base_url("https://api.example.com", "https://other/")
///     → "https://other/"  (the path wins — it carries its own scheme)
pub fn join_base_url(base_url: &str, path: &str) -> String {
    if path.is_empty() {
        return base_url.to_string();
    }
    // Already has a scheme → path is fully self-contained.
    if has_scheme(path) {
        return path.to_string();
    }
    let trimmed_base = base_url.trim_end_matches('/');
    if trimmed_base.is_empty() {
        return path.to_string();
    }
    if path.starts_with('/') {
        format!("{trimmed_base}{path}")
    } else {
        format!("{trimmed_base}/{path}")
    }
}

fn has_scheme(s: &str) -> bool {
    scheme_prefix_len(s).is_some()
}

/// Byte length of a leading `scheme://`, or `None`. Shared by
/// [`has_scheme`] and [`collapse_url_slashes`] so the two can never
/// disagree about where the scheme ends — that boundary is exactly what
/// decides which slashes are a protocol and which are a typo.
fn scheme_prefix_len(s: &str) -> Option<usize> {
    // RFC 3986 scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
    // Mirrors the TS regex `/^[a-z][a-z0-9+.-]*:\/\//i`.
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    // Now expect `:` followed by `//`.
    if i + 2 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b'/' && bytes[i + 2] == b'/' {
        Some(i + 3)
    } else {
        None
    }
}

/// Collapse runs of `/` in the **path** of a URL that has already been
/// joined *and* `{{var}}`-resolved, leaving every other byte alone.
///
/// Joining happens before substitution — necessarily, because an
/// environment's own base URL may be templated — so [`join_base_url`]
/// cannot know that `{{var}}` will expand to something already starting
/// with `/`. It inserts a separator, and the substitution then turns
/// `http://host:9200/{{var}}` into `http://host:9200//api`, which is a
/// different route to most servers. Only after substitution is the truth
/// known.
///
/// Scope is deliberately narrow — a blind collapse would corrupt real
/// URLs:
///   - `scheme://` is not a path. `https://` must survive verbatim.
///   - a leading `//` with **no** scheme is a protocol-relative
///     reference (`//cdn.example.com/x`); its slashes carry meaning.
///   - `?query` / `#fragment` are data, not path: a param carrying a
///     nested URL (`?redirect=https://x//y`) must pass through untouched.
///   - an unresolved `{{var}}` marker is text, not a separator, so it
///     lands in `missing_vars` rather than being flattened away.
///
/// Mirrors `collapseUrlSlashes` in `src/lib/url.ts`; the two must agree
/// or the preview and the request that actually goes out describe
/// different URLs.
pub fn collapse_url_slashes(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    // `scheme://` and the protocol-relative `//` are heads, not path.
    let (head, rest) = match scheme_prefix_len(url) {
        Some(n) => url.split_at(n),
        None if url.starts_with("//") => url.split_at(2),
        None => ("", url),
    };
    // An authority never contains a `/` (it ends at the first one), so
    // whatever follows the head is safe to treat as path.
    let cut = rest.find(['?', '#']).unwrap_or(rest.len());
    let (path, tail) = rest.split_at(cut);

    let mut out = String::with_capacity(head.len() + path.len() + tail.len());
    out.push_str(head);
    let mut prev_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(ch);
    }
    out.push_str(tail);
    out
}

// -------------------------------------------------------------------------
// Auth — substitute `{{var}}` markers in bearer / basic / apikey fields.
// -------------------------------------------------------------------------

/// Apply `{{var}}` substitution to every auth field that carries
/// user-visible text. Returns a fresh `AuthConfig`; the input is
/// not mutated (callers may be re-using the endpoint object across
/// sends). Missing variables inside auth text flow into the same
/// missing-list the URL scan produced.
fn resolve_auth(auth: &AuthConfig, vars: &HashMap<String, String>) -> (AuthConfig, Vec<String>) {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record = |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            missing.push(var);
        }
    };

    let kind = auth.kind.clone();
    let bearer = match (kind.clone(), auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) => {
            let r = resolve_template(b, vars);
            for m in r.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(r.resolved)
        }
        _ => auth.bearer.clone(),
    };
    let basic = match (kind.clone(), auth.basic.as_ref()) {
        (AuthType::Basic, Some(b)) => {
            let u = resolve_template(&b.username, vars);
            for m in u.missing {
                record(&mut missing, &mut seen, m);
            }
            let p = resolve_template(&b.password, vars);
            for m in p.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(BasicAuth {
                username: u.resolved,
                password: p.resolved,
            })
        }
        _ => auth.basic.clone(),
    };
    let apikey = match (kind.clone(), auth.apikey.as_ref()) {
        (AuthType::Apikey, Some(a)) => {
            let k = resolve_template(&a.key, vars);
            for m in k.missing {
                record(&mut missing, &mut seen, m);
            }
            let v = resolve_template(&a.value, vars);
            for m in v.missing {
                record(&mut missing, &mut seen, m);
            }
            Some(ApiKeyAuth {
                key: k.resolved,
                value: v.resolved,
                in_location: a.in_location,
            })
        }
        _ => auth.apikey.clone(),
    };
    (
        AuthConfig {
            kind,
            bearer,
            basic,
            apikey,
        },
        missing,
    )
}

// -------------------------------------------------------------------------
// URL / header / param / body assembly.
// -------------------------------------------------------------------------

/// Build the full ordered header list for an endpoint.
///
/// Order: user headers (enabled, non-empty), then the auto
/// `Content-Type` if missing and the body mode implies one, then a
/// default `Accept: application/json`, then the auth-derived
/// `Authorization` (or apikey-as-header). Duplicate-key check is
/// case-insensitive (HTTP header names are).
///
/// `apikey.inLocation == Query` is NOT emitted as a header here — the
/// caller appends it to the URL via `append_apikey_query`.
fn build_headers(
    endpoint_headers: &[simple_post_core::domain::KeyValue],
    body_mode: &simple_post_core::domain::BodyMode,
    auth: &AuthConfig,
    vars: &HashMap<String, String>,
    missing: &mut Vec<String>,
) -> Vec<(String, String)> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out: Vec<(String, String)> = Vec::new();

    let record_missing = |out: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            out.push(var);
        }
    };

    for h in endpoint_headers {
        if !h.enabled || h.key.is_empty() {
            continue;
        }
        let r = resolve_template(&h.value, vars);
        for m in r.missing {
            record_missing(missing, &mut seen, m);
        }
        out.push((h.key.clone(), r.resolved));
    }

    if let Some(ct) = auto_content_type(body_mode) {
        if !out
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            out.push(("Content-Type".to_string(), ct.to_string()));
        }
    }
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) {
        out.push(("Accept".to_string(), "application/json".to_string()));
    }

    match (&auth.kind, auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) if !b.is_empty() => {
            if !out
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            {
                out.push(("Authorization".to_string(), format!("Bearer {b}")));
            }
        }
        (AuthType::Basic, _) => {
            if let Some(b) = &auth.basic {
                if !out
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
                {
                    let token = b64_basic(&b.username, &b.password);
                    out.push(("Authorization".to_string(), format!("Basic {token}")));
                }
            }
        }
        (AuthType::Apikey, _) => {
            if let Some(a) = &auth.apikey {
                if a.in_location == ApiKeyLocation::Header
                    && !out.iter().any(|(k, _)| k.eq_ignore_ascii_case(&a.key))
                {
                    out.push((a.key.clone(), a.value.clone()));
                }
            }
        }
        _ => {}
    }

    out
}

fn auto_content_type(mode: &simple_post_core::domain::BodyMode) -> Option<&'static str> {
    use simple_post_core::domain::BodyMode;
    match mode {
        BodyMode::Json => Some("application/json"),
        BodyMode::Form => Some("application/x-www-form-urlencoded"),
        BodyMode::Raw => Some("text/plain"),
        BodyMode::None => None,
    }
}

fn append_apikey_query(url: String, auth: &AuthConfig) -> String {
    if let AuthConfig {
        kind: AuthType::Apikey,
        apikey: Some(a),
        ..
    } = auth
    {
        if a.in_location == ApiKeyLocation::Query && !a.key.is_empty() {
            let glue = if url.contains('?') { '&' } else { '?' };
            return format!("{url}{glue}{}={}", url_encode(&a.key), url_encode(&a.value));
        }
    }
    url
}

fn append_query_string(mut url: String, params: &[(String, String)]) -> String {
    let mut first = !url.contains('?');
    for (k, v) in params {
        if k.is_empty() || v.is_empty() {
            continue;
        }
        let glue = if first { '?' } else { '&' };
        url.push(glue);
        url.push_str(&url_encode(k));
        url.push('=');
        url.push_str(&url_encode(v));
        first = false;
    }
    url
}

fn url_encode(s: &str) -> String {
    // Form-encoded per `application/x-www-form-urlencoded` (matches
    // `encodeURIComponent` on the TS side). Space → `+`, everything
    // outside the unreserved set → `%XX` upper-case hex.
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Base64-encode `user:pass` for HTTP Basic auth. Routes through
/// UTF-8 bytes first so credentials containing non-ASCII (e.g.
/// Chinese usernames, emoji passwords) encode correctly — matches
/// the browser convention that Express and most servers accept
/// (decode the header as raw bytes).
fn b64_basic(user: &str, pass: &str) -> String {
    let combined = format!("{user}:{pass}");
    base64_encode(combined.as_bytes())
}

/// Minimal, allocation-friendly base64 encoder. Standard alphabet
/// (`A-Z a-z 0-9 + /`), `=` padding. Used only for `Authorization:
/// Basic` so a hand-rolled 30-line encoder beats pulling in the
/// `base64` crate just for this.
fn base64_encode(input: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

// -------------------------------------------------------------------------
// resolve_endpoint — the headline API.
// -------------------------------------------------------------------------

/// Combine an endpoint + env + project + overrides into the
/// fully-resolved wire shape.
///
/// Resolution order (mirrors `buildFetchInit`):
///   1. Apply overrides (url / method / headers / params / body).
///   2. Resolve baseUrl via [`resolve_base_url_with`].
///   3. Substitute `{{var}}` everywhere (URL path, headers, params,
///      body content, auth fields), then collapse duplicate path
///      slashes (see [`collapse_url_slashes`] — the substitution is what
///      can turn the join's separator into a `//`).
///   4. Append enabled query params (encoded).
///   5. Append apikey query (if `auth.kind == Apikey && inLocation == Query`).
///   6. Build ordered header list (user headers + auto Content-Type
///      + auto Accept + auth-derived `Authorization`).
///   7. Decide body inclusion (only when `body.mode != None && content
///      non-empty`).
///
/// Returns `Ok(ResolvedEndpoint)` on success. The `missing_vars`
/// field carries the union of every `{{x}}` that didn't resolve —
/// callers decide whether to fail or warn.
///
/// The module is selected through `overrides.module_id` rather than a
/// parameter: the module is caller context, exactly like the caller's own
/// base-URL choice, and putting it here means every existing caller that
/// passes `None` keeps compiling — resolving to "no module, so no cell to
/// read", which is what an endpoint with no tree context deserves.
///
/// `AppError::Validation` is only returned for clearly malformed
/// input (e.g. negative size constraints). For the current send
/// path, no validation rules are enforced — the HTTP sender will
/// surface its own errors when it tries to use the result.
pub fn resolve_endpoint(
    detail: &EndpointDetail,
    env: Option<&Environment>,
    project: &Project,
    overrides: Option<&EndpointOverrides>,
) -> Result<ResolvedEndpoint, AppError> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record_missing =
        |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
            if seen.insert(var.clone(), ()).is_none() {
                missing.push(var);
            }
        };

    // ---- apply overrides (cheap copy of the relevant fields) ---------
    let mut eff_url = detail.url.clone();
    let mut eff_method = detail.method.clone();
    let mut eff_headers = detail.headers.clone();
    let mut eff_params = detail.params.clone();
    let mut eff_body = detail.body.clone();
    let eff_auth = detail.auth.clone();
    // The caller's pick wins over the endpoint's own definition; this is
    // what lets the renderer's unsaved base-URL selection take effect.
    let eff_def_id = overrides.and_then(|o| o.base_url_definition_id.clone());
    if let Some(o) = overrides {
        if let Some(u) = o.url.clone() {
            eff_url = u;
        }
        if let Some(m) = o.method.clone() {
            eff_method = m;
        }
        if let Some(h) = o.headers.clone() {
            eff_headers = h;
        }
        if let Some(p) = o.params.clone() {
            eff_params = p;
        }
        if let Some(b) = o.body.clone() {
            eff_body = b;
        }
    }

    // ---- variable pool ------------------------------------------------
    let vars: HashMap<String, String> = env.map(|e| e.variables.clone()).unwrap_or_default();

    // ---- params: substitute vars first, then build the URL append.
    // Doing this in two steps means missing-var tracking and the
    // params_out echo list share one source of truth.
    let mut params_subst: Vec<(String, String)> = Vec::with_capacity(eff_params.len());
    for p in &eff_params {
        if !p.enabled || p.key.is_empty() || p.value.is_empty() {
            continue;
        }
        let r = resolve_template(&p.value, &vars);
        for m in r.missing {
            record_missing(&mut missing, &mut seen, m);
        }
        params_subst.push((p.key.clone(), r.resolved));
    }

    // ---- baseUrl resolution + URL substitution ------------------------
    let (base_url, _src, _def_id) = resolve_base_url_with(
        detail,
        env,
        project,
        eff_def_id.as_deref(),
        overrides.and_then(|o| o.module_id.as_deref()),
    );
    let joined = join_base_url(&base_url, &eff_url);
    let url_r = resolve_template(&joined, &vars);
    for m in url_r.missing {
        record_missing(&mut missing, &mut seen, m);
    }
    // After substitution, never before: `{{var}}` → `/api` is what turns
    // the separator `join_base_url` inserted into a `//`, so this is the
    // first point where the real path is known. Query/fragment data is
    // left alone by `collapse_url_slashes`, which is why params can still
    // be appended after it.
    let url_norm = collapse_url_slashes(&url_r.resolved);

    // ---- query string append (uses already-substituted params) -------
    let url_with_params = append_query_string(url_norm, &params_subst);

    // ---- auth resolution ---------------------------------------------
    let (resolved_auth, auth_miss) = resolve_auth(&eff_auth, &vars);
    for m in auth_miss {
        record_missing(&mut missing, &mut seen, m);
    }

    // ---- apikey in query ---------------------------------------------
    let final_url = append_apikey_query(url_with_params, &resolved_auth);

    // ---- body inclusion + substitution --------------------------------
    let body = match &eff_body.mode {
        simple_post_core::domain::BodyMode::None => None,
        _ if eff_body.content.is_empty() => None,
        _ => {
            let b = resolve_template(&eff_body.content, &vars);
            for m in b.missing {
                record_missing(&mut missing, &mut seen, m);
            }
            Some(b.resolved)
        }
    };

    // ---- header ordering ---------------------------------------------
    let headers = build_headers(
        &eff_headers,
        &eff_body.mode,
        &resolved_auth,
        &vars,
        &mut missing,
    );

    Ok(ResolvedEndpoint {
        url: final_url,
        method: eff_method.as_str().to_string(),
        headers,
        params: params_subst,
        body,
        auth: resolved_auth,
        missing_vars: missing,
    })
}

// -------------------------------------------------------------------------
// Tests — the cases the brief explicitly calls out, plus a few more.
// -------------------------------------------------------------------------

#[cfg(test)]
#[path = "variable_fill_test.rs"]
mod tests;
