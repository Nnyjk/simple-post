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

The shared low-level layer (`db`, `domain`, `dynamic`, `error`,
`http_executor`, `mcp_metrics`, `oauth2`, `path_params`, `permissions`,
`script`, `spec`, `template`, `tls`, `url`) lives in the sibling
`simple-post-core` crate so both this crate and the main Tauri crate can
consume them without creating a cycle.

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
    ├── daemon_http.rs         # the daemon's axum listener
    ├── dynamic/               # dynamic variables: the shared operations
    │   ├── mod.rs             #   module map
    │   ├── ops.rs             #   the operations all three consumers share
    │   └── source.rs          #   the SourceFetcher implementation
    ├── spec/                  # the OpenAPI spec upsert tool
    │   ├── mod.rs
    │   └── ops.rs
    ├── variable_fill.rs       # facade over variable_fill/ (the interpolation rules)
    ├── variable_fill/         # the interpolation rules, one file per concern
    │   ├── auth.rs            #   the bearer / basic / apikey / oauth2 fields
    │   ├── base_url.rs        #   the base URL resolution + joining
    │   ├── request.rs         #   the header order + the query append
    │   ├── resolve.rs         #   resolve_endpoint (the main API)
    │   ├── template.rs        #   the `{{name}}` substitution (forwards to core)
    │   └── types.rs           #   EndpointOverrides / ResolvedEndpoint
    ├── response_parse.rs      # response parsing + diff + JSONPath extraction
    └── mcp/                   # the JSON-RPC dispatcher + tool implementations
        ├── mod.rs             # facade: AppState, dispatch, tool_descriptors,
        │                       # REQUIRED_CAPABILITY table, tool_fn! macro
        ├── protocol.rs        #   the wire shapes
        ├── registry.rs        #   tool_table() + REQUIRED_CAPABILITY
        ├── dispatch.rs        #   the JSON-RPC dispatcher
        ├── permissions.rs     #   the capability gate
        ├── args.rs            #   the JSON-RPC argument readers
        ├── state.rs           #   AppState + serve_http
        ├── http_ops/          # HTTP send + variable resolution tools
        │   ├── mod.rs         #   the module map
        │   ├── args.rs        #   the three tools' argument shapes
        │   ├── batch.rs       #   api_run_collection
        │   ├── call_endpoint.rs # api_call_endpoint
        │   ├── run_request.rs #   api_run_request
        │   ├── dynamic.rs     #   the dynamic-variable lookup
        │   ├── envelope.rs    #   the result envelope
        │   ├── input.rs       #   the argument flattening + parsing
        │   ├── multipart.rs   #   the multipart assembly
        │   ├── project.rs     #   the project row of an ad-hoc request
        │   ├── send.rs        #   the send path + the script env snapshot
        │   └── table.rs       #   the dispatch table (the three schemas)
        ├── project_ops/       # project / collection / module / endpoint /
        │                       # spec tools
        │   ├── endpoint.rs    #   facade over endpoint/
        │   ├── endpoint/      #   the endpoint tools, one file per concern
        │   │   ├── args.rs    #     the argument shapes
        │   │   ├── input.rs   #     the input coercion
        │   │   ├── missing.rs #     the "what is still missing" rule
        │   │   ├── patch.rs   #     the patch coercion
        │   │   ├── read.rs    #     the read handlers
        │   │   ├── registry.rs #    the JSON Schema
        │   │   ├── resource.rs #    the node + detail composite read
        │   │   └── write.rs   #     the write handlers
        │   ├── collection.rs
        │   ├── module.rs
        │   ├── project.rs
        │   └── spec.rs
        ├── env_ops.rs         # environment CRUD tools
        ├── dynamic_ops.rs     # dynamic-variable tools
        ├── response_parse.rs  # JSON-path response parsing tools
        ├── resources.rs       # the cross-tool guide
        ├── key_value.rs       # the shared KeyValue helpers
        └── variable_fill.rs   # variable interpolation tools
```

## CLI surface

```
simple-post-mcp
  --url=<SIMPLE_POST_WEB_URL>          (default: http://localhost:4173)
  --password=<SIMPLE_POST_WEB_PASSWORD>   (optional, sent as `Authorization: Bearer …`)
  --transport=stdio|http               (default: stdio)
  --port=<number>                       (only for --transport=http, default 4333)
  --capability=read_only|safe_write|full_access   (default: backend decides)
  --log-level=info|debug
```

Three of the six flags also accept an env-var override: `--url` reads
`SIMPLE_POST_WEB_URL`, `--password` reads `SIMPLE_POST_WEB_PASSWORD`, and
`--capability` reads `SIMPLE_POST_MCP_CAPABILITY`. The flag wins when both
are set. `--transport`, `--port` and `--log-level` have no env-var override.

## Clients with no native binary

There is no npm wrapper. A client machine that cannot run `simple-post-mcp`
connects straight to the daemon over HTTP instead — start
`simple-post serve-web` on the host and point the client at the endpoint:

```json
{
  "mcpServers": {
    "simple-post": {
      "type": "http",
      "url": "http://<host>:4173/mcp"
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