# simple-post-core

Shared low-level layer for Simple Post. Hosts the modules that the main
Tauri crate and the standalone `simple-post-mcp` crate both depend on:

```
crates/simple-post-core/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # re-exports db / domain / error /
    │                          # http_executor / mcp_metrics / permissions
    │                          # / script
    ├── db/                    # SQLite + CRUD modules
    │   ├── mod.rs             # init, with_conn, with_tx, schema
    │   ├── schema.sql         # CREATE TABLE statements (no rows)
    │   ├── fixtures.json      # test-only fixture data (#[cfg(test)])
    │   ├── base_url_defs.rs
    │   ├── collections.rs
    │   ├── endpoints.rs
    │   ├── environments.rs
    │   ├── modules.rs
    │   ├── node_scripts.rs    # collection-level pre-request / test scripts
    │   ├── permissions.rs
    │   ├── preferences.rs
    │   └── projects.rs
    ├── domain.rs              # Project / Permission / Endpoint / Environment / …
    ├── domain/                # one file per concern (script.rs holds the
    │                          # request-script wire types)
    ├── error.rs               # AppError enum
    ├── http_executor.rs       # reqwest-based HTTP executor (used by
    │                          # `http_request` Tauri command)
    ├── mcp_metrics.rs         # in-memory MCP daemon metrics
    ├── script/                # the request-script sandbox
    │   ├── mod.rs             # facade + the public surface
    │   ├── prelude.rs         # the fixed JS that defines the host API
    │   ├── engine.rs          # the QuickJS runtime, its limits, the
    │   │                      # interrupt handler
    │   ├── chain.rs           # assembling the chain + the env-write merge
    │   ├── run.rs             # one phase (pre-request or test)
    │   ├── send.rs            # the whole sequence — the only place it is
    │   │                      # written, so the desktop path and MCP cannot
    │   │                      # drift apart
    │   ├── limits.rs          # the timeout / memory / stack ceilings
    │   └── types.rs           # the sandbox's own shapes
    └── permissions.rs         # per-project Permission enum + global
                               # CapabilityPolicy (read_only/safe_write/
                               # full_access)
```

## Why this crate exists

The modules above used to live inside `src-tauri`. The MCP
module (`src-tauri::mcp`) consumed them via `use crate::db::*` /
`use crate::domain::*` / `use crate::error::*` / etc. Pulling the MCP
module into its own `simple-post-mcp` crate would have created a
cycle (mcp needs core types, main crate also needs both mcp and core
types). Hosting the shared modules in a third crate breaks the cycle.

## Consumers

- `src-tauri` (Tauri shell + `serve-web`): `pub use simple_post_core::*`
  re-exports the modules back under the historical `crate::db::*` /
  `crate::domain::*` paths so call sites don't need to be rewritten.
- `crates/simple-post-mcp` (standalone MCP): imports each module
  directly as `simple_post_core::db::*`, etc.

## The request-script sandbox

`script/` runs a user's JavaScript. It is here rather than in the
server or the MCP crate because **both** transports need it and the
sequence must be written once: the desktop path (`http_request` /
`POST /api/http/send`) and MCP (`api_call_endpoint`) both call
`script::send_with_scripts`, so they cannot drift apart on *where* the
script sits in the request's life.

Three properties are load-bearing:

- **The host surface is one fixed JS prelude.** No Rust callback is
  registered with the engine, so the only capabilities a script has are
  the ones that prelude hands over. "It cannot reach the filesystem, the
  network or a process" holds because that path does not exist.
- **The limits are real.** The interrupt handler makes a timeout an
  actual interruption (an uncatchable exception) rather than something
  we wait out; the memory ceiling needs the default allocator, so the
  `rust-alloc` / `allocator` features of `rquickjs` must stay off.
- **The module must not write to stdout and must not panic.** It is
  linked into the MCP daemon, whose stdout carries JSON-RPC frames
  only, and the release profile aborts on panic. `stdout_test.rs`
  enforces the first.