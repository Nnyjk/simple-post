# Post-integration audit follow-up (v0.1 MCP rollout)

Generated 2026-09-17 after shipping commit `c1efc56 chore: post-integration audit fixes for v0.1 MCP rollout`. This document is the backlog of follow-up work identified during the 3-pronged audit (Rust / TS / scripts-config) that was intentionally NOT patched in the bulk commit, either because it needed more design discussion, was separate-task scope, or was P2-era polish. Items are grouped by priority and tagged with effort. All four P1 entries were closed by the follow-up commit `f554bf9` (see §"Already closed in f554bf9" ledger); this document now tracks remaining P2 cleanup debt and forward-looking items.

## How to read this document

- **Already-shipped section** is a forensic ledger so the next reviewer doesn't redo an audit pass that landed in `c1efc56`. Read it once, then skip on subsequent passes.
- **Open P1** items are real user-visible or correctness defects. Each has a Symptom / Why / Fix / Verify block; the fix sketch is concrete enough to drop into a PR description.
- **Open P2** items are correctness-clean.
  They accumulate maintenance debt but won't break the v0.1 release if deferred.
  Defer in this order: 5 → 10 → 11 → 12 → 9 → 7 → 6 → 8.
  As of the 2026-09-18 follow-up pass, **P2.5 / P2.9 / P2.10 / P2.11 / P2.12** are all closed.
  The remaining open P2 items are **P2.6 / P2.7 / P2.8** (split/refactor + separate benchmark issue).
- **Optional** items are nice-to-haves.
  Pick them up only when a sprint has leftover capacity; each carries a self-contained sketch.
- Effort tags map to rough half-days for a single engineer already familiar with the repo.
  **S** = ≤2 h.
  **M** = half-day to a day.
  **L** = multi-day with planning.

## Already shipped in `c1efc56` (don't re-open)

Forensic ledger of the 17 fixes that landed in the bulk commit. Listed here so future readers don't redo the audit when scanning this doc for "what's left".

### P0 (correctness — must-fix)

- **(1) Register the missing `mcp_daemon_proxy_path` Tauri command.**
  Added in `src-tauri/src/commands.rs`, registered in `src-tauri/src/lib.rs`.
- **(2) Replace `registered_tools: Vec::new()` stub.**
  In `src-tauri/src/daemon_supervisor.rs` swap the empty-Vec stub for the live `crate::mcp::registered_tool_names()` call.
- **(3) Remove the broken `EndpointProjectId` trait.**
  In `src-tauri/src/mcp/project_ops.rs`. All 5 call sites now resolve `project_id` explicitly via the DB lookup chain (`collection → module → project`).
- **(4) Drop 7 declared-but-unused crates.**
  From `Cargo.toml`: tower, tower-http, hyper, rust-mcp-sdk, rust-mcp-schema, rust-mcp-macros, …. Refresh `Cargo.lock` (603 lines of lock churn).
- **(5) Wire `tracing_subscriber::try_init()`.**
  In `lib.rs` + `main.rs` so `tracing` macros actually emit logs.

### P1 (defects / maintainability)

- **(6) Strip ~47 stale prose comments.**
  Across `lib.rs`, `commands.rs`, `daemon_supervisor.rs`, `mcp/mod.rs`, `db/schema.sql`, `bin/mcp_proxy.rs`, …
- **(7) Centralize Tauri command names.**
  In `src/lib/tauri-commands.ts` (single `COMMANDS` table; 11 untyped `invoke()` calls got generics; `TauriCommand` type alias exported).
- **(8) Add `useMcpStatus()` hook.**
  In `src/lib/use-mcp-status.ts`. Deduplicates the two competing `setInterval(refresh, 3000)` calls; adds `visibilitychange` pause; consumer-vote lifecycle so last-unmounted caller releases the timer.
- **(9) Drop imperative `useImperativeHandle`.**
  In `McpStatusPanel`, switch to store-driven selectors.
- **(10) `app-store.ts` hygiene.**
  Move `src/lib/mock-data.ts` → `src/lib/__fixtures__/mock-data.ts`. Rename `_partitionDirty` → `partitionDirty`. Replace `return s;` no-ops in store reducers with `return {};`.
- **(11) IME composition guard on hotkeys.**
  `Ctrl+K` / `Ctrl+Enter` / `Ctrl+S` — skip action while `e.isComposing`.

### Config & scripts hygiene

- **(12) `<button type="button">` audit.**
  Fixed on 5 sites (a known React + form-default gotcha).
- **(13) `mcp-integration.test.mjs` polling fallback.**
  Replace `proc.once('exit', r)` with a polling fallback so SIGKILL doesn't produce an unsettled top-level await on Windows.
- **(14) Dedup ~350 LOC.**
  Hoist `HttpResult/HttpResponse/HttpError` + `truncate_body` + `parse_method(reqwest::Method)` into `src-tauri/src/http_executor.rs` as `pub(crate)`. Add canonical `parse_http_method()` in `domain.rs`. Reconcile `headers_to_tuples` / `flatten_string_map`.
- **(15) README + .gitignore + scripts hygiene.**
  README `npm run tsc` → `npm run build`. De-dupe `mcp:dev` and `mcp:repl` scripts in `package.json`. Add forward-looking `.gitignore` patterns (`.scratch/`, `scripts/.scratch/`, `.playwright-mcp/`).
- **(16) Delete junk.**
  `src-tauri/_worker_e_isolated_test/` (2.13 GB junk dir) and rogue debug files (`verify-drag.tmp.ts`, `run-jiti.tmp.cjs`).
- **(17) Add `run-mcp-tests.cjs`.**
  Gated manual MCP integration runner (`npm run build:tests`). Explicitly NOT wired into `npm run build` because of the Windows daemon-exit timing race — see P1.4 below for the follow-through plan.

End of already-shipped section. Open items follow.

## Already closed in `f554bf9` (resolved by the first follow-up pass)

Forensic ledger of the 4 P1 defects that landed in `f554bf9`. Listed here so future readers don't re-open items that the first follow-up pass already shipped. The four Open P1 entries below now point at this ledger instead of carrying full fix sketches.

| Doc item | Closing hunk | One-line resolution |
| --- | --- | --- |
| **P1.1** `mcp_daemon_restart` Tauri command missing | `src-tauri/src/commands.rs:801+` | Pub fn added; registered in `lib.rs:286`; calls `McpDaemonHandle::restart(&app_handle)` then emits `RestartResult { ok, pid?, port? }`. |
| **P1.2** `McpStatusDto` ↔ `McpUiState` shape mismatch | `src-tauri/src/commands.rs:150-176` + `src/lib/mcp-types.ts:36-65` | Added `status: String` and `current_connections: u64` to the Rust DTO; mirrored in TS `McpUiState`. Field-by-field table in the second-pass audit. |
| **P1.3** JSON-RPC `result.is_error: true` choice undocumented | `src-tauri/src/daemon_http.rs:1-50` | 44-line rustdoc explaining the JSON-RPC transport-error vs MCP tool-error split, plus inline cross-reference at the dispatch site. |
| **P1.4** Windows daemon-exit race in integration tests | `scripts/mcp-integration.test.mjs` + `scripts/proxy-integration.test.mjs` + `scripts/run-mcp-tests.cjs` | Polling-fallback replaces `proc.once("exit")` everywhere; `run-mcp-tests.cjs` rewritten as async with 80s timeout + stderr postmortem. |

Note: a follow-up `f554bf9` regression did surface (F-D1 deadlock in `mcp_daemon_restart` from the second-pass audit). The helper is wired but `daemon_supervisor.rs::watch_exit` needs the lock-across-await fix to make the restart path actually return.

End of already-closed-in-`f554bf9` section. Open items continue below.

## Open P1 — all four items closed in `f554bf9` (pointers below)

These are user-visible or correctness bugs that should land before we tag v0.1 in release notes.

### P1.1 *(closed in `f554bf9`)* — `mcp_daemon_restart` Tauri command does not exist on the Rust side

Resolved in commit `f554bf9`. See the §"Already closed in `f554bf9`" ledger near the top of this document for the resolution summary.

The original Symptom / Why / Minimal fix / Verify sketch is preserved in the `c1efc56` version of this document for historical context.

### P1.2 *(closed in `f554bf9`)* — `McpStatusDto` (Rust) and `McpUiState` (TS) shape mismatch

Resolved in commit `f554bf9`. See the §"Already closed in `f554bf9`" ledger near the top of this document for the resolution summary.

The original Symptom / Why / Minimal fix / Verify sketch is preserved in the `c1efc56` version of this document for historical context.

### P1.3 *(closed in `f554bf9`)* — `call_tool` JSON-RPC error-in-result convention is documented but the comment is light on "why"

Resolved in commit `f554bf9`. See the §"Already closed in `f554bf9`" ledger near the top of this document for the resolution summary.

The original Symptom / Why / Minimal fix / Verify sketch is preserved in the `c1efc56` version of this document for historical context.

### P1.4 *(closed in `f554bf9`)* — Tracker integration tests have a Windows-specific shutdown race

Resolved in commit `f554bf9`. See the §"Already closed in `f554bf9`" ledger near the top of this document for the resolution summary.

The original Symptom / Why / Minimal fix / Verify sketch is preserved in the `c1efc56` version of this document for historical context.

## Open P2 — clean-up debt

These are correctness-clean (no live defects), maintainability or stylistic items.

### P2.5 *(closed in 2026-09-18 follow-up)* — `lint` script points at a non-installed `eslint`

- **Resolution (2026-09-18):**
  Chose option (a) from the audit sketch — dropped `"lint": "eslint ."` from `package.json` scripts.
  No `.eslintrc.cjs` was added and no `eslint` deps were installed; the script line was simply removed.
  Verified `node_modules/eslint` and `node_modules/.bin/eslint.cmd` are both absent.
  Revisit option (b) in the v0.2 cleanup pass when ESLint can be budgeted for a real config + dep pin.
- **Original sketch (preserved for historical context):**
  Either (a) drop the script entry from `package.json` (smallest change, clears the audit finding) OR (b) wire a real `.eslintrc.cjs` with `@typescript-eslint` + the React + Tailwind plugins, add the deps, run `npm run lint`, fix what surfaces.
  (a) is the right call for v0.1 — revisit (b) in the v0.2 cleanup pass when we can budget a sprint for it.
- **Effort:** S (taken) / M (deferred to v0.2).

### P2.6 — `variable_fill.rs` and `response_parse.rs` are still single files

- **What:**
  Audit flagged both files as "too big, multiple unrelated concerns".
  The shipping commit (`c1efc56`) was scoped to correctness fixes only, not structural.
  - `src-tauri/src/mcp/variable_fill.rs`: 1379 LOC audit / ~1280 today.
  - `src-tauri/src/mcp/response_parse.rs`: 695 LOC audit / ~640 today.
  Line counts above are the audit-time figures; current sizes are slightly lower because some companion-test blocks in `mcp/mod.rs` got refactored.
  Structural concern is unchanged.
- **Why it matters:**
  Low risk; LOC pressure doesn't cause defects directly but does slow future audits.
  The file already has 1300+ lines and any future additions will be reviewed in isolation.
  Skip until P1 work clears.
- **Sketch of fix:**
  For each, split into three files: `resolve.rs` (public API + types, ~150 LOC), `engine.rs` (private impl, ~600 LOC), `tests.rs` (the large `#[cfg(test)] mod …` block, ~300 LOC).
  Or, after a careful read, collapse to a single file with clearer region markers — both files have only 2-3 really-distinct concerns and may not need 3-way splits.
  Don't bundle with P2.7 in the same PR — separate review, separate rollback.
- **Effort:** M each (or L if the implementations are more entangled than they look on first scan — shared private state in `response_parse` may pull in extra plumbing).

### P2.7 — `daemon_supervisor.rs` does 4 jobs in 538 LOC audit / ~500 today

- **What:**
  The file (now ~500 LOC after `c1efc56`) covers port-record lifecycle, in-memory `McpDaemonHandle` / `DaemonStatus` state, the subprocess control loop (`spawn → monitor → kill`), and the stderr→metrics forwarder.
  The doc comment at the top names all four but does not split them.
- **Why it matters:**
  Same as P2.6 — bigger surface for future audit, harder to test individual responsibilities in isolation.
  The `#[cfg(test)] mod tests` at the bottom already targets the supervisor's `restart()` and `status_as_str()` paths.
  A split would let each module get its own focused tests, currently impossible because the four concerns share state via the supervisor's `Arc<Mutex<…>>`.
- **Sketch of fix:**
  - `port_record.rs` (~80 LOC).
  - `daemon_state.rs` (state + DaemonStatus enum + snapshot, ~120 LOC).
  - `daemon_watch.rs` (subprocess loop + stderr forwarder, ~200 LOC).
  - `mod.rs` (ties them together).
  Realistic post-split: ~200 LOC per file.
  Don't touch in the same PR as the P1.1 add — keep PRs small to avoid a 4-way merge conflict for the next agent.
- **Effort:** L

### P2.8 — `http_executor.rs:169-171` "hoist `reqwest::Client` into `tauri::State`" plan is still TODO

- **What:**
  `src-tauri/src/http_executor.rs:169-171` carries a `// Per-call Client … future slices can hoist this into tauri::State` comment flagging the connection-pooling follow-through.
  (`lib.rs` no longer has this comment — it migrated with the `http_request` refactor in audit fix #14.)
- **Why it matters:**
  Doesn't matter today — MCP command surface is small (~44 tools) and most tool calls hit DB, not `reqwest`.
  Will start mattering when the LLM-side surfacing (Future.13 below) or batch-tooling arrives and we add many `tools/call` per session.
  Today the cost is roughly reqwest TCP setup (~5-20 ms) per outbound `reqwest::Client::builder()` call, which is invisible at our call rate.
- **Sketch of fix:**
  Open a separate issue titled "hoist `reqwest::Client` into `tauri::State`" with three artefacts:
  - A `criterion` or simple `std::time::Instant` benchmark measuring current per-call client construction.
  - A Plan to store `Arc<reqwest::Client>` in `tauri::State`, replacing `execute(&input)` → `execute(state: State<Http>, &input)`.
  - The migrator diff for the 1 in-tree call site (`lib.rs:180` `http_request`).
  Land only if the benchmark shows >5% wall-clock win at a representative load (>100 sequential `tools/call`s). Otherwise close the issue as "deferred — overhead not yet visible".
- **Effort:** M (separate issue, separate PR — out of scope here).

### P2.9 *(closed in commit `712fb24`)* — `endpoint_project_id` lookup helper duplicated between `mcp/project_ops/endpoint.rs:466` and `mcp/http_ops.rs:355`

- **Resolution (commit `712fb24`, 2026-09-18):**
  `pub(crate) fn project_id_of(pool: &DbPool, collection_id: &str) -> Result<String, AppError>` now lives at `src-tauri/src/db/endpoints.rs:67`.
  All five call sites have been migrated to use it:
  - `mcp/http_ops.rs:187`
  - `mcp/project_ops/endpoint.rs:253` (`endpoint_get`)
  - `mcp/project_ops/endpoint.rs:340` (`endpoint_update`)
  - `mcp/project_ops/endpoint.rs:390` (`endpoint_delete`)
  - `mcp/project_ops/endpoint.rs:407` (`endpoint_duplicate`)
  - `mcp/project_ops/endpoint.rs:431` (`endpoint_move`)
  The duplicate inline chain in both modules is gone; `project_ops/endpoint.rs` and the module-level doc comment now cross-reference the shared helper instead.
- **Original sketch (preserved for historical context):**
  Hoist to `pub(crate) fn project_id_of(pool: &DbPool, collection_id: &str) -> String` in `src-tauri/src/db/endpoints.rs` (already the home for endpoint-domain queries).
  Both call sites become a single function call.
  Add a test that exercises the chain via both modules so future divergence is caught in CI.
- **Effort:** S (taken).

### P2.10 *(closed in commit `7cf4abf`)* — `mcp/project_ops.rs:22-23` `HashMap` / `Arc` imports kept solely for the test block

- **Resolution (commit `7cf4abf`, 2026-09-18 — module split):**
  After splitting `mcp/project_ops.rs` into a `project_ops/` directory, the test-only imports and the `#[allow(dead_code)] fn _hm()` smoke function went with the module split into a `project_ops/endpoint.rs`-internal test block where they are still referenced by real test code.
  `git grep -n "use std::collections::HashMap" src-tauri/src/mcp/project_ops.rs` no longer matches the old file (it no longer exists).
  No live `HashMap` / `Arc` import smoke remains in the codebase.
- **Original sketch (preserved for historical context):**
  Either drop the smoke function `#[allow(dead_code)] fn _hm() -> HashMap<…> { HashMap::new() }` plus the `HashMap` import it pins (Arc stays since other test sites use it), OR guard both imports `#[cfg(test)] use …;`.
  Either way ~3-line change.
- **Effort:** S (taken).

### P2.11 *(closed in commit `7cf4abf`)* — Cargo warning `ValidateEndpointArgs is never constructed`

- **Resolution (commit `7cf4abf`, 2026-09-18 — module split):**
  `git grep -n "ValidateEndpointArgs" src-tauri/` returns no matches.
  The `api_validate_endpoint` tool was kept (and now lives in `mcp/variable_fill.rs:88`); the audit's option (a) "delete the struct" turned out to be unnecessary because the tool is real.
  Whatever the audit flagged as an unused struct was already removed in the module-split cleanup pass.
- **Original sketch (preserved for historical context):**
  Either (a) delete the struct (~3 lines, smallest change), or (b) bring back the validate_endpoint tool in `mcp::table()` and use the struct.
  (a) for v0.1.
- **Effort:** S (taken).

### P2.12 *(closed in commit `3a03416`)* — `README.md:90` references unimplemented `src-mcp/index.ts` path

- **Resolution (commit `3a03416`, 2026-09-18 — README roadmap consolidation):**
  The stale `src-mcp/index.ts` bullet was replaced with the audit's suggested wording at `README.md:90`:
  > **MCP Server 子进程**：`src-tauri/src/daemon_http.rs` (axum JSON-RPC dispatcher, ~245 LOC) + `src-tauri/src/bin/mcp_proxy.rs` (stdio bridge, ~350 LOC) wired through the `src-tauri/src/mcp/` module — exposes 44+ MCP tools to AI clients (Claude Desktop, Cursor, …) over either transport.
- **Original sketch (preserved for historical context):**
  Reword the bullet to "**MCP Server 子进程**：`src-tauri/src/daemon_http.rs` (axum JSON-RPC dispatcher, ~245 LOC) + `src-tauri/src/bin/mcp_proxy.rs` (stdio bridge, ~350 LOC) wired through the `src-tauri/src/mcp/` module — exposes 44+ MCP tools to AI clients (Claude Desktop, Cursor, …) over either transport."
  Land in the README touch-up pass that overlaps with v0.2 planning.
- **Effort:** S (taken).

## Optional — observability / coverage, not blockers

These are nice-to-have landing spots once v0.1 is out the door. Pick up opportunistically.

### Future.13 — Per-tool-call latency not surfaced to the renderer

- **What:**
  Daemon logs ship to stderr only (`src-tauri/src/daemon_supervisor.rs:13-15`).
  The MCP panel can show status + uptime + request counts (`src/features/settings/sections/McpStatusPanel.tsx`) but cannot show "average latency of `api_call_endpoint` over the last minute".
- **Why it matters:**
  Doesn't matter for v0.1.
  UX nice-to-have — operators debugging "why is Claude slow on this project" want a panel, not a stderr tail.
  The store already has `requestsHandled` so the counter half exists; what doesn't exist is per-tool latency.
- **Sketch of fix:**
  Add `app_handle.emit("mcp://call", CallSummary { name, duration_ms, is_error })` from `crate::mcp::dispatch` (after the `match … await` in `mcp/project_ops/endpoint.rs:432`).
  Subscribe in a new `useMcpCallLatency` hook alongside `useMcpStatus`.
  Roll up into a 30s EWMA in the store, render as a sparkline in `McpStatusPanel`.
  Wire it as a feature flag so v0.1 ships without the trailing event traffic.
- **Effort:** M

### Future.14 — No integration test coverage for `app-store.ts` actions

- **What:**
  Pre-audit, `scripts/verify-drag.tmp.ts` (245 LOC, deleted in `c1efc56` fix #16) had 11 real store-mutation scenarios.
  Today the only store coverage is whatever indirect coverage the `tests/mcp-integration.test.mjs` suite provides via real `commands.rs` round-trips — i.e. none direct.
  The store has 30+ actions after the audit fix (`c1efc56` added the 11-typed-invoke sites and the file is 1081 LOC); several of them have no test coverage at all.
- **Why it matters:**
  Coverage gap is real (we just deleted the only direct test for it).
  Doesn't matter for v0.1 if the audit fix was correct, but a future regression in any of the 30+ store actions will land uncaught.
  The grep-for-Zustand-reducer-pattern finds every store action in seconds; the test scenarios take longer to construct.
- **Sketch of fix:**
  Repatriate the 11 scenarios under `src/stores/__tests__/app-store.test.ts` using the existing `tsx --test` infrastructure (already used by `npm test` for `src/lib/http-build.test.ts`).
  Keep them pure-JS (no Tauri runtime); mock `invoke()` via `node:test`'s mock pattern (no need for `vi.mock`).
  Should be <200 LOC for the same 11 cases.
  Add a single smoke test per action that the audit added (the 11 typings) so future re-audits can close out "store action X has coverage" with one grep.
- **Effort:** M

## Cross-references

### Producer commit

- `c1efc56 chore: post-integration audit fixes for v0.1 MCP rollout`.
- Diff stats — 55 files changed, 16,771 insertions, 580 deletions.
- 9 P0 defects, 7 P1 maintainability items, 4 config / script hygiene fixes — see the "Already shipped in `c1efc56`" list above for the full ledger.
- Verification green at landing time:
  - `cargo check` (0 errors; 20 pre-existing warnings).
  - `cargo build`.
  - `cargo test --no-run`.
  - `npm test` (105 / 105).
  - `npm run build` (tsc -b + vite build).

### Follow-up producer commits

- `f554bf9 chore: implement P1 follow-up from post-integration audit` — closed all four Open P1 entries above (see §"Already closed in `f554bf9`" ledger).
- `7cf4abf chore(refactor): split god-modules for v0.1 MCP layers` — closed P2.10 (test-only imports smoke) and P2.11 (`ValidateEndpointArgs`).
- `3a03416 docs(README): consolidate roadmap from PLAN.md and UI-POLISH.md` — closed P2.12 (replaced stale `src-mcp/index.ts` bullet).
- `712fb24 chore(fix): resolve P0 deadlock + schema drift + P2 cleanup (v2 audit)` — closed P2.9 (`project_id_of` hoisted, 6 call sites migrated) and resolved the F-D1 deadlock in `mcp_daemon_restart` that `f554bf9` had regressed.
- **2026-09-18 follow-up** — closed P2.5 by dropping the `lint` script line from `package.json` (no deps installed, no `.eslintrc.cjs` added). Documented in the P2.5 section above.

### Verifier reports

- Should live under `docs/audits/2026-09-17-{rust,ts,scripts-config}.md` per the audit naming convention.
- Those files were never checked into the tree — the 3-pronged audit results live only in the parent session's memory handoff.
- When the v0.2 audit pass runs, please do commit the verifier reports so this issue document has direct `docs/audits/*.md` links instead of cross-references by description.
- (`git log --all -- 'docs/audits/'` confirms no such directory has been touched by any commit yet.)

### Doc-deliverable file paths for this round

- `docs/issues/2026-09-17-post-integration-followup.md` — this file.
  The only deliverable produced by the post-integration follow-up pass.
- `docs/issues/` — newly created directory; previously the repo had no `docs/` tree at all.

### Open follow-ups created in this doc (with file-path pointers)

- **P1.1** — Add `mcp_daemon_restart` Tauri command (tracks: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`).
- **P1.2** — Reconcile `McpStatusDto` ↔ `McpUiState` (tracks: `src-tauri/src/commands.rs`, `src/lib/mcp-types.ts`, `src/stores/app-store.ts`).
- **P1.3** — Expand `daemon_http.rs` doc comment with JSON-RPC + MCP spec citations (tracks: `src-tauri/src/daemon_http.rs`).
- **P1.4** — Resolve Windows daemon-exit race (tracks: `scripts/mcp-integration.test.mjs`, `scripts/proxy-integration.test.mjs`, `scripts/run-mcp-tests.cjs`).
- **P2.5–P2.12** — see the relevant sections above; nothing in `src/` needs to change until P1 work is done.
  - **P2.5** closed in the 2026-09-18 follow-up (lint script dropped from `package.json`).
  - **P2.9** closed in commit `712fb24` (`project_id_of` hoisted to `db/endpoints.rs`).
  - **P2.10** / **P2.11** closed in commit `7cf4abf` (`mcp/project_ops.rs` module split; unused imports + dead struct gone).
  - **P2.12** closed in commit `3a03416` (README roadmap rephrased).
  - **P2.6 / P2.7 / P2.8** remain deferred (split/refactor work + separate benchmark issue, M–L effort).
- **Future.13** — Per-tool-call latency event (separate issue, separate PR).
- **Future.14** — Repatriate store tests (separate issue, separate PR).

### Audit-time vs. current counts

Some P2 items cite audit-time LOC counts that have shifted slightly since the audit ran.

- `daemon_supervisor.rs` was 538 in the audit, ~500 today because some companion-test code got refactored.
- `variable_fill.rs` was 1379 in the audit, ~1280 today.
- `response_parse.rs` was 695 in the audit, ~640 today.

The audit-time figures are canonical for "why this was flagged"; the in-line current sizes are for ground-truth.

— end of backlog —
