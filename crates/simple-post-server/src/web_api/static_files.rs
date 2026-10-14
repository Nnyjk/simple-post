use std::path::PathBuf;

use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

// -------------------------------------------------------------------------
// 静态 SPA。回落到 `index.html`，使前端路由可用。
// -------------------------------------------------------------------------

/// 提供 `npm run build` 产出的 `dist/`。根路径返回 `index.html`；`/assets/`
/// 下的内容由 `tower-http::services::ServeDir` 从磁盘提供。
///
/// `base` 是反代前缀（`Some("/sp")`）或 `None`（按根部署）。为何必须一路传
/// 到这里而不是交给打包器，见 [`shell_with_base`]。
pub(crate) async fn spa_index(spa_dir: PathBuf, base: Option<String>) -> Response {
    serve_file_or_index(spa_dir, "index.html", base).await
}

#[allow(dead_code)] // reserved for future asset-serve route; not yet wired in build_router
async fn spa_assets(
    axum::extract::State(spa_dir): axum::extract::State<PathBuf>,
    Path(rel): Path<String>,
) -> Response {
    // 拒绝任何 `..`，使这一层即使绑在 `0.0.0.0` 也只是只读静态面。
    if rel.split('/').any(|seg| seg == "..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    serve_file_or_index(spa_dir, &rel, None).await
}

/// 从 `spa_dir` 读一个文件；未命中则回 SPA shell。
async fn serve_file_or_index(spa_dir: PathBuf, rel: &str, base: Option<String>) -> Response {
    let path = spa_dir.join(rel);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            // 只有 shell 需要前缀改写。`/assets/*` 命中的是真实文件，无论 URL
            // 怎么拼，其字节都是对的。
            let bytes = if rel == "index.html" {
                shell_with_base(&bytes, base.as_deref())
            } else {
                bytes
            };
            respond(200, mime_for(&path), bytes)
        }
        Err(_) => {
            if rel == "index.html" {
                (
                    StatusCode::NOT_FOUND,
                    "index.html missing — run `npm run build`",
                )
                    .into_response()
            } else {
                // SPA 回落：同样做 base 处理——深链 `/sp/some/route` 必须带
                // 浏览器能解析的 asset URL，否则在该 URL 上刷新即白屏。
                let shell = spa_dir.join("index.html");
                match tokio::fs::read(&shell).await {
                    Ok(bytes) => {
                        let bytes = shell_with_base(&bytes, base.as_deref());
                        respond(200, mime_for(&shell), bytes)
                    }
                    Err(_) => (
                        StatusCode::NOT_FOUND,
                        "index.html missing — run `npm run build`",
                    )
                        .into_response(),
                }
            }
        }
    }
}

fn respond(status: u16, mime: String, body: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn mime_for(path: &std::path::Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .as_ref()
        .to_string()
}

/// SPA 读取 API 前缀所用的全局变量。与 `src/lib/tauri-commands.ts` 的
/// `readApiBase()` 配对——那是唯一的消费者，名字若变它退回 `""`。
pub(crate) const BASE_GLOBAL: &str = "__SIMPLE_POST_BASE__";

/// 按 `base` 改写 SPA shell。
///
/// # 为何必须有这一步
///
/// `vite.config.ts` 不设 `base`，故 `vite build` 产出**根绝对**的资源引用
/// （`<script src="/assets/index-<hash>.js">`），renderer 的 `webFetch` 也调
/// `fetch('/api/…')` 不带前缀。两者在根路径都对，一旦 router nest 到前缀下就
/// 全错：浏览器请求 `/assets/…` 与 `/api/…`，而挂在 `/sp` 的 router 看不到。
///
/// 前缀也无法在构建期定死：容器里它由运行时的
/// `SIMPLE_POST_PUBLIC_BASE_PATH` 决定，同一个镜像要同时服务两种形态。故在此
/// 按响应改写，取值来自 router 构造时的同一个 `WebState.base_path`。
///
/// # 改写什么
///
/// shell 中根绝对的 `src="…"` / `href="…"` 值。本应用的 `index.html` 里就是
/// `/favicon.svg` 与 `/assets/*`。协议相对（`//cdn…`）不动。仅形似属性的内联
/// JSON 是理论上的误判源，本 shell 没有，为一个人工维护的文件做完整 HTML 解析
/// 不划算。
pub(crate) fn shell_with_base(html: &[u8], base: Option<&str>) -> Vec<u8> {
    let base = base.map(str::trim).filter(|b| !b.is_empty());
    let Some(base) = base else {
        // 按根部署：原样透传，也不注入全局变量，renderer 保持根绝对行为，
        // 两条路径不会各走各的。
        return html.to_vec();
    };

    let prefix = base.trim_end_matches('/');
    let text = String::from_utf8_lossy(html);
    let mut out = String::with_capacity(text.len() + prefix.len() + 64);

    for chunk in rewrite_root_absolute(&text, prefix) {
        out.push_str(&chunk);
    }

    // renderer 自己发现不了前缀：`fetch('/api/…')` 与 `import.meta.env.BASE_URL`
    // （构建期常量）都指向「根」。注入服务器实际挂载的值。
    let script = format!(
        "<script>window.{BASE_GLOBAL}={prefix:?};</script>\n</head>",
        prefix = prefix,
    );
    match out.rfind("</head>") {
        Some(i) => {
            out.insert_str(i, &script);
        }
        None => {
            // 没有 `</head>` 的 shell 无法就地插入，改为追加，至少保证全局变量已定义。
            // 上面的 asset 改写无论如何都已执行。
            out.push_str(&script);
        }
    }

    out.into_bytes()
}

/// 给每个根绝对的 `src` / `href` 值加上 `prefix`。返回单元素 vec，调用方不必
/// 关心分块边界。
fn rewrite_root_absolute(text: &str, prefix: &str) -> Vec<String> {
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;

    while i < text.len() {
        // 找 `src="` / `href="` 属性的开头。
        let rest = &text[i..];
        let (attr_len, quote_len) = if rest.starts_with("src=\"") {
            (4usize, 5usize)
        } else if rest.starts_with("href=\"") {
            (5, 6)
        } else {
            // 不是属性开头：复制一个字符再重扫。按 `char` 而非字节推进，多字节内容才不被截断。
            let ch = rest.chars().next().expect("non-empty slice");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        let _ = attr_len;

        // 值紧跟引号之后。
        let value_start = i + quote_len;
        let after = &text[value_start..];

        // 协议相对（`//host/…`）与已带前缀的值不动；只有单个前导 `/` 归我们改写。
        let is_root_absolute = after.starts_with('/') && !after.starts_with("//");
        if is_root_absolute {
            // 两个斜杠都在这里写是有意的：吃掉源里的 `/` 却不补分隔符会得到
            // `/spassets/…`。
            out.push_str(&text[i..value_start]); // `src="` / `href="`
            out.push('/');
            out.push_str(prefix.trim_matches('/'));
            out.push('/');
            i = value_start + 1;
            continue;
        }

        // 不是根绝对值：原样复制属性开头，从引号处继续扫。
        out.push_str(&text[i..value_start]);
        i = value_start;
    }

    vec![out]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> &'static [u8] {
        br#"<!doctype html>
<html lang="zh-CN"><head>
<link rel="icon" type="image/svg+xml" href="/favicon.svg" />
<script type="module" crossorigin src="/assets/index-AbC123.js"></script>
<link rel="stylesheet" crossorigin href="/assets/index-AbC123.css" />
</head><body><div id="root"></div></body></html>"#
    }

    fn as_str(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("valid utf-8")
    }

    #[test]
    fn a_root_deployment_is_passed_through_byte_for_byte() {
        let out = shell_with_base(shell(), None);
        assert_eq!(out, shell());
        let out = shell_with_base(shell(), Some(""));
        assert_eq!(out, shell());
    }

    #[test]
    fn a_prefixed_deployment_rewrites_every_root_absolute_reference() {
        let html = as_str(&shell_with_base(shell(), Some("/sp")));
        assert!(
            html.contains(r#"src="/sp/assets/index-AbC123.js""#),
            "script src must carry the prefix:\n{html}"
        );
        assert!(
            html.contains(r#"href="/sp/assets/index-AbC123.css""#),
            "stylesheet href must carry the prefix:\n{html}"
        );
        assert!(
            html.contains(r#"href="/sp/favicon.svg""#),
            "icon href must carry the prefix:\n{html}"
        );
        // 不得有残留的裸根引用。
        assert!(
            !html.contains(r#""/assets/"#) && !html.contains(r#""/favicon.svg"#),
            "a root-absolute reference survived:\n{html}"
        );
    }

    #[test]
    fn the_prefix_reaches_the_renderer_through_the_injected_global() {
        let html = as_str(&shell_with_base(shell(), Some("/sp")));
        assert!(
            html.contains(r#"window.__SIMPLE_POST_BASE__="/sp";"#),
            "the renderer needs the prefix to build API URLs:\n{html}"
        );
        // 插在 `</head>` 之前，确保先于 module 脚本执行。
        let global = html.find("__SIMPLE_POST_BASE__").expect("injected");
        let end = html.find("</head>").expect("head terminator");
        assert!(global < end, "the global must precede the module entry");
    }

    /// compose 里常写 `SIMPLE_POST_PUBLIC_BASE_PATH=/sp/`。`nest` 会剥掉尾斜杠，
    /// 注入的值若仍带着，每个 API URL 都会多一个 `//`。
    #[test]
    fn a_trailing_slash_on_the_prefix_is_normalised() {
        let html = as_str(&shell_with_base(shell(), Some("/sp/")));
        assert!(html.contains(r#"src="/sp/assets/index-AbC123.js""#));
        assert!(
            html.contains(r#"window.__SIMPLE_POST_BASE__="/sp";"#),
            "prefix must be normalised, got:\n{html}"
        );
    }

    #[test]
    fn protocol_relative_urls_are_left_alone() {
        let html = r#"<head><script src="//cdn.example/x.js"></script></head>"#;
        let out = as_str(&shell_with_base(html.as_bytes(), Some("/sp")));
        assert!(
            out.contains(r#"src="//cdn.example/x.js""#),
            "a protocol-relative URL is not ours to prefix:\n{out}"
        );
    }

    /// 非 ASCII 必须存活：改写按 `char` 而非按字节推进，按字节的版本会在这里 panic
    /// 或损坏内容。
    #[test]
    fn multibyte_content_survives_the_rewrite() {
        let html =
            "<head><title>Simple Post 工具</title><script src=\"/assets/a.js\"></script></head>";
        let out = as_str(&shell_with_base(html.as_bytes(), Some("/sp")));
        assert!(out.contains("Simple Post 工具"));
        assert!(out.contains(r#"src="/sp/assets/a.js""#));
    }

    /// 没有 `</head>` 的 shell 仍要拿到全局变量，否则 renderer 会静默请求错 URL。
    #[test]
    fn a_headless_shell_still_gets_the_global() {
        let out = as_str(&shell_with_base(b"<div id=root>", Some("/sp")));
        assert!(out.contains(r#"window.__SIMPLE_POST_BASE__="/sp";"#));
    }
}
