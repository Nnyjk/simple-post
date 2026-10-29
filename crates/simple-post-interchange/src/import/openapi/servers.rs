//! `servers[0].url` 的拆分：host 进模块的 server，basePath 留在 path 上。

use serde_json::Value;

use crate::diagnostics::Diagnostic;
use crate::ir::IrServer;

use super::{Ctx, DocServers};

/// 文档级 `servers[0]` → host + basePath。
pub(super) fn doc_servers(value: Option<&Value>, ctx: &mut Ctx) -> DocServers {
    let none = DocServers {
        hosts: Vec::new(),
        base_path: String::new(),
    };
    let Some(Value::Array(list)) = value else {
        if value.is_some() {
            ctx.diagnostics.push(Diagnostic::warn(
                "/servers",
                "servers 不是数组，已按没有服务器导入",
            ));
        }
        return none;
    };
    let Some(first) = list.first() else {
        return none;
    };
    let url = first.get("url").and_then(Value::as_str);
    let Some(url) = url else {
        ctx.diagnostics.push(Diagnostic::warn(
            "/servers/0",
            "servers[0] 没有 url，已按没有服务器导入",
        ));
        return none;
    };

    // 一个模块只能有一个服务，多服务器在这套模型里没有可写的位置。
    for extra in list.iter().skip(1) {
        ctx.diagnostics.push(
            Diagnostic::warn("/servers", "多服务器无法导入，只取第一个").dropping(
                extra
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("（没有 url）")
                    .to_string(),
            ),
        );
    }

    let (host, base_path) = split_server_url(url);
    let hosts = if host.is_empty() {
        ctx.diagnostics.push(Diagnostic::warn(
            "/servers/0",
            "server url 里没有 host，前缀保留在路径上",
        ));
        Vec::new()
    } else {
        vec![IrServer {
            service: super::service_name(&host),
            url: host,
        }]
    };
    DocServers { hosts, base_path }
}

/// `https://api.example.com/v2` → (`https://api.example.com`, `/v2`)。
/// 没有 host（`/v2` 这样的相对写法）时 host 为空串，basePath 照留。
fn split_server_url(url: &str) -> (String, String) {
    let trimmed = url.trim();
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    // scheme 属于 URL 而不属于路径，故 host 要带着它。
    let scheme_end = without_query.find("://").map(|at| at + 3).unwrap_or(0);
    let rest = &without_query[scheme_end..];
    let end = rest.find('/').unwrap_or(rest.len());
    (
        without_query[..scheme_end + end].to_string(),
        rest[end..].trim_end_matches('/').to_string(),
    )
}
