# simple-post-mcp

MCP server module + standalone CLI binary for Simple Post.

Extracted from `src-tauri/src/mcp/` and `src-tauri/src/bin/mcp_proxy.rs`
in commit 3 of the v0.2+ workspace refactor. The shared low-level layer
(`db`, `domain`, `error`, `http_executor`, `permissions`, `mcp_metrics`)
lives in the sibling `simple-post-core` crate so both this crate and
the main Tauri crate can consume them without creating a cycle.

## Layout

```
crates/simple-post-mcp/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # re-exports `pub mod mcp`
    ├── main.rs                # CLI placeholder (commit 4 replaces this)
    └── mcp/                   # the JSON-RPC dispatcher + tool implementations
        ├── mod.rs             # AppState, dispatch, tool_descriptors,
        │                       # REQUIRED_CAPABILITY table, tool_fn! macro
        ├── env_ops.rs         # environment CRUD tools
        ├── http_ops.rs        # HTTP send + variable resolution tools
        ├── project_ops/       # project / collection / module / endpoint /
        │                       # base_url_def CRUD tools
        ├── response_parse.rs  # JSON-path response parsing tools
        └── variable_fill.rs   # variable interpolation tools
```

## Status

| What                                       | State            |
| ------------------------------------------ | ---------------- |
| Workspace member compiles                  | yes (commit 3)   |
| `mcp` module re-exported from main crate   | yes (commit 3)   |
| Standalone CLI (`--url` / `--password`)    | **pending (commit 4)** |
| npm wrapper (`@simple-post/mcp-server`)    | **pending (commit 4)** |

## Migration from `simple-post-mcp-proxy`

Commit 3 removes the `simple-post-mcp-proxy` binary that lived at
`src-tauri/src/bin/mcp_proxy.rs`. Replace your `mcpServers` entry:

```diff
- {
-   "command": "simple-post-mcp-proxy"
- }
+ {
+   "command": "simple-post-mcp"
+ }
```

In commit 4 the new binary will additionally support:

```
simple-post-mcp
  --url=<SIMPLE_POST_WEB_URL>        (default: http://localhost:4224)
  --password=<SIMPLE_POST_WEB_PASSWORD>   (optional, sent as `Authorization: Bearer …`)
  --transport=stdio|http             (default: stdio)
  --capability=read_only|safe_write|full_access   (default: read_only)
```

Until then, the binary just prints a placeholder message and exits 2.

## See also

- `crates/simple-post-core/README.md` — the shared layer this crate
  builds on.
- `docs/mcp-permission-policy.md` — the three-tier capability policy
  applied by `mcp::dispatch` / `mcp::tool_descriptors`.
- `docs/db x-reference-2026-09-21.md` — the v0.2+ backlog that drove
  the extraction.