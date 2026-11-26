# 测试与质量门

> 从 `AGENTS.md` 拆出的按需文档，原文逐字保留。只有动到本域时才需要读它。

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

- **Test file placement — frontend.** Every test lives in a `__tests__/` directory beside the code it covers: `src/lib/__tests__/*.test.ts` and `src/stores/__tests__/*.test.ts`. A test that covers no single module in `src/` (a repo-wide sentinel, e.g. the image reference spelling check in `src/__tests__/release-image-ref.test.ts`) lives in `src/__tests__/`. The file is still named after the module (`url.test.ts` for `url.ts`), not after the folder. `__tests__` is a recognised convention (Create React App / Angular lineage) rather than *the* standard — co-location is the more common modern React default — but it is a legitimate one, and consistency inside this repo is what matters. The double underscore marks the directory as non-shippable without relying on a bundler honouring any name.
- **Test file placement — Rust.** There is no `__tests__` equivalent and no way to add one: Rust's module system resolves a file's location from the module path, so a test-only directory is not discoverable. The two options are the inline `#[cfg(test)] mod tests { … }` and a sibling file reached through `#[path]`. This repo uses the sibling form everywhere except five files still inline (`src-tauri/src/main.rs`, `crates/simple-post-server/src/{auth,main}.rs`, `crates/simple-post-server/src/web_api/static_files.rs`, `crates/simple-post-core/src/oauth2/http_client.rs`) — match the dominant form when you add tests:
  ```rust
  #[cfg(test)]
  #[path = "projects_test.rs"]
  mod tests;
  ```
  `super` resolves identically from a `#[path]` module, so the extracted body keeps its `use super::*;` unchanged and `cargo fmt --all` re-indents it. `#[path]` rather than the Rust 2018 `projects/tests.rs` submodule directory, so the pairing stays visible in directories that are otherwise flat `*.rs` files. When splitting one, **match the braces — do not assume the block's closing brace is the last line**: `db/mod.rs` has `pub mod test_counter` *after* its `mod tests`, and swallowing that tail would compile a `#[cfg(test)]` test helper into the library.
- **`npm test` auto-discovers `src/**/*.test.ts`** through `scripts/run-tests.mjs` — no hardcoded list, new test files are picked up automatically. In `--watch` mode the list is captured at spawn time: restart `npm run test:watch` after adding/renaming a test file. Single file: `npx tsx --test path/to/foo.test.ts`.
- **A test that resolves the module under test at runtime survives neither a move nor a rename.** `tauri-commands-shell.test.ts` probes `IS_TAURI_SHELL` in a `spawnSync` child because that flag is frozen at module-eval time, and resolves the module with `new URL('../tauri-commands.ts', import.meta.url).href`. That URL is relative to the *test file*, so moving the test silently broke it until the path gained its `../`. Typecheck and lint both passed on the broken version — only the child process failed. Grep for `import.meta.url` and `__dirname` whenever a test file changes directory.
- **界面动作的语义写在有测试的层。** 本仓没有组件测试（`src/` 下没有 `.test.tsx`，`npm test` 只跑 `src/**/*.test.ts`），所以写在 JSX 里的一切都无人观测：一个处理器自己拼「先加载、再刷新」「先提交、再清空」「先重载、再展开」，顺序错了、可选参数（`force`、`projectId`、是否重载）漏了，界面表现为「按了没反应」或「对话框关了但什么都没建」，而不会红任何用例。凡是带语义的组合——强制重取、清空草稿、绕过幂等早读、按某个 id 露出节点——都收成**一条命名的 store 动作**，在 `src/stores/__tests__/` 里断言它发出的命令载荷与结果落进哪个桶/键，界面只调这一条动作。已经这样做的：`refreshAllDynamicVariables`（漏 `force` 就退化成读 TTL 缓存）、`submitNewItemAndClose`（先清空草稿就落成默认名）、`reloadWorkspaceAndExpand`（先展开再重载会被整份替换的节点列表带走）、`wsConnecting`（分两次写，中间那一刻「已连上却还挂着第 N 次重试」）。反过来，单动作调用或只搭本地 React state 的组合不要为了统一风格搬进 store——搬动本身有成本。
- **能被静默破坏的接线用源码哨兵钉住。** 没有 DOM 也观察不到的东西（谁在什么时候调谁、某个文件里出现了哪种调用）写成读源码的用例，放在 `src/lib/__tests__/`（共用助手 `_source-scan.ts`）或所属 `__tests__/` 目录；规则要指向具体文件与行号，并在失败文案里写清「错在哪、用户会看到什么」，不要写成「整仓不许出现某个字符串」——那种宽规则会误伤合法用法。现有哨兵：`dynamic-resolve-call-sites.test.ts`（解析调用点必须点名项目；面板不得自己拼「解析 + 重载」）、`ui-action-semantics.test.ts`（界面层不得直写 store 状态；三个创建对话框只调 `submitNewItemAndClose`；开新连接不得自己拼 `wsRetry` + `wsState`）、`import-refreshes-projects.test.ts`、`dead-components.test.ts`。
- **删除测试的判据。** 一条用例只有在「它不可能因为真实缺陷变红」时才删（典型：只断言不抛异常，而同一份输入的同一条路径已被下一条用例断言了输出；断言实现细节；断言两个由同一处定义的常量互等），删除时必须在同一处说明被谁替代（如「与下一条同输入同函数」）或为什么这个行为不会再坏；反过来，「跑得慢」「改起来烦」「覆盖的数字看着多余」都不是删除理由——慢就压缩夹具，烦就重写断言，多余就说明它守的是哪条契约。拿不准就留着，并在报告里写清为什么怀疑它。
- ESLint scoping decisions (in `eslint.config.js`): `no-explicit-any` off under `**/*.test.ts` and `**/__tests__/` (deliberately loose mocks); `no-require-imports` off for `.mjs`/`.cjs` tooling; names starting with `_` are sanctioned unused placeholders; `react-hooks/rules-of-hooks` is **error** — `HttpTester`'s hooks were hoisted above its `if (!endpoint)` early return to satisfy it (never put hooks after an early return).
- Headless store tests need `src/stores/__tests__/_browser-shim.ts`: installs Map-backed `localStorage` (Node 22 ships a methodless native `localStorage` that crashes otherwise), `document.visibilityState` emitter, timers, and `mockIPC`, and sets `window.__TAURI_INTERNALS__ = {}` *before* module eval so `IS_TAURI_SHELL === true`.
- **No seed data ships with the app.** `db::init` runs `schema.sql` and stops — a fresh install is empty. `crates/simple-post-core/src/db/fixtures.json` is **test-only**: it is reachable solely through `#[cfg(test)] pub(crate) mod test_support` (`init_seeded` / `seed`), which does not exist in a production build. Because `test_support` is `pub(crate)`, sibling crates cannot use it — they build their own rows through the public CRUD API instead. There is **no** TS mock-data module; store tests build their own fixtures inline.
- Rust DB tests isolate via `fresh_pool()` → temp-dir SQLite keyed by atomic counter + PID (`crates/simple-post-core/src/db/mod_test.rs`, the `#[path]` module of `db/mod.rs`).
- Store custom hooks (`useActiveProject`, …) throw outside a React render; in Node, read state with `useAppStore.getState()`.
- No React component/DOM tests; no coverage tooling configured (c8/nyc/tarpaulin absent).
- **Race-prone component logic is still pin-able here.** With no component/DOM tests, move the logic that races (debounce, cancellation, generation guards) into a React-free module beside the component and test *that* module; the component keeps only the state writes. `src/features/dynamic-variables/node-search.ts` + `__tests__/node-search.test.ts` is the worked example: it injects its timers, so the test drives the schedule instead of waiting out the debounce.
- Integration tests need a built binary — `npm run test:mcp` exits code 2 without it; use `npm run build:tests` (same preflight, exits 2 as well) when a gated variant is wanted.
- The same applies to `npm run test:web:auth` and `npm run test:web:basepath`: both require `target/debug/simple-post.exe` (or `--release`) on disk. `npm run perf:bench` is the exception — it resolves no binary and instead `fetch`es a **running** server (default `http://127.0.0.1:4173`, override with `--target`), so the server must be up separately.
