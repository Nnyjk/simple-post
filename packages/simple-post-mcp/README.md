# @simple-post/mcp-server

Standalone stdio ↔ HTTP bridge for [Simple Post](https://github.com/YOUR_ORG/simple-post).

Wraps the cargo-compiled `simple-post-mcp` native binary. No Rust toolchain
required — `npx -y @simple-post/mcp-server` ships a pre-built binary for
macOS / Linux / Windows on x64 and arm64.

## What it does

The native binary inside this package is the same one that the Simple Post
desktop app ships with. It reads JSON-RPC frames on stdin, forwards each
one to a running `simple-post serve-web` instance (HTTP), and writes the
response back on stdout. This is the protocol Claude Desktop, Cursor, and
every other MCP-aware AI client expect when they see an `stdio` server
entry.

The npm wrapper exists so the bridge can be installed / upgraded without
`cargo install simple-post-mcp` — useful when you don't want the Rust
toolchain on the machine that runs the AI client (e.g. CI runners, shared
hosts, end-user laptops that only need the bridge).

## Install + use

### Quick start

```bash
npm install -g @simple-post/mcp-server
```

Then point your MCP client (Claude Desktop / Cursor / etc.) at it:

```json
{
  "mcpServers": {
    "simple-post": {
      "command": "simple-post-mcp",
      "args": ["stdio"],
      "env": {
        "SIMPLE_POST_WEB_URL": "http://localhost:4224",
        "SIMPLE_POST_WEB_PASSWORD": "<if your server requires one>"
      }
    }
  }
}
```

### One-shot (no install)

```bash
npx -y @simple-post/mcp-server --url=http://localhost:4224 --transport=stdio
```

### Cross-machine setup

The native binary can talk to any reachable `simple-post serve-web`
instance, not just `localhost`. Point `--url` at a remote host:

```json
{
  "mcpServers": {
    "simple-post-remote": {
      "command": "npx",
      "args": ["-y", "@simple-post/mcp-server", "--url=http://B:4224"],
      "env": {
        "SIMPLE_POST_WEB_PASSWORD": "<bearer>"
      }
    }
  }
}
```

This is the dbx-style "MCP on A, web backend on B" deployment. The desktop app
serves the REST API (`simple-post serve-web`) while this package exposes the MCP
daemon to the AI client; both read the same `SIMPLE_POST_DATA_DIR`. See the
root `README.md` for the four distribution shapes.

| Flag          | Default                  | Notes |
| ------------- | ----------------------- | ----- |
| `--url`       | `http://localhost:4224` | also accepts `SIMPLE_POST_WEB_URL` |
| `--password`  | _none_                  | also accepts `SIMPLE_POST_WEB_PASSWORD`; sent as `Authorization: Bearer …` |
| `--transport` | `stdio`                 | `stdio` (default, line-oriented JSON-RPC) or `http` (exposes `/mcp` over HTTP for curl checks) |
| `--port`      | `4333`                  | only used by `--transport=http` |
| `--capability`| _backend default_       | also accepts `SIMPLE_POST_MCP_CAPABILITY`; `read_only` / `safe_write` / `full_access` |
| `--log-level` | `info`                  | `info` / `debug` |

## Troubleshooting

### `native binary missing at …/native/current/simple-post-mcp`

The `postinstall` hook didn't run (or failed silently). Try:

```bash
npm install --force @simple-post/mcp-server
```

…or fetch the binary directly: see `bin/postinstall.mjs` for the
platform/arch mapping.

### HTTP 401 errors

Wrong `--password` (or unset when the web backend requires one).
Use `--log-level=debug` to see exactly which status the bridge
received. The CLI surfaces it as JSON-RPC error code `-32001` so
AI clients can distinguish auth failures from generic -32000s.

### Linux: `GLIBC not found`

Build was done against a newer glibc than your system ships. Pin to a
musl build once the release workflow gains a `*-musl` target
(currently only the glibc binary ships). Track in the upstream issue
tracker.

## Source

Native binary lives in
[`crates/simple-post-mcp/`](https://github.com/YOUR_ORG/simple-post/tree/main/crates/simple-post-mcp).
This npm package is rebuilt and republished on every tagged release by
[`.github/workflows/release-mcp.yml`](https://github.com/YOUR_ORG/simple-post/tree/main/.github/workflows/release-mcp.yml).

## License

MIT — same as the parent project.