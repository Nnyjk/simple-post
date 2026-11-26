//! `/api/http/send` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "POST",
            path: "/api/http/send",
            summary: "Execute an arbitrary HTTP request (raw-URL escape hatch; \
                      honours the outbound URL policy)",
            request: Some("HttpRequestInput"),
            response: Some("HttpResult"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/http/send-progress",
            summary: "Execute a request and stream transfer progress as Server-Sent \
                      Events. The body is identical to /api/http/send. Each SSE \
                      `data:` line is an HttpSendProgressEvent JSON object: \
                      {type:'progress',progress:{sentBytes,totalBytes,\
                      serverReceivedBytes?,serverTotalBytes?}} while uploading or \
                      while the server reports its accepted bytes, then \
                      {type:'done',outcome} with the same envelope as \
                      /api/http/send, or {type:'error',message}. Aborts the request \
                      when the client disconnects.",
            request: Some("HttpSendBody"),
            response: Some("HttpSendProgressEvent"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/http/stream",
            summary: "Execute a request as a Server-Sent Events stream. The body is \
                      the same as /api/http/send plus `streamId`. Each SSE `data:` \
                      line is one StreamEvent JSON object: {type:'meta',status,\
                      statusText,headers,requestHeaders,contentType,url}, then \
                      {type:'chunk',seq,text,bytes} per body chunk, and finally \
                      {type:'end',durationMs,sizeBytes,truncated,reason} with \
                      reason eof|stopped|timeout, or {type:'error',kind,message} \
                      with kind timeout|network. Request scripts are not run.",
            request: Some("HttpStreamBody"),
            response: Some("StreamEvent"),
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/http/stream/{id}",
            summary: "Cancel an SSE stream started by POST /api/http/stream; emits \
                      {type:'end',reason:'stopped'} and closes the upstream \
                      connection. Idempotent (204 when unknown).",
            request: None,
            response: None,
        },
    ]
}
