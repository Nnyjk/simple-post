# simple-post-core

Shared low-level layer for Simple Post. Hosts the modules that the headless
server crate, the main Tauri crate and the standalone `simple-post-mcp` crate
all depend on:

```
crates/simple-post-core/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                 # re-exports db / domain / dynamic / error /
    │                          # http_executor / mcp_metrics / oauth2 /
    │                          # path_params / permissions / script / spec /
    │                          # template / tls / url
    ├── db/                    # SQLite + CRUD modules
    │   ├── mod.rs             # the module map + init / ping / with_conn /
    │   │                      # with_tx / DbPool / migrate
    │   ├── pool.rs            # the connection pool + the transaction helpers
    │   ├── schema.rs          # migrate — the column backfill + obsolete drops
    │   ├── schema.sql         # CREATE TABLE statements (no rows)
    │   ├── fixtures.json      # test-only fixture data (#[cfg(test)])
    │   ├── migration_fixtures.rs  # test-only: the two retired service-table
    │   │                      #   shapes, rebuilt as rows
    │   ├── test_helpers.rs    # test-only: the shared pool + temp-dir builder
    │   ├── test_support.rs    # test-only: the only reader of fixtures.json
    │   ├── base_url_defs.rs   # the project-level service list
    │   ├── base_url_migration.rs
    │   ├── dynamic_variable_cache.rs
    │   ├── dynamic_variables/ # the per-project definition table
    │   │   ├── mod.rs         #   facade
    │   │   ├── read.rs        #   list / list_by_project / get
    │   │   │                  #   / get_by_name_in_project / get_by_name_across
    │   │   │                  #   / project_id_of / source_project_id_of
    │   │   ├── write.rs       #   create / update / delete
    │   │   ├── validation.rs  #   source bindings, name shape
    │   │   └── mapping.rs     #   row → domain
    │   ├── endpoint_details.rs
    │   ├── environments.rs
    │   ├── node_scripts.rs    # collection-level pre-request / test scripts
    │   ├── nodes.rs           # facade over nodes/
    │   ├── nodes/             # the whole resource tree
    │   │   ├── read.rs        #   get / list_all / list_by_root /
    │   │   │                  #   list_by_root_and_type / list_children /
    │   │   │                  #   module_id_of / root_id_of / search /
    │   │   │                  #   search_paths / subtree_ids / type_of
    │   │   ├── write.rs       #   create / update / delete / duplicate /
    │   │   │                  #   reparent
    │   │   ├── tree.rs        #   move_many, move_to_root
    │   │   └── placement.rs   #   the parent/child contract
    │   ├── oauth2_tokens.rs
    │   ├── permissions.rs
    │   ├── preferences.rs
    │   ├── projects.rs
    │   └── response_history.rs
    ├── domain.rs              # facade over domain/ — Project / Permission /
    │                          # Endpoint / Environment / …
    ├── domain/                # one file per concern
    │   ├── auth.rs            #   AuthConfig / AuthType / RequestBody / KeyValue
    │   ├── dynamic.rs         #   the dynamic-variable wire types
    │   ├── http.rs            #   HttpMethod
    │   ├── oauth2.rs          #   the OAuth2 wire types
    │   ├── project.rs         #   Project / Environment
    │   ├── resource.rs        #   ResourceNode / EndpointDetail
    │   ├── response.rs        #   ApiResponse / StoredResponse
    │   ├── script.rs          #   the request-script wire types
    │   ├── settings.rs        #   Permission / RequestSettings / TlsSettings
    │   └── util.rs            #   uid / now_ms / the small helpers
    ├── dynamic/               # the dynamic-variable engine, one file per
    │                          # concern (merge / serialize / names / cache /
    │                          # extract / source / flight / fetcher / resolve /
    │                          # transform)
    ├── error.rs               # AppError enum
    ├── http_executor.rs       # reqwest-based HTTP executor (used by
    │                          # `http_request` Tauri command)
    ├── mcp_metrics.rs         # in-memory MCP daemon metrics
    ├── oauth2.rs              # facade over oauth2/
    ├── oauth2/                # the OAuth2 token flow
    │   ├── api.rs             #   the entry both transports share
    │   ├── form.rs            #   the x-www-form-urlencoded decoding
    │   ├── http_client.rs     #   the `oauth2` crate's outbound adapter
    │   ├── inject.rs          #   putting the token into the request
    │   ├── parse.rs           #   the token-response parsing
    │   ├── store.rs           #   the `oauth2_tokens` cache
    │   ├── token_endpoint.rs  #   the one round trip with the token endpoint
    │   └── validate.rs        #   the required-field check per flow
    ├── path_params.rs         # the path-parameter recognition + substitution
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
    │   ├── transform.rs        # the dynamic-variable's transform script
    │   ├── limits.rs          # the timeout / memory / stack ceilings
    │   └── types.rs           # the sandbox's own shapes
    ├── spec/                  # the one-call interface specification
    │   ├── mod.rs             # the addressing primitives + the facade
    │   ├── input.rs           # the shared input coercion
    │   └── upsert/            # the transactional write
    │       ├── mod.rs         #   facade
    │       ├── types.rs       #   the wire shapes
    │       ├── resolve.rs     #   module / collection / service / environment
    │       └── write.rs       #   the endpoint write + the whole transaction
    ├── template.rs            # the `{{name}}` substitution primitives
    ├── tls.rs                 # the HTTPS outbound settings
    ├── url.rs                 # the percent-encoding, matching the renderer's
    │                          # encodeURIComponent byte for byte
    └── permissions.rs         # per-project Permission enum + global
                               # CapabilityPolicy (read_only/safe_write/
                               # full_access)
```

## Why this crate exists

The modules above live here rather than inside `src-tauri`. The MCP
module (`src-tauri::mcp`) consumed them via `use crate::db::*` /
`use crate::domain::*` / `use crate::error::*` / etc. Pulling the MCP
module into its own `simple-post-mcp` crate would have created a
cycle (mcp needs core types, main crate also needs both mcp and core
types). Hosting the shared modules in a third crate breaks the cycle.

## Consumers

- `crates/simple-post-server` (the headless host): `pub use simple_post_core::{db,
  domain, error, http_executor, mcp_metrics, oauth2, permissions, script, spec}`
  re-exports the modules under the historical `crate::db::*` / `crate::domain::*`
  paths so call sites don't need to be rewritten.
- `src-tauri` (the Tauri shell + `serve-web`): `pub use simple_post_core::{…}`
  re-exports the same list, and `pub use simple_post_server::{…}` forwards the
  host half — that is why `crate::daemon_supervisor` / `crate::web_api` still
  resolve inside the desktop crate.
- `crates/simple-post-mcp` (the standalone MCP): imports each module directly
  as `simple_post_core::db::*`, etc.

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