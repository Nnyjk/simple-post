# Repository Guidelines

## Project Overview

`simple-post` is a desktop API client/debugger (Postman-like, Windows-first) whose core differentiator is a **built-in MCP server**: external AI clients (Claude Desktop, Cursor, Cline) can list/search/run/mutate endpoints through 44 MCP tools. Stack: **Tauri 2 + React 18 + TypeScript** frontend over a **Cargo workspace** (SQLite via `rusqlite`, `reqwest`, `axum`), shipping both a desktop app and a standalone npm-distributed MCP binary.

Docs are partly Chinese (`README.md`, crate READMEs); code/identifiers are English.

## Architecture & Data Flow

Five runtime pieces:

| Piece | Path | Role |
|---|---|---|
| Tauri shell + CLI | `src-tauri/` | Desktop GUI and the `#[tauri::command]` surface (`commands/`). Everything else is re-exported from the server crate below, so `crate::daemon_supervisor` / `crate::web_api` still resolve here |
| Server host | `crates/simple-post-server/` | `serve-web` REST/SPA server, daemon supervisor, port registry, bearer auth, wire DTOs. **No Tauri by default** — this is what the Docker image builds |
| Core library | `crates/simple-post-core/` | Domain models, SQLite (`db/`), `http_executor`, `permissions`, `mcp_metrics`, `error` — headless, no Tauri/axum dep |
| MCP server | `crates/simple-post-mcp/` | JSON-RPC 2.0 dispatch (44 tools), axum HTTP daemon, stdio bridge |
| Frontend | `src/` | React 18 + Vite 5 + Zustand 5 + Tailwind 3 + CodeMirror 6 |

**The Tauri split, and why it exists.** `src-tauri` links `tauri` unconditionally, so a Linux build of it drags in `libwebkit2gtk` / GTK / X11 — which a container serving `serve-web` needs only to run a GUI it cannot display. The server half therefore lives in `crates/simple-post-server`, and the dependency points one way: `simple-post (Tauri) → simple-post-server → simple-post-core`. Three consequences worth respecting:

1. **The `gui` feature is off by default.** It re-attaches only the three `AppHandle`-taking daemon verbs (`ensure_started` / `start` / `restart`) that emit `daemon://crashed` to the webview. `src-tauri` turns it on; the image build does not. Do **not** move it into `default` to "share more code" — that is the failure the feature exists to avoid.
2. **Wire DTOs are not Tauri-specific.** `McpStatusDto`, `CreateEndpointDetailOpts`, `RestartResult` and friends live in `simple_post_server::dto`, and `http_config.rs` holds the shared read/write helpers. `src-tauri/src/commands/dto.rs` is a `pub use` compatibility surface, not a definition site. A DTO that lands there again re-couples the server to `tauri::State`.
3. **Desktop `simple-post serve-web` still works** and delegates to the same implementation, so `npm run dev:split` and the documented LAN workflow do not regress. There is one implementation, not two.

**Resource tree.** Modules, collections and endpoints all live in **one `nodes` table**, discriminated by `type`. Hierarchy is `parent_id`; the owning project is denormalised onto every row as `root_id` (one indexed read answers "which project", which is both the store's filter and the MCP permission gate's key). Endpoint request configuration — method, url, auth, body, headers, params, tags — is a separate `endpoint_details` row keyed by node id, because those fields are meaningless for a module and the tree never renders them. The logical shape is still Project → Module → **Collection (recursively nestable, unbounded depth)** → Endpoint — with the one relaxation that an endpoint may sit directly under a module, so a single endpoint needs no throwaway collection first — and UI renders it with a recursive component.

The parent/child relationship is an **enforced contract**, not a convention: `ResourceNodeType::may_under` defines that a module may only be top-level, an endpoint may sit under either a collection or a module, and a collection may sit under either a module or another collection. A SQLite trigger and an application-layer check both enforce it. `db::nodes::move_many` refuses any other combination, and the frontend's optimistic `moveNodes` guard mirrors it.

**CLI modes** (`src-tauri/src/main.rs`, clap): default → Tauri GUI; `mcp` → embedded MCP daemon; `mcp-proxy` → stdio bridge; `serve-web` → headless REST + static SPA.

**CRUD flow** — component → Zustand slice (optimistic update) → `invokeCmd()` (`src/lib/tauri-commands.ts`) → Tauri IPC **or** `fetch('<base>/api/…')` → `src-tauri/src/commands/` (desktop) / `crates/simple-post-server/src/web_api/` (browser) → `simple_post_core::db` → `store.db` (WAL).

**HTTP requests** bypass webview CORS: `src/lib/http.ts::sendRequest` → `httpRequest` command → `http_executor.rs` (reqwest).

**Two surfaces, switched on `activeProjectId === ''`** (`src/App.tsx`). `WelcomeScreen` is a full-window page with no top bar, no tree and no tab strip; everything else lives in `WorkspaceShell` (`src/features/workspace/`), which is `lazy()`. The split does three things at once: it keeps the whole workspace — and through it the CodeMirror editors — out of the cold-start chunk; it pairs with the two-layer `bootstrap()` below; and it makes the no-project state genuinely empty instead of a frame full of controls that cannot do anything. `leaveProject()` (the TopBar wordmark) is the way back.

**Settings is a third host, not a third implementation.** The settings singleton is also a *tab*, so it has to be reachable with no project open — otherwise "打开设置" on the welcome screen writes a tab nobody renders and visibly does nothing. `App.tsx` therefore branches on `activeProjectId === '' && activeTabId === SETTINGS_TAB_ID` → `SettingsSurface` (`src/features/settings/SettingsSurface.tsx`), a `lazy()` full-window host that adds only the chrome a standalone surface needs (drag region, 返回, window controls) around **the same** `SettingsPanel` the workspace mounts as a tab. Two hosts, one panel: every section reads from the store rather than props, so the two cannot disagree. `SettingsPanel` gates its env section on `activeProjectId !== ''` — with no project the `新建环境` button would write to `projectId === ''` and fail silently, so it renders a hint instead. Both hosts are `lazy()` and `SettingsPanel` is a shared chunk: do not statically import either from `App.tsx`.

**Two-layer `bootstrap()`** (`src/stores/bootstrap-slice.ts`). Layer 1 is `projects_list`, awaited; it flips `loaded`, which tears down the HTML boot splash. Layer 2 is `nodes_list` + `endpoint_details_list` + `environments_list`, run by `loadWorkspace()` and **not** awaited. Consequences worth respecting: `nodes: []` is ambiguous while `nodesLoaded === false` (render a loading state, never "该项目还没有模块"); layer-2 failures land in `nodesError`, not `bootstrapError`, so a tree failure cannot strand the splash; and `pruneStaleTabs()` no-ops until `nodesLoaded` or it would delete every tab it is supposed to be keeping.

**Dev-server startup is NOT the app's startup.** `optimizeDeps.noDiscovery` + a short `include` in `vite.config.ts` is load-bearing, not a micro-optimisation: measured cold, the first request through the dev server took **11.3 s**, because the dependency crawler follows every `import()` and eagerly pre-bundles all 22 packages — the whole CodeMirror stack, `gsap`/`Flip`, `marked`, plus a 1.1 MB `lucide-react` — none of which the welcome screen touches. With discovery off and only the startup deps named, that same cold request is **139 ms**. Two things that look equivalent and are not: naming *all* deps in `include` is just as slow (10 s, because pre-bundling is eager once a package is named), and a `configureServer` pre-warm plugin makes it *worse* (15 s — it serialises behind Vite's own first request instead of overlapping it). Do not "optimise" this back. The trade it buys: the HTTP tester pays a few hundred ms of raw-ESM requests the first time it opens, which is the right side to spend on when the startup screen is on every launch.

**GSAP is not on the critical path.** `src/lib/gsap.ts` pulls core + CSSPlugin (~60 kB raw), so anything it reaches from the entry chunk pays for it on every cold start. That is why `WelcomeScreen`'s entrance is CSS (`.welcome-enter` in `globals.css`) rather than `gsap.from()`, and why `NewItemModal` / `BootstrapErrorBanner` are mounted on `newItem.open` / `bootstrapError` rather than unconditionally — a `lazy()` component fires its import as soon as it is *rendered*, so wrapping them in `<Suspense>` alone still downloads them during first paint. Verify with the dependency-edge probe described under **Vite `manualChunks`**; a chunk listing alone will not show you a static import edge.

**Services (baseUrl) are project-owned; their URLs are per (环境 × 模块 × 服务).** The service *list* is a project-level fact — `base_url_definitions.project_id` only — so two modules can never hold services that share a `name`, and every module sees every service. The *value* is the per-cell question: `environments.module_base_urls_json` is `service_id → module_id → url`, and each cell is independent. A module with no entry for a service has no URL for it and inherits **nothing** — not the project's ★, not another module's. There is deliberately no fallback layer above it; the v1 code reached for `env.base_urls` and then "whatever key happens to exist", which is exactly the silent cross-module bleed this replaced. `resolve_base_url_with` (`crates/simple-post-mcp/src/variable_fill.rs`) is the resolver; the module-less `resolve_base_url` wrapper exists for callers that have no module and now serves tests only.

**Three columns are dead on purpose — do not "clean them up".** `environments.base_urls_json`, `base_url_definitions.module_id` and `base_url_definitions.is_default` are superseded and never read or written by application code. They stay because a `DROP` means rebuilding the table, which means bumping `SCHEMA_VERSION`, and `ensure_current_schema` **deletes every user's store** on a bump. That is why `PRAGMA user_version` is still `3` despite the shape having changed: `module_base_urls_json` arrives additively through `db::ADDED_COLUMNS`, and `db::migrate` repairs rows written by either predecessor layout. An agent tidying "unused columns" here destroys user data.

**MCP flow**: AI client → stdio (`stdio_loop.rs`) → `POST 127.0.0.1:<port>/mcp` → `daemon_http.rs` → `mcp/mod.rs::dispatch` → capability gate (`read_only`/`safe_write`/`full_access`) → per-project permission check → tool handler → shared `db` pool / `http_executor`. Daemon port discovered via `mcp.port` file in the data dir (`SIMPLE_POST_DATA_DIR` or `%APPDATA%/simple-post`).

Dev split: Vite `5173` proxies `/api` + `/mcp` to axum `4173`.

## Key Directories

- `src/features/` — feature panels: `http-tester/`, `project-tree/`, `settings/`, `command-palette/`, `top-bar/`, `workspace/` (incl. `WorkspaceShell.tsx`), `response-viewer/`, `welcome/`
- `src/stores/` — Zustand: `app-store.ts` barrel composing `bootstrap-slice`, `crud-slice`, `ui-slice`, `mcp-slice`, `dirty-tabs-slice` + `store-types.ts`
- `src/lib/` — `tauri-commands.ts` (dual-transport IPC + `COMMANDS` registry), `http.ts`, `http-build.ts`, `tree-index.ts` (the shared `parentId → children` index), `mcp-types.ts`, `use-mcp-status.ts`
- `src/components/ui/` — shared primitives (`method-picker`, `tabs`, `dialog`, …)
- `src/types/domain.ts` — TS mirrors of Rust domain types
- `crates/simple-post-core/src/db/` — per-entity SQL modules (`nodes.rs` holds the whole resource tree, `endpoint_details.rs` its endpoint-only configuration) + `schema.sql` + `fixtures.json` (test-only)
- `crates/simple-post-mcp/src/mcp/` — tool handlers (`project_ops/`, `env_ops.rs`, `http_ops.rs`)
- `src-tauri/src/` — `commands/` (Tauri handlers, one module per resource; `dto.rs` is now only a re-export of the server crate's DTOs)
- `crates/simple-post-server/src/` — `web_api/` (REST + SPA host), `daemon_supervisor.rs`, `port_registry.rs`, `auth.rs`, `dto.rs`, `http_config.rs`, `lib.rs` (`run_mcp` / `run_serve_web`), `main.rs` (the Tauri-free CLI)
- `scripts/` — dev/test/build utilities (see below)
- `.scratch/` — gitignored agent scratch space (session state, JSON-RPC fixtures)

## Development Commands

```bash
npm run dev            # Vite dev server → http://127.0.0.1:5173
npm run dev:split      # Vite + `cargo run -- serve-web` (4173) concurrently — use this for full-backend work
npm run build          # tsc -b && vite build → dist/
npm run typecheck      # tsc -b only (no bundle)
npm run check          # one-shot quality gate: format:check → lint → typecheck → test → fmt:check → clippy
npm run lint[:fix]     # ESLint 10 flat config (eslint.config.js)
npm run format         # Prettier --write (.prettierrc + .prettierignore); `format:check` for CI
npm run fmt[:check]    # cargo fmt --all [--check]
npm run clippy         # cargo clippy --workspace --all-targets -- -D warnings
npm run tauri:dev      # Tauri desktop dev shell
npm run tauri:build    # production NSIS installer bundle
npm run tauri:check    # cargo check --manifest-path src-tauri/Cargo.toml
npm run icons          # regenerate every platform icon from src-tauri/app-icon.svg

npm test               # frontend unit tests (auto-discovers src/**/*.test.ts — see Testing)
npm run test:watch     # same, watch mode
npm run test:mcp       # MCP end-to-end (requires compiled binary)
npm run test:web:auth      # serve-web bearer auth flow integration (needs binary)
npm run test:web:basepath  # serve-web reverse-proxy base path integration (needs binary)
npm run perf:bench         # serve-web p50/p95/p99 latency (--assert enforces 200ms p95 budget)
npm run build:tests    # gated MCP test; exits 2 if binary missing
npm run mcp:repl       # interactive JSON-RPC REPL against a running daemon

cargo test --workspace              # all Rust tests
cargo test -p simple-post-core      # single crate
node scripts/dump-openapi.mjs > openapi.yaml      # render the OpenAPI spec from public_route_specs()
```

Lint/format tooling: **ESLint 10 (flat config) + Prettier 3**. ESLint owns correctness (`rules-of-hooks`, unused vars, …); style belongs to Prettier (`eslint-config-prettier` sits last in the config). Rust side: `cargo fmt`, `cargo clippy` with `-D warnings`.

**CI quality gate** — `.github/workflows/ci.yml` runs on every push to `main` and every PR (`release.yml` only publishes):
- `web` (ubuntu, Node 22): `format:check` → `lint` → `typecheck` → `npm test` → `vite build`
- `rust` (windows-latest): `cargo fmt --check` → `clippy -D warnings` → `cargo test --workspace`

Keep `npm run check` green locally before pushing — CI runs the same checks.

## Code Conventions & Common Patterns

**Error handling.** Rust returns `Result<T, AppError>` (`crates/simple-post-core/src/error.rs`); `AppError` serializes to a stable `{ code: <snake_case>, message }` wire shape — `code` is the contract, `message` is human-facing. MCP tool failures are returned *inside* `CallToolResult` with `is_error: true` (not a JSON-RPC envelope error) so LLM clients can observe them. Frontend never throws for network failure — `ApiResponse.status === 0` signals transport error.

**DB access.** Reads → `db::with_conn`; writes → `db::with_tx` (`BEGIN IMMEDIATE`, 3 retries / 50 ms backoff on `SQLITE_BUSY`). WAL + `foreign_keys=ON` + `busy_timeout=3000`. Complex fields (`KeyValue[]`, `RequestBody`, `AuthConfig`, `tags`) live in `*_json` text columns. New entity SQL goes in `crates/simple-post-core/src/db/<entity>.rs`.

**Partial updates.** Patch types (`ResourceNodePatch`, `EndpointDetailPatch`, `ProjectPatch`, `EnvironmentPatch`, …) use `#[serde(default, deserialize_with = "double_option")] Option<Option<T>>`: missing key = leave unchanged, JSON `null` = clear, value = set (`crates/simple-post-core/src/domain.rs`). Follow this pattern for any new updatable field. Note `ResourceNodePatch::parent_id` is the one place the pattern earns its keep: `Some(None)` promotes a node to the top of its project, which is a real write target rather than "leave alone".

**Serialization.** All domain types: `#[serde(rename_all = "camelCase")]`; TS counterparts in `src/types/domain.ts`. Keep both in sync when adding fields.

**Zustand selectors (infinite-loop hazard).** Equality is `Object.is`; a selector returning a fresh array/object (`.map`, `.filter`, spread) causes infinite re-render. Selectors must return stable references or `.find(...)`; derive filtered lists with `useMemo` in the component. Slices never import each other — they share state via the `AppState` interface and `get()`/`set()`. Verify with `npx tsx scripts/_verify_store.mjs`.

**Dual-transport invariant.** Any new backend command needs **five** coordinated edits: handler in `src-tauri/src/commands/<resource>.rs` (re-exported by `commands/mod.rs`, which must use **glob** `pub use` — an item list drops the `#[macro_export]`ed `__cmd__<name>` macro and `generate_handler!` fails with E0433), registration in `src-tauri/src/lib.rs`, a `COMMANDS` constant in `src/lib/tauri-commands.ts`, a route in `crates/simple-post-server/src/web_api/` (the `.route()` registrations live in `mod.rs::build_router`, the handler in the matching `web_api/routes/<resource>.rs`), and a `WEB_ROUTES` entry (if browser-callable). Missing one side breaks either desktop or web mode silently. A DTO both sides need belongs in `crates/simple-post-server/src/dto.rs`, **not** in `commands/` — see the Tauri split above.

**Tauri macro gotcha.** `http_request` in `src-tauri/src/lib.rs` must stay **non-`pub`** — marking it `pub` triggers a `__cmd__http_request` identifier collision (E0255) between `#[tauri::command]` and `generate_handler!`.

**App / window icons have no config knob.** There is no `icon` field on `WindowConfig` and no `defaultWindowIcon` under `app` in Tauri 2 — adding one fails the build with `unknown field` at `tauri-build`. The single lever is `bundle.icon`: `tauri-build` picks the first `.ico` from it and embeds it into the exe as resource `32512`, and tao registers window classes with a null icon, so Windows resolves the taskbar icon from that executable resource. Changing the taskbar icon therefore means **editing `src-tauri/app-icon.svg` and re-running `npm run icons`** — not editing `tauri.conf.json`. Two gotchas in that pipeline: `icon.icns` is not byte-reproducible (`tauri icon` stamps it, so it always diffs), and `tauri icon` parses its SVG with a strict XML parser whose comments may not contain `--`, which makes writing CSS custom-property names like `--primary` in a doc comment fail the whole run.

**Routing.** Axum 0.7 path params use `:id` syntax (`/api/projects/:id`), not `{id}`.

**Naming.** Rust: `snake_case` functions/modules/columns, `PascalCase` types. Tauri commands `snake_case` in Rust, `camelCase` in the frontend `COMMANDS` registry. Test files: `*.test.ts`; Rust tests go in a **sibling** `*_test.rs` file mounted by `#[path]`, not inline (the inline form survives only in a few files under `src-tauri/` — see Testing & QA).

**Async/concurrency.** tokio multi-thread runtime; metrics via `AtomicU64` with `Ordering::Relaxed`; `port_registry.rs` uses OS atomic file creation (`create_new`) to prevent multi-instance races.

**MCP stdout discipline.** In `simple-post-mcp`, stdout carries *only* JSON-RPC frames; all logging, panics, and `__mcp_metrics__{…}` telemetry go to **stderr**. Non-JSON on stdout crashes AI clients.

**MCP permissions — two gates, both fail-closed.** Default policy is `read_only`; mutating tools (`env_create`, `endpoint_update`, …) return `is_error: true` until the user raises the tier. **Gate A (global):** `mcp::tool_descriptors` filters `tools/list` by `REQUIRED_CAPABILITY` under the policy the user picked in Settings, and `mcp::dispatch` re-checks before the handler — so an AI client cannot bypass it by calling `tools/call` directly, it just gets `AppError::PermissionDenied`. A tool with **no** capability row is treated as unclassified and denied, not allowed. `Capability` is a total order `Read < Create < Update < Delete < Send`; `Send` means outbound HTTP and is `full_access`-only — that is the deliberate "safe write ≠ free outbound" boundary. **Gate B (per-project):** `required_permission()` over the `permissions` table (`Read < ReadCall < Full`, default `ReadCall`); an unknown tool name is treated as `Full`.

Gate B has two ways to silently become a no-op, and both shipped:

1. **Wrong project id.** Handlers must resolve the real owning project and pass it — `db::nodes::root_id_of` for anything that is a `nodes` row, `db::base_url_defs::project_id_of` for a baseUrl definition, `db::environments::lookup_project_id` for an environment. Passing the literal string `"global"` looks harmless and is not: `agent_permissions.project_id` is an FK to `projects(id)` and ids are minted `proj_<millis>`, so the lookup finds no row, falls back to `Permission::default()` = `ReadCall`, and every write tool — which requires `Full` — is denied. **The tool is then permanently dead while still looking correctly registered.** This shipped on five `env_*` tools, then again on `module_update` / `module_delete` / `collection_create` / `base_url_def_update` / `base_url_def_delete`. `project_create` and `project_list` have no owning project by construction and correctly skip Gate B entirely — Gate A governs them.
2. **Discarded result.** `check_permission(..).await.ok()` compiles, passes review by eye, and throws the `PermissionDenied` away, so the gate never blocks. It shipped on `api_call_endpoint` — the one tool that performs an outbound HTTP request. Always propagate with `?`.

`no_gate_is_passed_the_literal_global` and `no_permission_check_result_is_discarded` in `crates/simple-post-mcp/src/mcp/mod_test.rs` are the regression net. They are source-level rather than behavioural on purpose: a permanently-denied tool still returns a well-formed `PermissionDenied`, so no functional test would notice. The policy itself persists in `preferences` under key `mcp.policy`.

**Adding an MCP tool requires TWO registry edits, not one.** A tool added to `tool_table()` without a matching `REQUIRED_CAPABILITY` row **escapes the global capability gate entirely** — both the `tools/list` filter and the `dispatch` pre-flight treat a missing row as unclassified and now fail *closed* (deny), but the tool becomes invisible/unusable rather than policy-compliant. This actually shipped once: `module_move` and `collection_move` were registered without rows and bypassed the user's `read_only` choice. `every_registered_tool_has_a_capability` (and its inverse `every_capability_row_names_a_registered_tool`) in `crates/simple-post-mcp/src/mcp/mod.rs` is the regression net — if you add a tool and those tests fail, add the row with the correct `Capability` (`Read < Create < Update < Delete < Send`; `Send` means outbound HTTP and is `full_access`-only).

**MCP wire-format field names are camelCase, and the hand-written JSON is a third site.** `ToolDescriptor.input_schema` and `CallToolResult.is_error` shipped as snake_case while the spec spells them `inputSchema` and `isError`, so all 44 tools reached a compliant client as argument-less and every failed call read as a success — with every unit test green, because the handlers still received the right arguments. There are **three** output paths, not two: `CallToolResult` and `ToolDescriptor` are fixed by `#[serde(rename_all = "camelCase")]`, but `daemon_http.rs::handle_tools_call`'s failure arm builds its envelope with `json!({…})` and bypasses both structs — fixing the structs alone left that one broken. Don't confuse `CallToolContent`'s `rename_all = "snake_case"` with this: that one controls the `type` **tag**, whose only variant is `text`. Five regression tests sit in `mod_test.rs`, two of them source-level (`include_str!` over `daemon_http.rs`, skipping comment lines so the file's own prose about the bug can't trip them) because no functional test can observe this class of defect.

**An MCP client must not hardcode a daemon port.** `simple-post mcp` defaults to `--port=0`, so the OS-assigned port changes on every restart and a hardcoded `http://127.0.0.1:<port>/mcp` endpoint silently dies with the next restart — which also means the user cannot change `mcp.policy` without breaking their client's connection, since changing the policy requires a daemon restart. `simple-post-mcp.exe` does **not** solve this on its own: it ignores `mcp.port` and posts to `--url`, which it joins as `{base_url}/mcp` (passing `…/mcp` yields `/mcp/mcp` → 404). The arrangement with no moving parts is **stdio bridge → `serve-web` on its fixed default port 4173**, whose `/mcp` route runs the same `mcp::dispatch`: the client config holds only `command` + `args`, so there is no port in it to drift.

**`preferences.value` holds a JSON-encoded value, not the bare string.** `db::preferences::set` runs `serde_json::to_string`, and every reader runs `from_str` on the result — writing the literal `full_access` instead of `"full_access"` fails to parse, and `mcp_policy` treats an unrecognised value as `ReadOnly` with no error anywhere. The symptom is `tools/list` staying at 16 tools no matter what you wrote to the DB.

**IME guard.** Global shortcuts in `src/App.tsx` must check `e.isComposing`, else CJK IME Enter fires requests.

**The tab strip's active-indicator is derived per frame, never measured once.** `WorkspaceTabs.tsx` keeps one persistent bar at strip level and, while anything that can move a tab may be animating, re-reads the active tab's *live* rect every animation frame and eases toward it (`BAR_APPROACH` / `BAR_SETTLE_MS`). Do not "simplify" this back to `offsetLeft` + `gsap.to`. Three things move those tabs and all three can be mid-flight at the moment a one-shot measurement is taken: the Flip tween (with `absolute: true` every tweened tab reports its **pre-commit** position), the CSS width transition on tab mode change (200ms, changes widths *and* every neighbour's offset), and the strip's `scrollLeft`. Measuring once is what produced the bar drifting to the wrong x and keeping a stale width when tabs were closed. The bar's x is in **content** coordinates — add `strip.scrollLeft` back, or it drifts by exactly the amount scrolled.

**Semantic token pairing — a `-foreground` token is a colour for text *on* the saturated fill, not on a surface.** `--destructive-foreground` is `0 0% 98%` in **both** theme scopes (`globals.css`), so pairing it with a light surface produces white-on-white: `bg-background/85 text-destructive-foreground` renders as an invisible badge in light mode and as colourless text with no error signal in dark. This shipped once on the body editor's pinned validation pill. Two correct shapes: **solid** `bg-destructive text-destructive-foreground` (identical in both themes, ~4.8:1, nothing overrides either token), or a tinted surface with the tokens the dark scope *does* lighten — `--success-foreground` / `--warning-foreground`, the convention the tab badges use. Note `.alert-error-inline` in `globals.css` still carries the broken combination; it is currently unused, so fix it if you wire it up rather than copying it.

**Reverse-proxy base path is a runtime fact, not a build constant.** `serve-web --public-base-path /sp` `nest`s the *entire* router under the prefix — `/api/*`, `/mcp`, and the three health probes all move to `/sp/…` together. Two consequences that are easy to get wrong:

- `vite.config.ts` sets **no `base`**, so `vite build` emits root-absolute asset URLs (`<script src="/assets/index-<hash>.js">`), and every `WEB_ROUTES` path is root-absolute (`/api/projects`). Neither can be corrected at build time: in the container the prefix arrives as `SIMPLE_POST_PUBLIC_BASE_PATH` at **run** time and one image has to serve both shapes. So `static_files::shell_with_base` rewrites the shell's root-absolute `src`/`href` per response and injects `window.__SIMPLE_POST_BASE__`, which `readApiBase()` in `src/lib/tauri-commands.ts` reads to prefix API calls. **Never "fix" a base-path report by adding `base` to `vite.config.ts` or an nginx rewrite** — those reintroduce a second place that has to agree with the first.
- `/healthz` moves with everything else, so any probe hardcoded to the bare root 404s in exactly the deployment that sets a prefix. The image's healthcheck script reads the same env var for this reason.
- There is **no** `<base href>` tag, and there must not be one: it does not affect a root-absolute `src`, so it would look like a fix while changing nothing.

The regression net is `scripts/test-web-basepath.mjs`, which parses the served shell and actually requests every URL it finds. Its earlier version only asserted the *server* nested correctly (`/sp/api/…` → 200, bare → 404) and never looked at what the browser would ask for — which is how a deployment where the shell loaded and every API call 404'd could pass. Assert on the shell's real references and their content-types; `200 text/html` for a `.js` file is the ServeDir fallback, not an asset.

**A route named in `auth::is_public_path` but never registered answers `200` with the SPA shell.** The auth bypass list and the route tree are two places that have to agree, and nothing made them. `/healthz` sat in that list since the beginning (comment: "legacy, kept for backward compat") with **no `.route("/healthz", …)` anywhere** — so at the root it fell through to the ServeDir fallback and returned the index with `200 text/html`, and under a base path it 404'd. The container `HEALTHCHECK` passed the whole time, because it only grepped for `200 OK`. A probe that cannot fail is worse than no probe: **grep for the response body**, not the status line. Two assertions catch this class — `is_public_path` membership is not route existence, and a probe should assert the thing it is named after.

**OpenAPI.** `public_route_specs()` in `crates/simple-post-server/src/web_api/spec.rs` (re-exported from `web_api/mod.rs`) is the single source of truth; internal routes (`/api/mcp/*`, `/api/preferences`) are intentionally undocumented. `node scripts/dump-openapi.mjs` renders that table as an OpenAPI 3.1 document on stdout — pipe it wherever you want, e.g. `node scripts/dump-openapi.mjs > openapi.yaml`. It has no checked-in output since `docs/` was removed.

**Traps a generated spec cannot express**, kept here now that `docs/http-api-reference.md` is gone:

- axum 0.7 path params are `:id`, **not** `{id}`. The OpenAPI spec's own convention is `{id}`, so copying a path out of the yaml silently registers a literal segment and the request falls through to the SPA fallback — a 200 with HTML, which looks like "the endpoint does nothing".
- The error envelope is stable: `{ "code": <snake_case>, "message" }`. The `code` is the part to branch on — treat it as the contract and `message` as human-facing text. Note the frontend currently does **not** branch on it: `invokeCmd` folds `code` into the thrown message (`HTTP 400 POST /api/… [validation] …`), so a caller that needs to distinguish "not found" from "denied" must parse the string or add a typed branch. Don't widen that string-formatting into more call sites.
- The auth matrix: `/api/healthz`, `/mcp/healthz` and `/healthz` are always unauthenticated, as are `/assets/*` and the SPA shell — a browser cannot attach `Authorization` to a `<script type="module">`.
- The openapi-diff comparison is the regression guard for any `web_api/` route-tree refactor: regenerate before and after, and a route that vanished shows up as a missing path rather than a failing test.

## Important Files

- `src/main.tsx`, `src/App.tsx` — frontend entry; theme preload, global shortcuts, layout
- `src/lib/tauri-commands.ts` — `IS_TAURI_SHELL` detection, `invokeCmd`, `COMMANDS`, `WEB_ROUTES`
- `src/stores/app-store.ts` — store barrel + safe selector hooks
- `src-tauri/src/main.rs` / `src-tauri/src/lib.rs` — CLI dispatch / Tauri `run()` (the `serve-web` and `mcp` arms forward into the server crate)
- `crates/simple-post-server/src/web_api/` — REST surface for `serve-web`: router + spec re-exports in `mod.rs`, handlers in `routes/`, OpenAPI source in `spec.rs`, SPA shell rewriting in `static_files.rs`
- `crates/simple-post-server/src/main.rs` — the Tauri-free CLI (`simple-post-server serve-web` / `mcp`); this is the container entrypoint
- `crates/simple-post-core/src/db/mod.rs` + `schema.sql` — pool, transactions, migrations, seed
- `crates/simple-post-core/src/db/nodes.rs` — the resource tree: CRUD, `list_children`, `move_many` (reparents + re-derives `root_id` across the subtree), `move_to_root` (cross-project), `duplicate`, `search`
- `crates/simple-post-core/src/domain.rs`, `error.rs` — canonical types + wire errors
- `crates/simple-post-mcp/src/mcp/mod.rs` — 44-tool registry, `REQUIRED_CAPABILITY`, `dispatch`
- `Cargo.toml` — workspace root (members, shared deps, release profile)
- `src-tauri/tauri.conf.json` — window, NSIS bundle, build hooks, CSP (intentionally `null`)
- `src-tauri/app-icon.svg` — the **only** source of app-icon artwork; `npm run icons` runs `tauri icon` over it to regenerate all of `src-tauri/icons/`
- `src-tauri/rust-toolchain.toml`, `src-tauri/capabilities/default.json`
- `vite.config.ts` — alias, dev server, `/api` + `/mcp` proxy (ws disabled on `/mcp`)
- `Dockerfile` / `.dockerignore` / `docker-compose.yml` / `docker-swarm.yml` — container deployment (issue #17). The Dockerfile's three stages are load-bearing: the **runtime stage installs no packages at all**, which is only true while `simple-post-server` stays Tauri-free. Both yml files use `image:` (never `build:`) and omit a top-level `version:` so one shape feeds `docker compose up` *and* `docker stack deploy`. In the swarm file `deploy.replicas` is pinned to `1` on purpose — the store is a single-writer SQLite file in WAL mode, and WAL's `-shm`/`-wal` sidecars only work on one filesystem. **That covers concurrency, not placement**: the volume has the `local` driver, so a reschedule onto another node would find no volume, get a fresh empty one, and start on a blank database. The `placement` constraint below the replicas block is what closes that hole, and it is on by default for that reason — `replicas: 1` is frequently mistaken for a substitute, and it is not. The image tag is `${SP_IMAGE:-simple-post:local}` because Swarm pulls the name written in the spec on every node; a bare local tag exists on one machine only.
- `tsconfig.json`, `package.json`, `tailwind.config.js`
- `eslint.config.js`, `.prettierrc`, `.prettierignore` — lint/format rules and scope
- `.editorconfig`, `.gitattributes` — LF-everywhere policy (only `*.bat`/`*.cmd` are CRLF)
- `.github/workflows/ci.yml` — the push/PR quality gate; `release.yml` only publishes
- `scripts/run-tests.mjs` — test discovery runner behind `npm test`

## Runtime/Tooling Preferences

- **Node `>=20 <23`** (root `package.json` engines); package manager is **npm** (lockfile v3). No pnpm/yarn/bun — do not add their lockfiles.
- Root `package.json` declares **no workspaces**, and there is no second npm package: the `simple-post-mcp` bridge ships only as a compiled binary (release artefact / `cargo install`). Anything an AI client needs is reachable over the daemon's HTTP endpoint, so no registry publish path exists.
- **Rust**: edition 2021, MSRV 1.77.2, toolchain channel **`stable`** with `rustfmt` + `clippy` (`src-tauri/rust-toolchain.toml`). Track `stable` so edition-2024 transitive deps (clap 4.6+, etc.) compile; if you need a pinned baseline, set `channel = "1.85.0"` (edition2024 stable release) or newer and run `rustup toolchain install` first.
- TypeScript path alias `@/*` → `src/*`; `moduleResolution: bundler`, `noEmit` (type checks run via `npm run typecheck` / `tsc -b`, also embedded in `npm run build`).
- Workspace builds output to `<repo>/target/`, not `src-tauri/target/`.
- **Line endings: LF everywhere** — `.gitattributes` (`* text=auto eol=lf`) overrides `core.autocrlf`; `.editorconfig` mirrors it for editors (UTF-8, final newline, trim whitespace, 2-space default / 4 for `*.rs`, Markdown keeps trailing spaces). Only `*.bat` / `*.cmd` stay CRLF.
- **Prettier scope**: `*.md` and lockfiles are in `.prettierignore` — hand-maintained prose stays byte-stable. There is no longer a generated `docs/openapi.yaml` in the tree (`docs/` was removed); if you render one, put it outside the repo or add it to `.prettierignore` yourself.
- Windows-first: PowerShell helpers in `scripts/*.ps1`. When writing UTF-8 files from PowerShell, specify `-Encoding utf8NoBOM` — 5.1's default ANSI encoding corrupts UTF-8 and can break `rustc`.
- Multi-worker convention (per `README.md`): parallel workers deliver code only; end-to-end/browser verification is done once by the main session.

## Testing & QA

**No Vitest/Jest/React Testing Library.** Frontend tests use the Node built-in runner via `tsx --test` + `node:assert/strict`.

| Layer | Command | Location |
|---|---|---|
| Frontend unit/slice | `npm test` / `npm run test:watch` | `src/*/__tests__/*.test.ts` |
| Rust | `cargo test --workspace` / `-p <crate>` | sibling `#[path]` test modules — see below |
| MCP end-to-end | `npm run test:mcp` | `scripts/mcp-integration.test.mjs` (needs compiled `simple-post` binary) |
| Web auth integration | `npm run test:web:auth` | `scripts/test-web-auth.mjs` (needs compiled binary; spawns two phases — open + locked) |
| Web base-path integration | `npm run test:web:basepath` | `scripts/test-web-basepath.mjs` (needs compiled binary; `--public-base-path /sp`) |
| Web perf bench | `npm run perf:bench` | `scripts/perf-bench.mjs` (p50/p95/p99 over N hits against a **running** server; `--target URL` to point elsewhere; `--assert` gates 200ms p95) |
| Gated CI variant | `npm run build:tests` | `scripts/run-mcp-tests.cjs` (exits 2 if the binary is absent) |
| Store invariant check | `npx tsx scripts/_verify_store.mjs` | selector stability, cascade delete, prefs persistence |

Key facts:

- **Test file placement — frontend.** Every test lives in a `__tests__/` directory beside the code it covers: `src/lib/__tests__/*.test.ts` and `src/stores/__tests__/*.test.ts`. The file is still named after the module (`http.test.ts` for `http.ts`), not after the folder. `__tests__` is a recognised convention (Create React App / Angular lineage) rather than *the* standard — co-location is the more common modern React default — but it is a legitimate one, and consistency inside this repo is what matters. The double underscore marks the directory as non-shippable without relying on a bundler honouring any name.
- **Test file placement — Rust.** There is no `__tests__` equivalent and no way to add one: Rust's module system resolves a file's location from the module path, so a test-only directory is not discoverable. The two options are the inline `#[cfg(test)] mod tests { … }` and a sibling file reached through `#[path]`. This repo uses the sibling form everywhere except a handful still inline under `src-tauri/` (`auth.rs`, `daemon_supervisor.rs`, `port_registry.rs`) — match the dominant form when you add tests:
  ```rust
  #[cfg(test)]
  #[path = "projects_test.rs"]
  mod tests;
  ```
  `super` resolves identically from a `#[path]` module, so the extracted body keeps its `use super::*;` unchanged and `cargo fmt --all` re-indents it. `#[path]` rather than the Rust 2018 `projects/tests.rs` submodule directory, so the pairing stays visible in directories that are otherwise flat `*.rs` files. When splitting one, **match the braces — do not assume the block's closing brace is the last line**: `db/mod.rs` has `pub mod test_counter` *after* its `mod tests`, and swallowing that tail would compile a `#[cfg(test)]` test helper into the library.
- **`npm test` auto-discovers `src/**/*.test.ts`** through `scripts/run-tests.mjs` — no hardcoded list, new test files are picked up automatically. In `--watch` mode the list is captured at spawn time: restart `npm run test:watch` after adding/renaming a test file. Single file: `npx tsx --test path/to/foo.test.ts`.
- **A test that resolves the module under test at runtime survives neither a move nor a rename.** `tauri-commands.test.ts` probes `IS_TAURI_SHELL` in a `spawnSync` child because that flag is frozen at module-eval time, and resolves the module with `new URL('./tauri-commands.ts', import.meta.url).href`. That URL is relative to the *test file*, so moving the test into `__tests__/` silently broke it until the path gained its `../`. Typecheck and lint both passed on the broken version — only the child process failed. Grep for `import.meta.url` and `__dirname` whenever a test file changes directory.
- ESLint scoping decisions (in `eslint.config.js`): `no-explicit-any` off under `**/*.test.ts` and `**/__tests__/` (deliberately loose mocks); `no-require-imports` off for `.mjs`/`.cjs` tooling; names starting with `_` are sanctioned unused placeholders; `react-hooks/rules-of-hooks` is **error** — `HttpTester`'s hooks were hoisted above its `if (!endpoint)` early return to satisfy it (never put hooks after an early return).
- Headless store tests need `src/stores/__tests__/_browser-shim.ts`: installs Map-backed `localStorage` (Node 22 ships a methodless native `localStorage` that crashes otherwise), `document.visibilityState` emitter, timers, and `mockIPC`, and sets `window.__TAURI_INTERNALS__ = {}` *before* module eval so `IS_TAURI_SHELL === true`.
- **No seed data ships with the app.** `db::init` runs `schema.sql` and stops — a fresh install is empty. `crates/simple-post-core/src/db/fixtures.json` is **test-only**: it is reachable solely through `#[cfg(test)] pub(crate) mod test_support` (`init_seeded` / `seed`), which does not exist in a production build. Because `test_support` is `pub(crate)`, sibling crates cannot use it — the `endpoint_send_tests` module in `src-tauri/src/commands/endpoint_send_tests.rs` builds its own rows through the public CRUD API instead. There is **no** TS mock-data module; store tests build their own fixtures inline.
- Rust DB tests isolate via `fresh_pool()` → temp-dir SQLite keyed by atomic counter + PID (`crates/simple-post-core/src/db/mod_test.rs`, the `#[path]` module of `db/mod.rs`).
- Store custom hooks (`useActiveProject`, …) throw outside a React render; in Node, read state with `useAppStore.getState()`.
- No React component/DOM tests; no coverage tooling configured (c8/nyc/tarpaulin absent).
- Integration tests need a built binary — `npm run test:mcp` exits code 2 without it; use `npm run build:tests` (same preflight, exits 2 as well) when a gated variant is wanted.
- The same applies to `npm run test:web:auth` and `npm run test:web:basepath`: both require `target/debug/simple-post.exe` (or `--release`) on disk. `npm run perf:bench` is the exception — it resolves no binary and instead `fetch`es a **running** server (default `http://127.0.0.1:4173`, override with `--target`), so the server must be up separately.
