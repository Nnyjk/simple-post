# simple-post-server

The headless host of Simple Post. This crate owns everything the desktop
shell and the container image share, minus the GUI:

- `web_api` — the axum router behind `serve-web`: the REST surface, the
  `/mcp` JSON-RPC endpoint and the static SPA host.
- `daemon_supervisor` — the host-independent spawn and teardown of the MCP
  daemon subprocess.
- `auth` — the bearer middleware and the list of paths that stay public.
- `port_registry` — the `mcp.port` file that the stdio bridge reads to
  discover the daemon's port.
- `dto` / `http_config` — the wire shapes both transports share, plus the
  helpers for the daemon's listener configuration.

## Why this crate exists

`src-tauri` links `tauri` unconditionally, and Tauri pulls the whole
GTK/WebKit stack on Linux. That made "run the server only" impossible to
containerize: the image carried X11 libraries for a GUI it can never show,
and the `--lib` test binary dragged the Windows GUI runtime in through
`AppHandle` (see the long note on `daemon_supervisor`). Splitting the
server out turns the image into `debian-slim` plus one binary, and keeps
the test binary free of what it does not need.

The dependency direction is one-way:

```text
simple-post (Tauri)  ──▶  simple-post-server  ──▶  simple-post-core
                                  │
                                  └──▶  simple-post-interchange
                                  └──▶  simple-post-mcp
```

`src-tauri` re-exports the modules below, so the Tauri commands still
resolve `crate::daemon_supervisor::…` and `crate::web_api::…`.

## Layout

```
crates/simple-post-server/
├── Cargo.toml
├── README.md
└── src/
    ├── lib.rs                # the public surface + the shared re-exports
    ├── main.rs               # the headless binary: the `serve-web` command
    ├── cli.rs                # the CLI fragments both binaries share (serve-web / mcp)
    ├── auth.rs               # the bearer middleware + the public-path list
    ├── dto.rs                # the wire shapes both transports share
    ├── http_config.rs        # the MCP listener config + its fingerprint
    ├── dynamic_api.rs        # the dynamic-variable orchestration
    ├── interchange_api.rs    # the import / export orchestration
    ├── project_api.rs        # the project and tree-node creation orchestration
    ├── spec_api.rs           # the interface-specification orchestration
    ├── daemon_supervisor.rs  # the facade over the MCP daemon supervision
    ├── daemon_supervisor/    # the supervision, one file per concern
    │   ├── listen_config.rs  #   the persisted listener row → the child's argv
    │   ├── log_buffer.rs     #   the bounded ring buffer for the log tail
    │   ├── probe.rs          #   the healthz probe + the "start was a no-op" rule
    │   ├── reap.rs           #   the termination side: which PIDs, which channel
    │   ├── state.rs          #   the per-spawn bookkeeping
    │   ├── status.rs         #   the lifecycle enum + the snapshot projection
    │   ├── stderr.rs         #   the daemon's stderr forwarding
    │   └── watcher.rs        #   the exit watchers
    ├── port_registry.rs      # the `mcp.port` file facade
    ├── port_registry/        # the registry, one file per concern
    │   ├── claim.rs          #   the multi-instance claim (create_new + fallback)
    │   ├── health.rs         #   the `/mcp/healthz` liveness probe
    │   ├── paths.rs          #   the platform data dir + the file path
    │   ├── proc.rs           #   the process-table probe + the forced kill
    │   └── record.rs         #   the record shape + the configuration fingerprint
    ├── web_api/              # the REST + static-file layer
    │   ├── dto.rs            #   the web-side wire shapes
    │   ├── error.rs          #   the AppError → HTTP mapping
    │   ├── router.rs         #   the route tree + `WebState`
    │   ├── serve.rs          #   the startup entry
    │   ├── static_files.rs   #   the SPA shell + the hashed assets
    │   ├── routes/           #   the handlers, one file per resource
    │   └── spec/             #   the OpenAPI contract table
    └── (the tests sit beside their source as `*_test.rs` / `mod_test.rs`)
```

## The two transports share one orchestration layer

Every feature that both the desktop commands and the REST routes expose
keeps its orchestration in this crate: `interchange_api` (import and
export), `project_api` (project and tree-node creation), `spec_api`
(interface specifications) and `dynamic_api` (dynamic variables). The
Tauri commands in `src-tauri/src/commands/` and the REST handlers in
`web_api/routes/` are thin shells over them.

The shells parse the **wire format** and nothing else: the REST side
accepts the two body shapes that `POST /api/projects` has always taken
and fills in the missing id, while the Tauri side parses the `kind`
string into its enum. The orchestration itself decides who mints the id,
what the timestamps are, how the parent relationship is derived and where
the transaction boundary sits.

A shell that grows past that boundary silently removes the feature from
the other transport, and nothing reports the divergence. The regression net
is `project_api_shells_test.rs`: it reads the four shells' source and
requires each of them to call the shared layer.

## The MCP daemon is reused only when its configuration matches

The supervisor writes the daemon's identity into the `mcp.port` file
together with a **fingerprint** of the listener configuration the daemon
actually serves. The fingerprint is a separate top-level key in the same
JSON object, so `port` / `pid` / `bind` keep their shape and the
third-party readers of that file (the proxy binary, `scripts/mcp-repl.mjs`)
still parse it.

The rule when a supervisor starts:

- The recorded PID is alive and `/mcp/healthz` answers, **and** the
  recorded fingerprint equals the fingerprint of the configuration this
  supervisor wants → **adopt** the daemon. The supervisor records its PID
  and leaves the process alone, so an MCP session in flight keeps working.
- The fingerprint differs, or the record has no fingerprint at all (a file
  written by an older build) → **restart** the daemon. "Cannot be judged"
  resolves toward restarting, for the same reason `requires_restart`
  answers `true` when it cannot judge.

`serve-web` follows the same rule as the desktop application. It used to
tear the daemon down unconditionally before starting one, which killed the
daemon the desktop application had started and dropped every MCP client
attached to it.

## Consumers

- `src-tauri` (the Tauri shell + its `serve-web` subcommand):
  `pub use simple_post_server::{…}` forwards the host half, which is why
  the desktop crate keeps the `crate::daemon_supervisor` and
  `crate::web_api` paths.
- The Docker image builds `simple-post-server` directly. That binary has
  no Tauri dependency, so the image carries no GTK or WebKit.
