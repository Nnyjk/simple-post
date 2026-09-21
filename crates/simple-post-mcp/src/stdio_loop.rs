// crates/simple-post-mcp/src/stdio_loop.rs
//
// Synchronous stdio ↔ HTTP bridge for the `simple-post-mcp` binary.
//
// One JSON-RPC frame per line on stdin; one line of response per
// frame on stdout. Matches the protocol every existing MCP client
// (Claude Desktop, Cursor, …) speaks, so the binary is a drop-in
// replacement for the previous `simple-post-mcp-proxy`.
//
// Errors don't panic: HTTP failures and parse errors are converted
// to JSON-RPC `error` blocks and written back on stdout. Only
// truly fatal I/O failures (stdin pipe closed) end the loop.
//
// Frame shape:
//   in:  {"jsonrpc":"2.0","id":N,"method":"tools/list","params":{}}
//   out: {"jsonrpc":"2.0","id":N,"result":{...}}
//        {"jsonrpc":"2.0","id":N,"error":{"code":N,"message":"..."}}
//        {"jsonrpc":"2.0","method":"...","params":{...}} (notifications, no id)
//
// `tools/list` responses are filtered through `capability::filter_tools`
// before being written back, so the client sees the same visibility
// as if it had hit `/mcp` directly with a narrower `mcp.policy`.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::capability::{filter_tools, CapabilityOverride};
use crate::error::ClientError;
use crate::http_client::{to_rpc_error, WebClient};

pub async fn run(
    client: WebClient,
    cap_override: CapabilityOverride,
    mut log: impl Write,
) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut reader = stdin.lock();

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // EOF — clean shutdown.
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let frame: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let (code, msg) = to_rpc_error(ClientError::BadFrame(e.to_string()));
                writeln!(out, "{}", json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":msg}}))?;
                out.flush()?;
                continue;
            }
        };

        // Notifications (no `id`) get forwarded but never get a
        // response written — matching JSON-RPC 2.0.
        let id = frame.get("id").cloned();
        let method = frame.get("method").cloned().unwrap_or(Value::Null);
        let params = frame.get("params").cloned().unwrap_or(Value::Null);

        // For `tools/list`, fetch the full set and apply our local
        // capability filter on top of whatever the backend returned.
        // For every other method, we just proxy the frame as-is.
        let response = if method == "tools/list" {
            let full = client.post_mcp(&frame).await;
            match full {
                Ok(mut v) => {
                    if let Some(result) = v.get_mut("result") {
                        if let Some(tools) = result.get_mut("tools") {
                            if let Some(arr) = tools.as_array_mut() {
                                let before = arr.len();
                                arr.retain(|t| {
                                    let name = t
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("");
                                    filter_tools(name, cap_override)
                                });
                                let after = arr.len();
                                if before != after {
                                    let _ = writeln!(
                                        log,
                                        "[simple-post-mcp] tools/list filtered: {} -> {} (capability={:?})",
                                        before, after, cap_override
                                    );
                                }
                            }
                        }
                    }
                    Some(v)
                }
                Err(e) => {
                    let (code, msg) = to_rpc_error(e);
                    Some(json!({"jsonrpc":"2.0","id":id.clone().unwrap_or(Value::Null),"error":{"code":code,"message":msg}}))
                }
            }
        } else {
            match client.post_mcp(&frame).await {
                Ok(v) => Some(v),
                Err(e) => {
                    let (code, msg) = to_rpc_error(e);
                    Some(json!({"jsonrpc":"2.0","id":id.clone().unwrap_or(Value::Null),"error":{"code":code,"message":msg}}))
                }
            }
        };

        // Notifications get no response line.
        if id.is_none() {
            continue;
        }
        if let Some(resp) = response {
            writeln!(out, "{}", resp)?;
            out.flush()?;
        }
    }
}