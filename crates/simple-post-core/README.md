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
    ├── db/                    # SQLite + CRUD modules
    │   ├── mod.rs             # init, with_conn, with_tx, schema
    │   ├── schema.sql         # CREATE TABLE statements (no rows)
    │   ├── fixtures.json      # test-only fixture data (#[cfg(test)])
    │   ├── base_url_defs.rs
    │   ├── collections.rs
    │   ├── endpoints.rs
    │   ├── environments.rs
    │   ├── modules.rs
    │   ├── permissions.rs
    │   ├── preferences.rs
    │   └── projects.rs
    ├── domain.rs              # Project / Permission / Endpoint / Environment / …
    ├── error.rs               # AppError enum
    ├── http_executor.rs       # reqwest-based HTTP executor (used by
    │                          # `http_request` Tauri command)
    ├── mcp_metrics.rs         # in-memory MCP daemon metrics
    └── permissions.rs         # per-project Permission enum + global
                               # CapabilityPolicy (read_only/safe_write/
                               # full_access)
```

## Why this crate exists

Before commit 3 the modules above lived inside `src-tauri`. The MCP
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