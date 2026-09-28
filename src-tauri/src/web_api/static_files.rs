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
pub(crate) async fn spa_index(spa_dir: PathBuf) -> Response {
    serve_file_or_index(spa_dir, "index.html").await
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
    serve_file_or_index(spa_dir, &rel).await
}

/// Read a file from `spa_dir`; on miss, return the SPA shell. Used
/// by `spa_index` (which always serves `index.html`) and by the
/// fallback handler for any unknown route.
async fn serve_file_or_index(spa_dir: PathBuf, rel: &str) -> Response {
    let path = spa_dir.join(rel);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => {
            if rel == "index.html" {
                (
                    StatusCode::NOT_FOUND,
                    "index.html missing — run `npm run build`",
                )
                    .into_response()
            } else {
                // SPA fallback: serve the shell. Same async fn, but
                // the call site avoids the recursion by inlining the
                // read here — that keeps the compiler happy without
                // dragging in `Box::pin`.
                let shell = spa_dir.join("index.html");
                match tokio::fs::read(&shell).await {
                    Ok(bytes) => {
                        let mime = mime_guess::from_path(&shell).first_or_octet_stream();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, mime.as_ref())
                            .body(Body::from(bytes))
                            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
