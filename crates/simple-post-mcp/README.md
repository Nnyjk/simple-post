# simple-post-mcp

MCP server module + standalone CLI binary for Simple Post.

Extracted from `src-tauri/src/mcp/` and `src-tauri/src/bin/mcp_proxy.rs`
during the workspace refactor. The standalone binary's CLI surface is
`--url` / `--password` / `--transport=stdio|http` / `--capability` /
`--log-level`.

The binary is the only distribution form — there is no npm wrapper. An AI
client that cannot run a Rust binary should connect to the daemon's HTTP
endpoint instead (start `simple-post serve-web`, point the client at
`http://<host>:<port>/mcp`), which needs nothing installed client-side.

The shared low-level layer (`db`, `domain`, `error`, `http_executor`,
`permissions`, `mcp_metrics`) lives in the sibling `simple-post-core`
crate so both this crate and the main Tauri crate can consume them
without creating a cycle.

## Layout

```
crates/simple-post-mcp/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # re-exports `pub mod mcp` + standalone modules
    ├── main.rs                # clap CLI entry: stdio loop OR --transport=http
    ├── error.rs               # ClientError + http_status_to_rpc_code (HTTP 401 → -32001)
    ├── http_client.rs         # reqwest::Client wrapper with optional bearer auth
    ├── stdio_loop.rs          # line-oriented JSON-RPC ↔ HTTP bridge
    ├── capability.rs          # --capability local tool-list filter
    ├── dynamic/               # dynamic variables: the shared operations
    │   ├── mod.rs             #   (ops.rs) + the SourceFetcher implementation
    │   ├── ops.rs             #   (source.rs)
    └── mcp/                   # the JSON-RPC dispatcher + tool implementations
        ├── mod.rs             # AppState, dispatch, tool_descriptors,
        │                       # REQUIRED_CAPABILITY table, tool_fn! macro
        ├── env_ops.rs         # environment CRUD tools
        ├── http_ops.rs        # HTTP send + variable resolution tools
        ├── project_ops/       # project / collection / module / endpoint /
        │                       # base_url_def CRUD tools
        ├── response_parse.rs  # JSON-path response parsing tools
        ├── dynamic_ops.rs     # dynamic-variable tools
        └── variable_fill.rs   # variable interpolation tools
```

## CLI surface

```
simple-post-mcp
  --url=<SIMPLE_POST_WEB_URL>          (default: http://localhost:4224)
  --password=<SIMPLE_POST_WEB_PASSWORD>   (optional, sent as `Authorization: Bearer …`)
  --transport=stdio|http               (default: stdio)
  --port=<number>                       (only for --transport=http, default 4333)
  --capability=read_only|safe_write|full_access   (default: backend decides)
  --log-level=info|debug
```

Every flag also accepts an env-var override (`SIMPLE_POST_WEB_URL` /
`SIMPLE_POST_WEB_PASSWORD` / `SIMPLE_POST_MCP_CAPABILITY`); the flag
wins when both are set.

## Clients with no native binary

There is no npm wrapper. A client machine that cannot run `simple-post-mcp`
connects straight to the daemon over HTTP instead — start
`simple-post serve-web` on the host and point the client at the endpoint:

```json
{
  "mcpServers": {
    "simple-post": {
      "type": "http",
      "url": "http://<host>:4224/mcp"
    }
  }
}
```

Nothing is installed on the client, and the transport is the same
JSON-RPC the stdio bridge speaks — it just replaces stdin/stdout with HTTP.

## Migration from `simple-post-mcp-proxy`

The `simple-post-mcp-proxy` binary that lived at
`src-tauri/src/bin/mcp_proxy.rs` has been removed. Replace your
`mcpServers` entry:

```diff
- {
-   "command": "simple-post-mcp-proxy"
- }
+ {
+   "command": "simple-post-mcp"
+ }
```

## See also

- `crates/simple-post-core/README.md` — the shared layer this crate
  builds on.
- `AGENTS.md` — the two-gate permission model (global capability policy + per-project permission)
  applied by `mcp::dispatch` / `mcp::tool_descriptors`.