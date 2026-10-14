// crates/simple-post-mcp/src/stdio_loop.rs
//
// `simple-post-mcp` 二进制的 stdio ↔ HTTP 桥。
//
// stdin 每行一帧 JSON-RPC，stdout 每帧回一行 —— Claude Desktop、Cursor
// 等 MCP 客户端说的就是这套协议。
//
// 出错不 panic：HTTP 失败与解析失败都转成 JSON-RPC `error` 块写回 stdout，
// 只有真正致命的 I/O 失败（stdin 管道关闭）才结束循环。
//
// 帧形态：
//   in:  {"jsonrpc":"2.0","id":N,"method":"tools/list","params":{}}
//   out: {"jsonrpc":"2.0","id":N,"result":{...}}
//        {"jsonrpc":"2.0","id":N,"error":{"code":N,"message":"..."}}
//        {"jsonrpc":"2.0","method":"...","params":{...}} (notifications, no id)
//
// `tools/list` 的结果先过一遍 `capability::filter_tools` 再写回，客户端看到的
// 就是「本地档位收窄之后」那份清单。

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
            // EOF，干净退出。
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
                writeln!(
                    out,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":msg}})
                )?;
                out.flush()?;
                continue;
            }
        };

        // 无 `id` 的 notification 只转发，不回响应帧。
        let id = frame.get("id").cloned();
        let method = frame.get("method").cloned().unwrap_or(Value::Null);
        let _params = frame.get("params").cloned().unwrap_or(Value::Null);

        // `tools/list` 取回全集后叠一层本地 capability 过滤；
        // 其他 method 原样代理。
        let response = if method == "tools/list" {
            let full = client.post_mcp(&frame).await;
            match full {
                Ok(mut v) => {
                    if let Some(result) = v.get_mut("result") {
                        if let Some(tools) = result.get_mut("tools") {
                            if let Some(arr) = tools.as_array_mut() {
                                let before = arr.len();
                                arr.retain(|t| {
                                    let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
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
                    Some(
                        json!({"jsonrpc":"2.0","id":id.clone().unwrap_or(Value::Null),"error":{"code":code,"message":msg}}),
                    )
                }
            }
        } else {
            match client.post_mcp(&frame).await {
                Ok(v) => Some(v),
                Err(e) => {
                    let (code, msg) = to_rpc_error(e);
                    Some(
                        json!({"jsonrpc":"2.0","id":id.clone().unwrap_or(Value::Null),"error":{"code":code,"message":msg}}),
                    )
                }
            }
        };

        // notification 不写响应行。
        if id.is_none() {
            continue;
        }
        if let Some(resp) = response {
            writeln!(out, "{}", resp)?;
            out.flush()?;
        }
    }
}
