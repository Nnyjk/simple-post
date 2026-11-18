//! `/api/ws/sessions` 一组。

use super::super::RouteSpec;

pub(in crate::web_api::spec) fn routes() -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            method: "POST",
            path: "/api/ws/sessions",
            summary: "Open a WebSocket session. The response is a text/event-stream \
                      whose `data:` lines are WsEvent objects: {type:'state',state:\
                      'connecting'|'open'}, {type:'frame',direction:'in'|'out',kind:\
                      'text'|'binary',data? ,base64?,bytes,at} (at is epoch ms), \
                      {type:'closed',code|null,reason,at}, {type:'error',kind:\
                      'timeout'|'network'|'protocol',message}. Frames are not \
                      persisted; reconnect policy lives in the renderer.",
            request: Some("WsConnectBody"),
            response: Some("WsEvent"),
        },
        RouteSpec {
            method: "POST",
            path: "/api/ws/sessions/{id}/send",
            summary: "Send one frame on an open session: {kind:'text'|'binary', \
                      data?, base64?}. Returns 204; 404 when the session is gone.",
            request: Some("WsSendBody"),
            response: None,
        },
        RouteSpec {
            method: "DELETE",
            path: "/api/ws/sessions/{id}",
            summary: "Close a session. Optional ?code= and ?reason= go out in the \
                      close frame; the event stream then ends with {type:'closed'}.",
            request: None,
            response: None,
        },
    ]
}
