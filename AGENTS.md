# Repository Guidelines

## Project Overview

`simple-post` is a desktop API client/debugger (Postman-like, Windows-first) whose core differentiator is a **built-in MCP server**: external AI clients (Claude Desktop, Cursor, Cline) can list/search/run/mutate endpoints through 44+ MCP tools. Stack: **Tauri 2 + React 18 + TypeScript** frontend over a **Cargo workspace** (SQLite via `rusqlite`, `reqwest`, `axum`), shipping both a desktop app and a standalone npm-distributed MCP binary.

Docs are partly Chinese (`README.md`, crate READMEs); code/identifiers are English.

## Architecture & Data Flow

Four runtime pieces:

| Piece | Path | Role |
|---|---|---|
| Tauri shell + CLI | `src-tauri/` | Desktop GUI, `serve-web` REST/SPA server, daemon supervisor, port registry |
| Core library | `crates/simple-post-core/` | Domain models, SQLite (`db/`), `http_executor`, `permissions`, `mcp_metrics`, `error` — headless, no Tauri/axum dep |
| MCP server | `crates/simple-post-mcp/` | JSON-RPC 2.0 dispatch (44 tools), axum HTTP daemon, stdio bridge |
| Frontend | `src/` | React 18 + Vite 5 + Zustand 5 + Tailwind 3 + CodeMirror 6 |

**CLI modes** (`src-tauri/src/main.rs`, clap): default → Tauri GUI; `mcp` → embedded MCP daemon; `mcp-proxy` → stdio bridge; `serve-web` → headless REST + static SPA.

**CRUD flow** — component → Zustand slice (optimistic update) → `invokeCmd()` (`src/lib/tauri-commands.ts`) → Tauri IPC **or** `fetch('/api/…')` → `src-tauri/src/commands.rs` / `web_api.rs` → `simple_post_core::db` → `store.db` (WAL).

**HTTP requests** bypass webview CORS: `src/lib/http.ts::sendRequest` → `httpRequest` command → `http_executor.rs` (reqwest).

**MCP flow**: AI client → stdio (`stdio_loop.rs`) → `POST 127.0.0.1:<port>/mcp` → `daemon_http.rs` → `mcp/mod.rs::dispatch` → capability gate (`read_only`/`safe_write`/`full_access`) → per-project permission check → tool handler → shared `db` pool / `http_executor`. Daemon port discovered via `mcp.port` file in the data dir (`SIMPLE_POST_DATA_DIR` or `%APPDATA%/simple-post`).

Dev split: Vite `5173` proxies `/api` + `/mcp` to axum `4173`.

## Key Directories

- `src/features/` — feature panels: `http-tester/`, `project-tree/`, `settings/`, `command-palette/`, `top-bar/`, `workspace/`, `response-viewer/`, `welcome/`
- `src/stores/` — Zustand: `app-store.ts` barrel composing `bootstrap-slice`, `crud-slice`, `ui-slice`, `mcp-slice`, `dirty-tabs-slice` + `store-types.ts`
- `src/lib/` — `tauri-commands.ts` (dual-transport IPC + `COMMANDS` registry), `http.ts`, `http-build.ts`, `mcp-types.ts`, `use-mcp-status.ts`
- `src/components/ui/` — shared primitives (`method-picker`, `tabs`, `dialog`, …)
- `src/types/domain.ts` — TS mirrors of Rust domain types
- `crates/simple-post-core/src/db/` — per-entity SQL modules + `schema.sql` + `fixtures.json`
- `crates/simple-post-mcp/src/mcp/` — tool handlers (`project_ops/`, `env_ops.rs`, `http_ops.rs`)
- `src-tauri/src/` — `commands.rs`, `web_api.rs`, `daemon_supervisor.rs`, `port_registry.rs`, `lib.rs`, `main.rs`
- `packages/simple-post-mcp/` — independent npm package wrapping prebuilt native binaries
- `scripts/` — dev/test/build utilities (see below); `docs/` — HTTP reference, OpenAPI, MCP permission policy
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

npm test               # frontend unit tests (auto-discovers src/**/*.test.ts — see Testing)
npm run test:watch     # same, watch mode
npm run test:mcp       # MCP end-to-end (requires compiled binary)
npm run test:web:auth      # serve-web bearer auth flow integration (needs binary)
npm run test:web:basepath  # serve-web reverse-proxy base path integration (needs binary)
npm run perf:bench         # serve-web p50/p95/p99 latency (--assert enforces 200ms p95 budget)
npm run build:tests    # gated MCP test; exits 0 if binary missing
npm run mcp:repl       # interactive JSON-RPC REPL against a running daemon

cargo test --workspace              # all Rust tests
cargo test -p simple-post-core      # single crate
node scripts/dump-openapi.mjs > docs/openapi.yaml   # regenerate OpenAPI after web_api.rs route changes
```

Lint/format tooling: **ESLint 10 (flat config) + Prettier 3**. ESLint owns correctness (`rules-of-hooks`, unused vars, …); style belongs to Prettier (`eslint-config-prettier` sits last in the config). Rust side: `cargo fmt`, `cargo clippy` with `-D warnings`.

**CI quality gate** — `.github/workflows/ci.yml` runs on every push to `main` and every PR (release / release-mcp / docker workflows only publish):
- `web` (ubuntu, Node 22): `format:check` → `lint` → `typecheck` → `npm test` → `vite build`
- `rust` (windows-latest): `cargo fmt --check` → `clippy -D warnings` → `cargo test --workspace`

Keep `npm run check` green locally before pushing — CI runs the same checks.

## Code Conventions & Common Patterns

**Error handling.** Rust returns `Result<T, AppError>` (`crates/simple-post-core/src/error.rs`); `AppError` serializes to a stable `{ code: <snake_case>, message }` wire shape. MCP tool failures are returned *inside* `CallToolResult` with `is_error: true` (not a JSON-RPC envelope error) so LLM clients can observe them. Frontend never throws for network failure — `ApiResponse.status === 0` signals transport error.

**DB access.** Reads → `db::with_conn`; writes → `db::with_tx` (`BEGIN IMMEDIATE`, 3 retries / 50 ms backoff on `SQLITE_BUSY`). WAL + `foreign_keys=ON` + `busy_timeout=3000`. Complex fields (`KeyValue[]`, `RequestBody`, `AuthConfig`, `tags`) live in `*_json` text columns. New entity SQL goes in `crates/simple-post-core/src/db/<entity>.rs`.

**Partial updates.** Patch types (`EndpointPatch`, …) use `#[serde(default, deserialize_with = "double_option")] Option<Option<T>>`: missing key = leave unchanged, JSON `null` = clear, value = set (`crates/simple-post-core/src/domain.rs`). Follow this pattern for any new updatable field.

**Serialization.** All domain types: `#[serde(rename_all = "camelCase")]`; TS counterparts in `src/types/domain.ts`. Keep both in sync when adding fields.

**Zustand selectors (infinite-loop hazard).** Equality is `Object.is`; a selector returning a fresh array/object (`.map`, `.filter`, spread) causes infinite re-render. Selectors must return stable references or `.find(...)`; derive filtered lists with `useMemo` in the component. Slices never import each other — they share state via the `AppState` interface and `get()`/`set()`. Verify with `npx tsx scripts/_verify_store.mjs`.

**Dual-transport invariant.** Any new backend command needs **five** coordinated edits: handler in `src-tauri/src/commands.rs`, registration in `src-tauri/src/lib.rs`, a `COMMANDS` constant in `src/lib/tauri-commands.ts`, a route in `src-tauri/src/web_api.rs`, and a `WEB_ROUTES` entry (if browser-callable). Missing one side breaks either desktop or web mode silently.

**Tauri macro gotcha.** `http_request` in `src-tauri/src/lib.rs` must stay **non-`pub`** — marking it `pub` triggers a `__cmd__http_request` identifier collision (E0255) between `#[tauri::command]` and `generate_handler!`.

**Routing.** Axum 0.7 path params use `:id` syntax (`/api/projects/:id`), not `{id}`.

**Naming.** Rust: `snake_case` functions/modules/columns, `PascalCase` types. Tauri commands `snake_case` in Rust, `camelCase` in the frontend `COMMANDS` registry. Test files: `*.test.ts`; Rust tests inline as `#[cfg(test)] mod tests`.

**Async/concurrency.** tokio multi-thread runtime; metrics via `AtomicU64` with `Ordering::Relaxed`; `port_registry.rs` uses OS atomic file creation (`create_new`) to prevent multi-instance races.

**MCP stdout discipline.** In `simple-post-mcp`, stdout carries *only* JSON-RPC frames; all logging, panics, and `__mcp_metrics__{…}` telemetry go to **stderr**. Non-JSON on stdout crashes AI clients.

**MCP permissions.** Default policy is `read_only` — mutating tools (`env_create`, `endpoint_update`, …) return `is_error: true` until the user raises the tier. Consult `docs/mcp-permission-policy.md` before touching dispatch/permissions.

**IME guard.** Global shortcuts in `src/App.tsx` must check `e.isComposing`, else CJK IME Enter fires requests.

**OpenAPI.** `public_route_specs()` in `src-tauri/src/web_api.rs` is the single source of truth; internal routes (`/api/mcp/*`, `/api/preferences`) are intentionally undocumented.

## Important Files

- `src/main.tsx`, `src/App.tsx` — frontend entry; theme preload, global shortcuts, layout
- `src/lib/tauri-commands.ts` — `IS_TAURI_SHELL` detection, `invokeCmd`, `COMMANDS`, `WEB_ROUTES`
- `src/stores/app-store.ts` — store barrel + safe selector hooks
- `src-tauri/src/main.rs` / `src-tauri/src/lib.rs` — CLI dispatch / Tauri `run()`
- `src-tauri/src/web_api.rs` — REST surface for `serve-web`; OpenAPI source
- `crates/simple-post-core/src/db/mod.rs` + `schema.sql` — pool, transactions, migrations, seed
- `crates/simple-post-core/src/domain.rs`, `error.rs` — canonical types + wire errors
- `crates/simple-post-mcp/src/mcp/mod.rs` — 44-tool registry, `REQUIRED_CAPABILITY`, `dispatch`
- `Cargo.toml` — workspace root (members, shared deps, release profile)
- `src-tauri/tauri.conf.json` — window, NSIS bundle, build hooks, CSP (intentionally `null`)
- `src-tauri/rust-toolchain.toml`, `src-tauri/capabilities/default.json`
- `vite.config.ts` — alias, dev server, `/api` + `/mcp` proxy (ws disabled on `/mcp`)
- `tsconfig.json`, `package.json`, `tailwind.config.js`
- `eslint.config.js`, `.prettierrc`, `.prettierignore` — lint/format rules and scope
- `.editorconfig`, `.gitattributes` — LF-everywhere policy (only `*.bat`/`*.cmd` are CRLF)
- `.github/workflows/ci.yml` — push/PR quality gate; `release.yml` / `release-mcp.yml` / `docker-build.yml` only publish
- `scripts/run-tests.mjs` — test discovery runner behind `npm test`

## Runtime/Tooling Preferences

- **Node `>=20 <23`** (root `package.json` engines); package manager is **npm** (lockfile v3). No pnpm/yarn/bun — do not add their lockfiles.
- Root `package.json` declares **no workspaces**; `packages/simple-post-mcp` is an independently published package (`@simple-post/mcp-server`).
- **Rust**: edition 2021, MSRV 1.77.2, toolchain channel **`stable`** with `rustfmt` + `clippy` (`src-tauri/rust-toolchain.toml`). Track `stable` so edition-2024 transitive deps (clap 4.6+, etc.) compile; if you need a pinned baseline, set `channel = "1.85.0"` (edition2024 stable release) or newer and run `rustup toolchain install` first.
- TypeScript path alias `@/*` → `src/*`; `moduleResolution: bundler`, `noEmit` (type checks run via `npm run typecheck` / `tsc -b`, also embedded in `npm run build`).
- Workspace builds output to `<repo>/target/`, not `src-tauri/target/`.
- **Line endings: LF everywhere** — `.gitattributes` (`* text=auto eol=lf`) overrides `core.autocrlf`; `.editorconfig` mirrors it for editors (UTF-8, final newline, trim whitespace, 2-space default / 4 for `*.rs`, Markdown keeps trailing spaces). Only `*.bat` / `*.cmd` stay CRLF.
- **Prettier scope**: `*.md`, lockfiles, and generated `docs/openapi.yaml` are in `.prettierignore` — hand-maintained prose and generated artifacts stay byte-stable.
- Windows-first: PowerShell helpers in `scripts/*.ps1`. When writing UTF-8 files from PowerShell, specify `-Encoding utf8NoBOM` — 5.1's default ANSI encoding corrupts UTF-8 and can break `rustc`.
- Multi-worker convention (per `README.md`): parallel workers deliver code only; end-to-end/browser verification is done once by the main session.

## Testing & QA

**No Vitest/Jest/React Testing Library.** Frontend tests use the Node built-in runner via `tsx --test` + `node:assert/strict`.

| Layer | Command | Location |
|---|---|---|
| Frontend unit/slice | `npm test` / `npm run test:watch` | `src/lib/*.test.ts`, `src/stores/__tests__/*.test.ts` |
| Rust | `cargo test --workspace` / `-p <crate>` | inline `#[cfg(test)]` modules |
| MCP end-to-end | `npm run test:mcp` | `scripts/mcp-integration.test.mjs` (needs compiled `simple-post` binary) |
| Web auth integration | `npm run test:web:auth` | `scripts/test-web-auth.mjs` (needs compiled binary; spawns two phases — open + locked) |
| Web base-path integration | `npm run test:web:basepath` | `scripts/test-web-basepath.mjs` (needs compiled binary; `--public-base-path /sp`) |
| Web perf bench | `npm run perf:bench` | `scripts/perf-bench.mjs` (p50/p95/p99 over N hits; `--assert` gates 200ms p95) |
| Gated CI variant | `npm run build:tests` | `scripts/run-mcp-tests.cjs` (exits 0 if binary absent) |
| Store invariant check | `npx tsx scripts/_verify_store.mjs` | selector stability, cascade delete, prefs persistence |

Key facts:

- **`npm test` auto-discovers `src/**/*.test.ts`** through `scripts/run-tests.mjs` — no hardcoded list, new test files are picked up automatically. In `--watch` mode the list is captured at spawn time: restart `npm run test:watch` after adding/renaming a test file. Single file: `npx tsx --test path/to/foo.test.ts`.
- ESLint scoping decisions (in `eslint.config.js`): `no-explicit-any` off under `**/*.test.ts` and `**/__tests__/` (deliberately loose mocks); `no-require-imports` off for `.mjs`/`.cjs` tooling; names starting with `_` are sanctioned unused placeholders; `react-hooks/rules-of-hooks` is **error** — `HttpTester`'s hooks were hoisted above its `if (!endpoint)` early return to satisfy it (never put hooks after an early return).
- Headless store tests need `src/stores/__tests__/_browser-shim.ts`: installs Map-backed `localStorage` (Node 22 ships a methodless native `localStorage` that crashes otherwise), `document.visibilityState` emitter, timers, and `mockIPC`, and sets `window.__TAURI_INTERNALS__ = {}` *before* module eval so `IS_TAURI_SHELL === true`.
- Fixtures: `src/lib/__fixtures__/mock-data.ts` (TS), `crates/simple-post-core/src/db/fixtures.json` (SQLite seed — also used at DB init).
- Rust DB tests isolate via `fresh_pool()` → temp-dir SQLite keyed by atomic counter + PID (`crates/simple-post-core/src/db/mod.rs`).
- Store custom hooks (`useActiveProject`, …) throw outside a React render; in Node, read state with `useAppStore.getState()`.
- No React component/DOM tests; no coverage tooling configured (c8/nyc/tarpaulin absent).
- Integration test needs a built binary — `npm run test:mcp` exits code 2 without it; use `npm run build:tests` when a soft gate is wanted.
- The same applies to `npm run test:web:auth`, `npm run test:web:basepath`, and `npm run perf:bench` — all three require `target/debug/simple-post.exe` (or `--release`) on disk. Plan verification of commit 5's auth/base-path landed in commit 7 (this commit) per the deferred-to-commit-7-hygiene note in commit 5's message.
