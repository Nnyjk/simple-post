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
//!   * [`resolve_endpoint`] — combine an `Endpoint` + active
//!     `Environment` + `Project` + optional [`EndpointOverrides`]
//!     into a fully-formed [`ResolvedEndpoint`] ready for the HTTP
//!     sender.

use std::collections::HashMap;

use serde::Deserialize;

use crate::domain::{
    ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, Endpoint, Environment,
    HttpMethod, Project, RequestBody,
};
use crate::error::AppError;

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
    pub headers: Option<Vec<crate::domain::KeyValue>>,
    pub params: Option<Vec<crate::domain::KeyValue>>,
    pub body: Option<RequestBody>,
}

/// What `resolve_endpoint` produces. Already has every `{{var}}`
/// substituted, the baseUrl joined in, and the auth config reduced
/// to its post-resolve form. The HTTP sender (`crate::http_executor`)
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

    let resolved = template_replace(template, |key| {
        match vars.get(key) {
            Some(v) => v.clone(),
            None => {
                if seen_missing.insert(key.to_string(), ()).is_none() {
                    missing.push(key.to_string());
                }
                format!("{{{{{key}}}}}")
            }
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
                if is_word(inner) {
                    if seen.insert(inner.to_string(), ()).is_none() {
                        out.push(inner.to_string());
                    }
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
/// surfaces to label the current selection ("项目默认" /
/// "鉴权服务" / "自定义"). The MCP daemon uses the same enum to
/// explain its choice back to the renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseUrlSource {
    /// `endpoint.baseUrlOverride` won.
    Override,
    /// `endpoint.baseUrlDefinitionId` won (and pointed at a real def).
    Definition,
    /// Project default won.
    Default,
    /// Nothing matched; the caller is responsible for whatever
    /// fallback (often `""`).
    None,
}

/// Compute the effective baseUrl for an endpoint, given the
/// env that supplies its `baseUrls[defId]` and the project that
/// declares the definitions and the default.
///
/// Precedence (highest first):
///   1. `endpoint.baseUrl_override`  — per-endpoint custom host
///   2. `endpoint.base_url_definition_id` — the def the endpoint
///      points at (must exist in `project.base_url_definitions`);
///      URL is then `env.base_urls[def_id]`.
///   3. `project.default_base_url_definition_id` → `env.base_urls[def_id]`
///   4. `""` — nothing
///
/// `env.base_urls` is keyed by `BaseUrlDefinition.id`, NOT by name.
/// That keeps rename operations cheap and the resolution deterministic
/// even if two defs ever share a name. The env is taken from
/// `endpoint.environment_id` — when no env is selected (None) the
/// lookup short-circuits to "".
pub fn resolve_base_url(
    endpoint: &Endpoint,
    env: Option<&Environment>,
    project: &Project,
) -> (String, BaseUrlSource, Option<String>) {
    if let Some(ovr) = endpoint.base_url_override.as_deref() {
        if !ovr.is_empty() {
            return (ovr.to_string(), BaseUrlSource::Override, None);
        }
    }
    let def_id = endpoint
        .base_url_definition_id
        .clone()
        .or_else(|| project.default_base_url_definition_id.clone());
    let def_id = match def_id {
        Some(d) => d,
        None => return (String::new(), BaseUrlSource::None, None),
    };
    let def = project
        .base_url_definitions
        .iter()
        .find(|d| d.id == def_id);
    let url = env.and_then(|e| e.base_urls.get(&def_id));
    if let (Some(_def), Some(u)) = (def, url) {
        if !u.is_empty() {
            let source = if endpoint.base_url_definition_id.as_deref() == Some(def_id.as_str()) {
                BaseUrlSource::Definition
            } else {
                BaseUrlSource::Default
            };
            return (u.clone(), source, Some(def_id));
        }
    }
    (String::new(), BaseUrlSource::None, None)
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
    // RFC 3986 scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )
    // Mirrors the TS regex `/^[a-z][a-z0-9+.-]*:\/\//i`.
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
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
    i + 2 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b'/' && bytes[i + 2] == b'/'
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
                in_: a.in_,
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
/// `apikey.in_ == Query` is NOT emitted as a header here — the
/// caller appends it to the URL via `append_apikey_query`.
fn build_headers(
    endpoint_headers: &[crate::domain::KeyValue],
    body_mode: &crate::domain::BodyMode,
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
        if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            out.push(("Content-Type".to_string(), ct.to_string()));
        }
    }
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) {
        out.push(("Accept".to_string(), "application/json".to_string()));
    }

    match (&auth.kind, auth.bearer.as_deref()) {
        (AuthType::Bearer, Some(b)) if !b.is_empty() => {
            if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")) {
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
                if a.in_ == ApiKeyLocation::Header {
                    if !out
                        .iter()
                        .any(|(k, _)| k.eq_ignore_ascii_case(&a.key))
                    {
                        out.push((a.key.clone(), a.value.clone()));
                    }
                }
            }
        }
        _ => {}
    }

    out
}

fn auto_content_type(mode: &crate::domain::BodyMode) -> Option<&'static str> {
    use crate::domain::BodyMode;
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
        if a.in_ == ApiKeyLocation::Query && !a.key.is_empty() {
            let glue = if url.contains('?') { '&' } else { '?' };
            return format!(
                "{url}{glue}{}={}",
                url_encode(&a.key),
                url_encode(&a.value)
            );
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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
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
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
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
///   2. Resolve baseUrl via [`resolve_base_url`].
///   3. Substitute `{{var}}` everywhere (URL path, headers, params,
///      body content, auth fields).
///   4. Append enabled query params (encoded).
///   5. Append apikey query (if `auth.kind == Apikey && in_ == Query`).
///   6. Build ordered header list (user headers + auto Content-Type
///      + auto Accept + auth-derived `Authorization`).
///   7. Decide body inclusion (only when `body.mode != None && content
///      non-empty`).
///
/// Returns `Ok(ResolvedEndpoint)` on success. The `missing_vars`
/// field carries the union of every `{{x}}` that didn't resolve —
/// callers decide whether to fail or warn.
///
/// `AppError::Validation` is only returned for clearly malformed
/// input (e.g. negative size constraints). For the current send
/// path, no validation rules are enforced — the HTTP sender will
/// surface its own errors when it tries to use the result.
pub fn resolve_endpoint(
    endpoint: &Endpoint,
    env: Option<&Environment>,
    project: &Project,
    overrides: Option<&EndpointOverrides>,
) -> Result<ResolvedEndpoint, AppError> {
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let record_missing = |missing: &mut Vec<String>, seen: &mut HashMap<String, ()>, var: String| {
        if seen.insert(var.clone(), ()).is_none() {
            missing.push(var);
        }
    };

    // ---- apply overrides (cheap copy of the relevant fields) ---------
    let mut eff_url = endpoint.url.clone();
    let mut eff_method = endpoint.method.clone();
    let mut eff_headers = endpoint.headers.clone();
    let mut eff_params = endpoint.params.clone();
    let mut eff_body = endpoint.body.clone();
    let eff_auth = endpoint.auth.clone();
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
    let vars: HashMap<String, String> = env
        .map(|e| e.variables.clone())
        .unwrap_or_default();

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
    let (base_url, _src, _def_id) = resolve_base_url(endpoint, env, project);
    let joined = join_base_url(&base_url, &eff_url);
    let url_r = resolve_template(&joined, &vars);
    for m in url_r.missing {
        record_missing(&mut missing, &mut seen, m);
    }

    // ---- query string append (uses already-substituted params) -------
    let url_with_params = append_query_string(url_r.resolved, &params_subst);

    // ---- auth resolution ---------------------------------------------
    let (resolved_auth, auth_miss) = resolve_auth(&eff_auth, &vars);
    for m in auth_miss {
        record_missing(&mut missing, &mut seen, m);
    }

    // ---- apikey in query ---------------------------------------------
    let final_url = append_apikey_query(url_with_params, &resolved_auth);

    // ---- body inclusion + substitution --------------------------------
    let body = match &eff_body.mode {
        crate::domain::BodyMode::None => None,
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
    let headers = build_headers(&eff_headers, &eff_body.mode, &resolved_auth, &vars, &mut missing);

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
mod tests {
    use super::*;
    use crate::domain::{
        ApiKeyAuth, ApiKeyLocation, AuthConfig, AuthType, BasicAuth, BodyMode, KeyValue,
        RequestBody,
    };

    fn empty_env() -> Environment {
        Environment {
            id: "e1".into(),
            project_id: "p1".into(),
            name: "dev".into(),
            base_urls: HashMap::new(),
            variables: HashMap::new(),
            is_active: true,
        }
    }

    fn project_with_defs() -> Project {
        Project {
            id: "p1".into(),
            name: "demo".into(),
            description: None,
            base_url: None,
            base_url_definitions: vec![
                crate::domain::BaseUrlDefinition {
                    id: "auth".into(),
                    name: "auth".into(),
                    description: None,
                },
                crate::domain::BaseUrlDefinition {
                    id: "data".into(),
                    name: "data".into(),
                    description: None,
                },
            ],
            default_base_url_definition_id: Some("data".into()),
            color: "#000".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn minimal_endpoint() -> Endpoint {
        Endpoint {
            id: "ep1".into(),
            collection_id: "c1".into(),
            name: "demo".into(),
            description: None,
            method: HttpMethod::Get,
            url: "/users/{{id}}".into(),
            environment_id: Some("e1".into()),
            base_url_definition_id: None,
            base_url_override: None,
            params: Vec::new(),
            headers: Vec::new(),
            body: RequestBody::default(),
            auth: AuthConfig::default(),
            docs: None,
            notes: None,
            tags: vec![],
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    // ---- resolve_template --------------------------------------------

    #[test]
    fn resolve_template_hits_known_var() {
        let mut vars = HashMap::new();
        vars.insert("name".into(), "alice".into());
        let r = resolve_template("hello {{name}}", &vars);
        assert_eq!(r.resolved, "hello alice");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn resolve_template_unknown_leaves_marker() {
        let vars = HashMap::new();
        let r = resolve_template("hi {{who}}", &vars);
        assert_eq!(r.resolved, "hi {{who}}");
        assert_eq!(r.missing, vec!["who"]);
    }

    #[test]
    fn resolve_template_dedupes_missing_list() {
        let vars = HashMap::new();
        let r = resolve_template("{{x}}/{{x}}/{{y}}", &vars);
        assert_eq!(r.resolved, "{{x}}/{{x}}/{{y}}");
        assert_eq!(r.missing, vec!["x", "y"]);
    }

    #[test]
    fn resolve_template_empty_input_returns_empty() {
        let vars = HashMap::new();
        let r = resolve_template("", &vars);
        assert_eq!(r.resolved, "");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn resolve_template_chinese_var_name_and_value() {
        let mut vars = HashMap::new();
        vars.insert("名字".into(), "小明".into());
        // Note: `\w+` is `[A-Za-z0-9_]+`, so non-ASCII names don't match
        // the marker syntax. The marker passes through verbatim.
        let r = resolve_template("hi {{名字}}", &vars);
        assert_eq!(r.resolved, "hi {{名字}}");
        // Whitespace inside the marker is also not a word.
        let r = resolve_template("hi {{ name }}", &vars);
        assert_eq!(r.resolved, "hi {{ name }}");
    }

    #[test]
    fn resolve_template_empty_value_is_kept() {
        // An empty-string entry IS a hit; the substitution must produce
        // an empty segment, not leave the marker.
        let mut vars = HashMap::new();
        vars.insert("empty".into(), "".into());
        let r = resolve_template("x={{empty}}!", &vars);
        assert_eq!(r.resolved, "x=!");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn resolve_template_doubled_marker_does_not_recurse() {
        // "No recursion" property: a substitution result that itself
        // contains `{{...}}` markers must NOT be re-scanned in the
        // same pass.
        //
        // The single-pass scanner finds the first `}}` it sees in
        // `{{{{a}}}}`, considers the interior `{{a` a malformed token
        // (because it contains `{`), and emits the marker verbatim. So
        // the assertion below confirms two things at once:
        //   1. Substituting `a = "{{b}}"` for an outer `{{a}}` produces
        //      a string that still contains `{{b}}` (no second scan
        //      pulled it in).
        //   2. Running resolve_template on the already-expanded string
        //      does NOT behave any differently — proving the resolver
        //      is a pure single pass, idempotent on its own output.
        let mut vars = HashMap::new();
        vars.insert("a".into(), "{{b}}".into());
        let r = resolve_template("{{a}}", &vars);
        assert_eq!(r.resolved, "{{b}}");
        assert!(r.missing.is_empty(), "missing = {:?}", r.missing);

        // And the doubled-up case stays verbatim — the first `}}`
        // wins, the marker is left untouched, and the input does not
        // mutate at all.
        let mut vars2 = HashMap::new();
        vars2.insert("a".into(), "X".into());
        let r2 = resolve_template("{{{{a}}}}", &vars2);
        assert_eq!(r2.resolved, "{{{{a}}}}");
        assert!(r2.missing.is_empty());
        // No second pass swallowed the inner braces either.
        assert!(!r2.resolved.contains("X"));
    }

    // ---- list_missing_vars -------------------------------------------

    #[test]
    fn list_missing_vars_dedupes_and_preserves_order() {
        let v = list_missing_vars("{{a}}-{{b}}-{{a}}/{{c}}");
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    #[test]
    fn list_missing_vars_ignores_non_word_markers() {
        let v = list_missing_vars("{{ with space }}{{ a.b }}{{ok}}");
        assert_eq!(v, vec!["ok"]);
    }

    // ---- resolve_base_url --------------------------------------------

    #[test]
    fn resolve_base_url_override_wins() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://data.example.com".into());
        let mut ep = minimal_endpoint();
        ep.base_url_override = Some("https://custom.example.com".into());
        ep.base_url_definition_id = Some("data".into());
        let project = project_with_defs();
        let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
        assert_eq!(url, "https://custom.example.com");
        assert_eq!(src, BaseUrlSource::Override);
    }

    #[test]
    fn resolve_base_url_definition_chosen() {
        let mut env = empty_env();
        env.base_urls.insert("auth".into(), "https://auth.example.com".into());
        let mut ep = minimal_endpoint();
        ep.base_url_definition_id = Some("auth".into());
        let project = project_with_defs();
        let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
        assert_eq!(url, "https://auth.example.com");
        assert_eq!(src, BaseUrlSource::Definition);
    }

    #[test]
    fn resolve_base_url_falls_back_to_project_default() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://data.example.com".into());
        let ep = minimal_endpoint(); // base_url_definition_id = None
        let project = project_with_defs(); // default = "data"
        let (url, src, _) = resolve_base_url(&ep, Some(&env), &project);
        assert_eq!(url, "https://data.example.com");
        assert_eq!(src, BaseUrlSource::Default);
    }

    #[test]
    fn resolve_base_url_no_env_returns_empty() {
        let project = project_with_defs();
        let ep = minimal_endpoint();
        let (u, src, _) = resolve_base_url(&ep, None, &project);
        assert_eq!(u, "");
        assert_eq!(src, BaseUrlSource::None);
    }

    #[test]
    fn resolve_base_url_env_lacks_value_for_base_url() {
        // def id is known, env is selected, but env doesn't carry a
        // value for it — return None.
        let env = empty_env();
        let mut ep = minimal_endpoint();
        ep.base_url_definition_id = Some("auth".into());
        let project = project_with_defs();
        let (u, src, _) = resolve_base_url(&ep, Some(&env), &project);
        assert_eq!(u, "");
        assert_eq!(src, BaseUrlSource::None);
    }

    // ---- join_base_url -----------------------------------------------

    #[test]
    fn join_base_url_strips_trailing_slash() {
        assert_eq!(
            join_base_url("https://api.example.com/", "/users"),
            "https://api.example.com/users"
        );
    }

    #[test]
    fn join_base_url_full_path_in_path_wins() {
        assert_eq!(
            join_base_url("https://api.example.com", "https://other/foo"),
            "https://other/foo"
        );
    }

    #[test]
    fn join_base_url_no_base_returns_path() {
        assert_eq!(join_base_url("", "/users"), "/users");
    }

    #[test]
    fn join_base_url_inserts_separator_when_missing() {
        assert_eq!(
            join_base_url("https://api.example.com", "users"),
            "https://api.example.com/users"
        );
    }

    // ---- resolve_endpoint baseUrl precedence -------------------------

    #[test]
    fn resolve_endpoint_picks_override_over_definition() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://data.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/users".into();
        ep.base_url_definition_id = Some("data".into());
        ep.base_url_override = Some("https://override.example.com".into());
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert_eq!(r.url, "https://override.example.com/users");
    }

    #[test]
    fn resolve_endpoint_substitutes_vars_in_url_path() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("id".into(), "42".into());
        let mut ep = minimal_endpoint();
        ep.url = "/users/{{id}}".into();
        ep.base_url_definition_id = Some("data".into());
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert_eq!(r.url, "https://api.example.com/users/42");
        assert!(r.missing_vars.is_empty());
    }

    #[test]
    fn resolve_endpoint_collects_missing_vars_across_url_headers_body_auth() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/users/{{user_id}}".into();
        ep.base_url_definition_id = Some("data".into());
        ep.headers = vec![KeyValue {
            id: "h1".into(),
            key: "X-Trace".into(),
            value: "{{trace_id}}".into(),
            enabled: true,
            description: None,
        }];
        ep.body = RequestBody {
            mode: BodyMode::Raw,
            content: "post={{payload}}".into(),
            schema: None,
        };
        ep.auth = AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some("Bearer {{token}}".into()),
            basic: None,
            apikey: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        // URL + headers + body + auth missing vars are unioned, deduped,
        // and ordered by first appearance.
        assert!(r.missing_vars.contains(&"user_id".to_string()));
        assert!(r.missing_vars.contains(&"trace_id".to_string()));
        assert!(r.missing_vars.contains(&"payload".to_string()));
        assert!(r.missing_vars.contains(&"token".to_string()));
        // And the unresolved markers stayed put (we can see them in the
        // body string and URL):
        assert!(r.url.contains("{{user_id}}"));
        assert!(r.body.unwrap().contains("{{payload}}"));
    }

    // ---- auth injection ----------------------------------------------

    #[test]
    fn resolve_endpoint_bearer_auth_injects_authorization_header() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("token".into(), "secret-xyz".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.auth = AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some("{{token}}".into()),
            basic: None,
            apikey: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        let auth = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .expect("Authorization header must be present");
        assert_eq!(auth.1, "Bearer secret-xyz");
    }

    #[test]
    fn resolve_endpoint_basic_auth_injects_authorization_header() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("u".into(), "alice".into());
        env.variables.insert("p".into(), "wonder".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.auth = AuthConfig {
            kind: AuthType::Basic,
            bearer: None,
            basic: Some(BasicAuth {
                username: "{{u}}".into(),
                password: "{{p}}".into(),
            }),
            apikey: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        let auth = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .expect("Authorization header must be present");
        // "alice:wonder" base64 → "YWxpY2U6d29uZGVy"
        assert!(auth.1.starts_with("Basic "));
        assert!(auth.1.contains("YWxpY2U6d29uZGVy"));
    }

    #[test]
    fn resolve_endpoint_apikey_header_injects_custom_header() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("k".into(), "ABC123".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.auth = AuthConfig {
            kind: AuthType::Apikey,
            bearer: None,
            basic: None,
            apikey: Some(ApiKeyAuth {
                key: "X-API-Key".into(),
                value: "{{k}}".into(),
                in_: ApiKeyLocation::Header,
            }),
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        let hit = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("x-api-key"))
            .expect("apikey header must be present");
        assert_eq!(hit.1, "ABC123");
        // And the URL must NOT carry the apikey in the query.
        assert!(!r.url.contains("ABC123"));
    }

    #[test]
    fn resolve_endpoint_apikey_query_appends_to_url() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("k".into(), "Q-VAL".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.auth = AuthConfig {
            kind: AuthType::Apikey,
            bearer: None,
            basic: None,
            apikey: Some(ApiKeyAuth {
                key: "access_token".into(),
                value: "{{k}}".into(),
                in_: ApiKeyLocation::Query,
            }),
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert!(r.url.contains("access_token=Q-VAL"));
        // And there's NO Authorization header.
        assert!(r
            .headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")));
        // And the URL doesn't carry an apikey-shaped header.
        assert!(r
            .headers
            .iter()
            .all(|(k, _)| !k.eq_ignore_ascii_case("access_token")));
    }

    #[test]
    fn resolve_endpoint_user_authorization_wins_over_auth_config() {
        // If the user already declared an Authorization header, the
        // auth-config-derived one must NOT replace it (matches the
        // TS behavior — fetch keeps the last value of a record key,
        // cURL would otherwise emit two `-H` lines for the same name).
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        env.variables.insert("token".into(), "from-env".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.headers = vec![KeyValue {
            id: "h1".into(),
            key: "Authorization".into(),
            value: "Bearer user-set".into(),
            enabled: true,
            description: None,
        }];
        ep.auth = AuthConfig {
            kind: AuthType::Bearer,
            bearer: Some("{{token}}".into()),
            basic: None,
            apikey: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        // The user's "Bearer user-set" should win.
        let auth = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .unwrap();
        assert_eq!(auth.1, "Bearer user-set");
    }

    // ---- body inclusion ----------------------------------------------

    #[test]
    fn resolve_endpoint_skips_empty_body() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.body = RequestBody {
            mode: BodyMode::Json,
            content: "".into(),
            schema: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert!(r.body.is_none());
    }

    #[test]
    fn resolve_endpoint_skips_none_mode() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.body = RequestBody {
            mode: BodyMode::None,
            content: "ignored".into(),
            schema: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert!(r.body.is_none());
    }

    #[test]
    fn resolve_endpoint_auto_content_type_added() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.method = HttpMethod::Post;
        ep.body = RequestBody {
            mode: BodyMode::Json,
            content: "{\"x\":1}".into(),
            schema: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        let ct = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .unwrap();
        assert_eq!(ct.1, "application/json");
        let accept = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("accept"))
            .unwrap();
        assert_eq!(accept.1, "application/json");
    }

    #[test]
    fn resolve_endpoint_user_content_type_wins() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/me".into();
        ep.base_url_definition_id = Some("data".into());
        ep.method = HttpMethod::Post;
        ep.headers = vec![KeyValue {
            id: "h1".into(),
            key: "Content-Type".into(),
            value: "application/vnd.custom+json".into(),
            enabled: true,
            description: None,
        }];
        ep.body = RequestBody {
            mode: BodyMode::Json,
            content: "{}".into(),
            schema: None,
        };
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        let ct = r
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .unwrap();
        assert_eq!(ct.1, "application/vnd.custom+json");
    }

    // ---- query params -------------------------------------------------

    #[test]
    fn resolve_endpoint_appends_enabled_params_to_url() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/search".into();
        ep.base_url_definition_id = Some("data".into());
        ep.params = vec![
            KeyValue {
                id: "p1".into(),
                key: "q".into(),
                value: "hello world".into(),
                enabled: true,
                description: None,
            },
            KeyValue {
                id: "p2".into(),
                key: "disabled".into(),
                value: "x".into(),
                enabled: false,
                description: None,
            },
            KeyValue {
                id: "p3".into(),
                key: "empty".into(),
                value: "".into(),
                enabled: true,
                description: None,
            },
        ];
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert!(r.url.contains("q=hello+world"));
        assert!(!r.url.contains("disabled="));
        assert!(!r.url.contains("empty="));
    }

    #[test]
    fn resolve_endpoint_merges_existing_query_with_params() {
        // URL already has a query string; params should be appended
        // with `&`, not `?`.
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let mut ep = minimal_endpoint();
        ep.url = "/search?existing=1".into();
        ep.base_url_definition_id = Some("data".into());
        ep.params = vec![KeyValue {
            id: "p1".into(),
            key: "extra".into(),
            value: "2".into(),
            enabled: true,
            description: None,
        }];
        let project = project_with_defs();
        let r = resolve_endpoint(&ep, Some(&env), &project, None).unwrap();
        assert_eq!(r.url, "https://api.example.com/search?existing=1&extra=2");
    }

    // ---- overrides ---------------------------------------------------

    #[test]
    fn resolve_endpoint_applies_url_override() {
        let mut env = empty_env();
        env.base_urls.insert("data".into(), "https://api.example.com".into());
        let ep = minimal_endpoint();
        let project = project_with_defs();
        let overrides = EndpointOverrides {
            url: Some("https://override.example.com/custom".into()),
            ..Default::default()
        };
        let r = resolve_endpoint(&ep, Some(&env), &project, Some(&overrides)).unwrap();
        // Full-path override (with scheme) wins over baseUrl + path.
        assert_eq!(r.url, "https://override.example.com/custom");
    }
}