//! `/api/interchange/*` 一组。
//!
//! 两条边各两步：**先预览后落定**。预览是只读的，它回答「这份文件里有哪些条目、
//! 有哪些已经在项目里了、有哪些构造会丢」，用户勾完才提交。导出侧对称：
//! 先看候选清单（可选环境），再产文档。
//!
//! `import/preview` 的 URL 模式是**唯一一条能读任意用户指定地址的通道**，
//! 与 `/api/oauth2/token` 同族，故四条都不在 `is_public_path` 名单里。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "POST",
            path: "/api/interchange/import/preview",
            summary: "Parse a Postman collection or an OpenAPI document into a \
                      selectable intermediate form; writes nothing. Either `content` \
                      or `url`; URL mode echoes the fetched text back as \
                      `resolvedContent` so the commit does not fetch twice",
            stability: "public",
            request: Some("ImportPreviewOpts"),
            response: Some("ImportPreviewDto"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/interchange/import/commit",
            summary: "Re-parse the same content and land the endpoints whose keys \
                      are in `selected` under the given module, together with the \
                      source environments, in one transaction",
            stability: "public",
            request: Some("ImportCommitOpts"),
            response: Some("ImportCommitDto"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/interchange/export/preview",
            summary: "List a module's subtree as export candidates, with the \
                      per-module service URLs resolved under the named environment, \
                      or under the project's default environment when none is named",
            stability: "public",
            request: Some("ExportPreviewOpts"),
            response: Some("ExportPreviewDto"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/interchange/export/document",
            summary: "Render the selected endpoints as an OpenAPI 3.0 document in \
                      the requested format; reads only, returns the document text",
            stability: "public",
            request: Some("ExportDocumentOpts"),
            response: Some("ExportDocumentDto"),
        },
    ]
}
