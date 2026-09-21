# simple-post-mcp

MCP server module + standalone CLI binary for Simple Post.

Extracted from `src-tauri/src/mcp/` and `src-tauri/src/bin/mcp_proxy.rs`
in commit 3 of the v0.2+ workspace refactor. Commit 4 fills the
standalone binary's CLI surface (`--url` / `--password` /
`--transport=stdio|http` / `--capability` / `--log-level`) and adds the
npm wrapper package `@simple-post/mcp-server`.

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

## CLI surface (commit 4)

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

## npm wrapper

The same binary ships inside the npm package
[`@simple-post/mcp-server`](../../packages/simple-post-mcp/). The
release pipeline is in `.github/workflows/release-mcp.yml` and produces
one binary per (platform, arch); `bin/postinstall.mjs` symlinks the
right one into `native/current/`. Users get it with:

```bash
npx -y @simple-post/mcp-server --url=http://localhost:4224
```

…or, for a persistent install:

```bash
npm install -g @simple-post/mcp-server
```

## Migration from `simple-post-mcp-proxy`

The `simple-post-mcp-proxy` binary that lived at
`src-tauri/src/bin/mcp_proxy.rs` was removed in commit 3. Replace your
`mcpServers` entry:

```diff
- {
-   "command": "simple-post-mcp-proxy"
- }
+ {
+   "command": "simple-post-mcp"
+ }
```

Or, for one-shot installs with no native binary on disk:

```diff
- {
-   "command": "simple-post-mcp-proxy"
- }
+ {
+   "command": "npx",
+   "args": ["-y", "@simple-post/mcp-server"]
+ }
```

## See also

- `crates/simple-post-core/README.md` — the shared layer this crate
  builds on.
- `packages/simple-post-mcp/README.md` — the npm wrapper.
- `docs/mcp-permission-policy.md` — the three-tier capability policy
  applied by `mcp::dispatch` / `mcp::tool_descriptors`.