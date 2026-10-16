//! 一条 request 落成一个 endpoint：URL 拆成 path / params / 一台 server，其余字段各归各处。

use serde_json::{Map, Value};
use simple_post_core::domain::{parse_http_method, KeyValue};

use crate::diagnostics::Diagnostic;
use crate::ir::{IrEndpoint, IrServer};

use super::auth::parse_auth;
use super::body::parse_body;
use super::template::scan_templates;
use super::{description_text, field_text, flag, kv_list, Ctx};

pub(super) fn build_endpoint(
    req: &Map<String, Value>,
    name: String,
    ptr: &str,
    servers: &mut Vec<IrServer>,
    ctx: &mut Ctx,
) -> Option<IrEndpoint> {
    let Some(url_value) = req.get("url") else {
        ctx.diagnostics.push(Diagnostic::error(
            format!("{ptr}/request"),
            "request 没有 url，已跳过这一条",
        ));
        return None;
    };

    let method_text = field_text(req, "method").unwrap_or_default();
    let method = match parse_http_method(&method_text) {
        Ok(m) => m,
        Err(e) => {
            ctx.diagnostics.push(
                Diagnostic::error(
                    format!("{ptr}/request/method"),
                    format!("method 不可用（{e}），已跳过这一条"),
                )
                .dropping(method_text),
            );
            return None;
        }
    };

    let url = UrlParts::new(url_value, &format!("{ptr}/request/url"), ctx);
    if let Some(server) = &url.server {
        // 同一 host 在这个模块里出现多次只算一个服务。
        if !servers.iter().any(|s| s.url == server.url) {
            servers.push(server.clone());
        }
    }

    let endpoint = IrEndpoint {
        key: format!("{ptr}/request"),
        name,
        description: description_text(req.get("description")),
        method,
        path: url.path,
        params: url.params,
        headers: kv_list(req.get("header"), &format!("{ptr}/request/header"), ctx),
        body: parse_body(req, &format!("{ptr}/request/body"), ctx),
        auth: parse_auth(req, &format!("{ptr}/request/auth"), ctx),
        tags: Vec::new(),
        ..Default::default()
    };
    scan_templates(&endpoint, ptr, ctx);
    Some(endpoint)
}

// -------------------------------------------------------------------------
// URL —— 拆成 path + params + 一台 server
// -------------------------------------------------------------------------

/// URL 的三段落法：path-only 的路径、query 参数、以及一台服务。
struct UrlParts {
    path: String,
    params: Vec<KeyValue>,
    server: Option<IrServer>,
}

impl UrlParts {
    fn new(value: &Value, ptr: &str, ctx: &mut Ctx) -> Self {
        // `url` 可以是整条字符串，也可以是拆开的对象；后者才带 query[] 的 disabled 位。
        let obj = value.as_object();
        let raw = match value {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => field_text(o, "raw")
                .filter(|s| !s.is_empty())
                .or_else(|| rebuild_raw(o)),
            _ => None,
        };

        let Some(raw) = raw.filter(|r| !r.is_empty()) else {
            return Self {
                path: String::new(),
                params: Vec::new(),
                server: None,
            };
        };

        let (protocol, authority, rest) = split_authority(&raw);
        let rest = rest.split('#').next().unwrap_or("");
        let (path_part, query) = match rest.split_once('?') {
            Some((p, q)) => (p, q),
            None => (rest, ""),
        };
        // 对象形态的 `query[]` 带 disabled 位，比从 raw 切出来的字符串更可信。
        let params = obj
            .and_then(query_params)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| parse_query_string(query));

        match authority {
            // 整体占位（`{{baseUrl}}/users`）拆不出稳定的服务名，整串留在 path 里。
            Some(host) if host.contains("{{") => {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        ptr,
                        "host 是一个整体变量，拆不出稳定的服务名；整串 URL 保留在 path 里",
                    )
                    .dropping(raw.clone()),
                );
                Self {
                    path: raw,
                    params,
                    server: None,
                }
            }
            Some(host) if !host.is_empty() => Self {
                path: path_part.to_string(),
                params,
                server: Some(IrServer {
                    service: super::service_name(host),
                    url: match protocol.filter(|p| !p.is_empty()) {
                        Some(p) => format!("{p}://{host}"),
                        None => format!("//{host}"),
                    },
                }),
            },
            _ => {
                ctx.diagnostics.push(
                    Diagnostic::warn(
                        ptr,
                        "URL 里没有 host（相对地址或整体变量），无法生成服务；整串保留在 path 里",
                    )
                    .dropping(raw.clone()),
                );
                Self {
                    path: path_part.to_string(),
                    params,
                    server: None,
                }
            }
        }
    }
}

/// `protocol` + `host` + `path` → 一条 raw，供 [`split_authority`] 统一处理。
///
/// 缺 protocol 时按原样拼（没有 `://`），后续自然落进「相对地址」分支并出诊断。
pub(super) fn rebuild_raw(o: &Map<String, Value>) -> Option<String> {
    let host = match o.get("host") {
        Some(Value::Array(a)) => join_host(a),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let path = match o.get("path") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("/"),
        Some(Value::String(s)) => s.trim_start_matches('/').to_string(),
        _ => String::new(),
    };

    let mut raw = String::new();
    if let Some(p) = field_text(o, "protocol").filter(|p| !p.is_empty()) {
        raw.push_str(&p);
        raw.push_str("://");
    }
    raw.push_str(&host);
    if !path.is_empty() {
        if !raw.is_empty() && !host.is_empty() {
            raw.push('/');
        }
        raw.push_str(&path);
    }
    Some(raw)
}

/// host 数组拼回主机名。末位是纯数字时它其实是端口（`["localhost", "8080"]`），
/// 按域名用 `.` 连会得到一个不存在的主机。
fn join_host(arr: &[Value]) -> String {
    let parts: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
    match parts.split_last() {
        Some((last, head)) if !head.is_empty() && last.chars().all(|c| c.is_ascii_digit()) => {
            format!("{}:{}", head.join("."), last)
        }
        _ => parts.join("."),
    }
}

/// `(protocol, authority, 余下部分)`。没有 `://` 时 authority 为 `None`。
fn split_authority(raw: &str) -> (Option<&str>, Option<&str>, &str) {
    let Some(i) = raw.find("://") else {
        return (None, None, raw);
    };
    let rest = &raw[i + 3..];
    match rest.find(['/', '?']) {
        Some(j) => (Some(&raw[..i]), Some(&rest[..j]), &rest[j..]),
        None => (Some(&raw[..i]), Some(rest), ""),
    }
}

/// 对象形态的 `query[]`。
fn query_params(o: &Map<String, Value>) -> Option<Vec<KeyValue>> {
    let arr = o.get("query")?.as_array()?;
    Some(
        arr.iter()
            .map(|e| {
                let e = e.as_object();
                KeyValue {
                    key: e.and_then(|e| field_text(e, "key")).unwrap_or_default(),
                    value: e.and_then(|e| field_text(e, "value")).unwrap_or_default(),
                    enabled: !e.is_some_and(|e| flag(e, "disabled")),
                    ..Default::default()
                }
            })
            .collect(),
    )
}

/// raw 里切出来的 `a=1&b=2`。
fn parse_query_string(q: &str) -> Vec<KeyValue> {
    q.split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            KeyValue {
                key: k.to_string(),
                value: v.to_string(),
                enabled: true,
                ..Default::default()
            }
        })
        .collect()
}
