use std::path::PathBuf;

use axum::{
    body::Body,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

// -------------------------------------------------------------------------
// Static SPA. Falls back to `index.html` so client-side routing works.
// -------------------------------------------------------------------------

/// Serves `dist/` produced by `npm run build`. The root path returns
/// `index.html`; anything under `/assets/` is served from disk via
/// `tower-http::services::ServeDir`.
///
/// `base` is the reverse-proxy prefix (`Some("/sp")`) or `None` when the
/// app is served at the root. See [`shell_with_base`] for why it has to
/// be threaded through here rather than left to the bundler.
pub(crate) async fn spa_index(spa_dir: PathBuf, base: Option<String>) -> Response {
    serve_file_or_index(spa_dir, "index.html", base).await
}

#[allow(dead_code)] // reserved for future asset-serve route; not yet wired in build_router
async fn spa_assets(
    axum::extract::State(spa_dir): axum::extract::State<PathBuf>,
    Path(rel): Path<String>,
) -> Response {
    // Reject any `..` to keep this a read-only static surface even
    // when bound to `0.0.0.0`.
    if rel.split('/').any(|seg| seg == "..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    serve_file_or_index(spa_dir, &rel, None).await
}

/// Read a file from `spa_dir`; on miss, return the SPA shell. Used
/// by `spa_index` (which always serves `index.html`) and by the
/// fallback handler for any unknown route.
async fn serve_file_or_index(spa_dir: PathBuf, rel: &str, base: Option<String>) -> Response {
    let path = spa_dir.join(rel);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            // Only the shell needs the prefix rewrite. An `/assets/*`
            // hit resolves to a real file whose own bytes are correct
            // regardless of how the URL that reached it was spelled.
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
                // SPA fallback: serve the shell, with the same base-path
                // treatment — a deep link (`/sp/some/route`) has to come
                // back with asset URLs the browser can actually resolve,
                // or a refresh on that URL yields a blank page.
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

/// The global the SPA reads its API prefix from. Kept in sync with
/// `readApiBase()` in `src/lib/tauri-commands.ts` — that function is the
/// only consumer, and it degrades to `""` if the name ever changes.
pub(crate) const BASE_GLOBAL: &str = "__SIMPLE_POST_BASE__";

/// Rewrite the SPA shell for a deployment under `base`.
///
/// # Why this exists at all
///
/// `vite.config.ts` sets no `base`, so `vite build` emits **root-absolute**
/// asset references (`<script src="/assets/index-<hash>.js">`) and the
/// renderer's `webFetch` calls `fetch('/api/…')` with no prefix. Both are
/// correct at the root and both break the moment the router is nested
/// under a prefix: the browser asks for `/assets/…` and `/api/…`, which a
/// router mounted at `/sp` never sees.
///
/// The prefix cannot be fixed at build time either — in the container it
/// arrives as the `SIMPLE_POST_PUBLIC_BASE_PATH` environment variable at
/// *run* time, and one image has to serve both shapes. So it is injected
/// here, per response, from the same `WebState.base_path` the router was
/// built with.
///
/// This also retires the `nest_service("/assets", …)` workaround that used
/// to live in `build_router`: that republished the static tree at the bare
/// root so the absolute asset URLs would resolve. It fixed the `<script>`
/// tag and nothing else — every `fetch('/api/…')` still 404'd — and it did
/// so by serving assets *outside* the prefix the operator asked for.
/// Correcting the shell makes it dead weight, so it is gone.
///
/// # What is rewritten
///
/// Root-absolute `src="…"` / `href="…"` values in the shell. In this app's
/// `index.html` that is `/favicon.svg` and `/assets/*`. Protocol-relative
/// (`//cdn…`) values are left alone. Inline JSON that merely *looks* like
/// an attribute is a theoretical false positive here; the shell has none,
/// and a full HTML parse is not worth it for one hand-maintained file.
pub(crate) fn shell_with_base(html: &[u8], base: Option<&str>) -> Vec<u8> {
    let base = base.map(str::trim).filter(|b| !b.is_empty());
    let Some(base) = base else {
        // Served at the root: pass the file through untouched. No global
        // is injected either, so the renderer keeps its root-absolute
        // behaviour and the two paths cannot drift.
        return html.to_vec();
    };

    let prefix = base.trim_end_matches('/');
    let text = String::from_utf8_lossy(html);
    let mut out = String::with_capacity(text.len() + prefix.len() + 64);

    for chunk in rewrite_root_absolute(&text, prefix) {
        out.push_str(&chunk);
    }

    // The renderer cannot discover the prefix on its own: `fetch('/api/…')`
    // and `import.meta.env.BASE_URL` (a build-time constant) both say
    // "root". Inject the value the server actually mounted at.
    let script = format!(
        "<script>window.{BASE_GLOBAL}={prefix:?};</script>\n</head>",
        prefix = prefix,
    );
    match out.rfind("</head>") {
        Some(i) => {
            out.insert_str(i, &script);
        }
        None => {
            // A shell without a </head> is not something we can patch in
            // place; append so the global is still defined. The asset
            // rewrite above already ran either way.
            out.push_str(&script);
        }
    }

    out.into_bytes()
}

/// Prefix every root-absolute `src` / `href` value with `prefix`.
///
/// Returns the rewritten text as a single-element vec so the caller does
/// not have to care about chunk boundaries.
fn rewrite_root_absolute(text: &str, prefix: &str) -> Vec<String> {
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;

    while i < text.len() {
        // Look for the opening of a `src="` / `href="` attribute.
        let rest = &text[i..];
        let (attr_len, quote_len) = if rest.starts_with("src=\"") {
            (4usize, 5usize)
        } else if rest.starts_with("href=\"") {
            (5, 6)
        } else {
            // Not an attribute start. Copy one char and rescan. Working
            // on chars (not bytes) keeps multi-byte content intact.
            let ch = rest.chars().next().expect("non-empty slice");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        let _ = attr_len;

        // The value starts right after the quote.
        let value_start = i + quote_len;
        let after = &text[value_start..];

        // Protocol-relative (`//host/…`) and already-prefixed values are
        // left alone; only a single leading `/` is ours to rewrite.
        let is_root_absolute = after.starts_with('/') && !after.starts_with("//");
        if is_root_absolute {
            // Emit `<prefix>/` and skip the source's own leading `/`.
            // Both slashes are written here on purpose: consuming the
            // source's `/` without re-emitting the separator yields
            // `/spassets/…`.
            out.push_str(&text[i..value_start]); // `src="` / `href="`
            out.push('/');
            out.push_str(prefix.trim_matches('/'));
            out.push('/');
            i = value_start + 1;
            continue;
        }

        // Not a root-absolute value. Copy the attribute opener verbatim
        // and rescan from the quote.
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
        // And nothing may be left pointing at the bare root.
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
        // Injected before </head> so it runs before the module script.
        let global = html.find("__SIMPLE_POST_BASE__").expect("injected");
        let end = html.find("</head>").expect("head terminator");
        assert!(global < end, "the global must precede the module entry");
    }

    /// A trailing slash is what an operator writes in a compose file
    /// (`SIMPLE_POST_PUBLIC_BASE_PATH=/sp/`). `nest` strips it, so the
    /// injected value must not carry it or every API URL gains a `//`.
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

    /// Non-ASCII must survive. The rewrite walks the string by `char`,
    /// not by byte — a byte-wise version would panic or corrupt here.
    #[test]
    fn multibyte_content_survives_the_rewrite() {
        let html =
            "<head><title>Simple Post 工具</title><script src=\"/assets/a.js\"></script></head>";
        let out = as_str(&shell_with_base(html.as_bytes(), Some("/sp")));
        assert!(out.contains("Simple Post 工具"));
        assert!(out.contains(r#"src="/sp/assets/a.js""#));
    }

    /// A shell with no `</head>` still gets the global rather than
    /// silently serving a renderer that will call the wrong URL.
    #[test]
    fn a_headless_shell_still_gets_the_global() {
        let out = as_str(&shell_with_base(b"<div id=root>", Some("/sp")));
        assert!(out.contains(r#"window.__SIMPLE_POST_BASE__="/sp";"#));
    }
}
