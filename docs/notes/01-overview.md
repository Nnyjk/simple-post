> 适用范围：第一次接手、要在一小时内跑起来并看懂仓库形状的人或 agent
> 权威来源：README.md §这是什么/§部署、AGENTS.md §Project Overview/§Key Directories/§Development Commands、src-tauri/src/main.rs、crates/simple-post-server/src/main.rs、package.json、Cargo.toml
> 最后核对：2026-11-13（v0.1.34）
> 前置阅读：README.md

# 01 总览

## 一句话结论

Simple Post 是 **Windows 优先、本地优先的桌面 API 调试工具**（Postman 类），差异化在于**内置 MCP Server**：Claude Desktop / Cursor / Cline 等外部 AI 客户端可以直接读、搜、发、改这里的接口定义。整套业务逻辑只有一份 Rust 核心，交付成三种形态：桌面 app、无头 `serve-web`（REST + SPA）、`simple-post-mcp` 桥。

## 三种交付形态与入口

| 形态 | 入口文件 | 启动方式 | 说明 |
| --- | --- | --- | --- |
| 桌面 GUI | `src-tauri/src/main.rs`（无子命令）→ `simple_post_lib::run()`（`src-tauri/src/lib.rs`） | `npm run tauri:dev` / 安装包 | 链接 `tauri` 与 webview；`commands/` 是 IPC 面 |
| 无头 Web（桌面二进制） | `src-tauri/src/main.rs` 的 `serve-web` 分支 → `simple_post_server::cli::serve_web` | `simple-post serve-web --bind 0.0.0.0 --port 4173` | 与容器同一套实现，只是宿主不同 |
| 无头 Web（容器） | `crates/simple-post-server/src/main.rs` | `simple-post-server serve-web ...` | **不链接 Tauri**，故镜像里没有 GTK/WebKit；这是 Docker 的命令 |
| MCP daemon | `simple-post mcp` / `simple-post-server mcp` → `simple_post_mcp::mcp` + `server::mcp_router` | 由 supervisor 作为子进程拉起，或手工起 | 监听**自己的**端口（`mcp.http_config`），主端口不提供 `/mcp` |
| MCP stdio 桥 | `crates/simple-post-mcp/src/main.rs` | `simple-post-mcp --url=… --password=…` | 只以编译产物分发（`cargo install --path crates/simple-post-mcp`）；不给 `--url` 时从 `mcp.port` 发现端口 |

`src-tauri/src/main.rs` 的 clap 只有四个分支：默认 GUI、`mcp`、`mcp-proxy`、`serve-web`。`mcp-proxy` **故意拒绝运行**（退出码 2），把用户指向独立的 `simple-post-mcp` 二进制——主二进制里不重复一份 stdio 桥。

## 技术栈

| 层 | 选型 | 备注 |
| --- | --- | --- |
| 前端 | React 19 + TypeScript 6 + Vite 8（Rolldown）+ Zustand 5 + Tailwind 4 + CodeMirror 6 | 测试用 Node 内置 runner（`tsx --test`），没有 Vitest/Jest/RTL |
| 桌面壳 | Tauri 2（`tauri` 2.x、CLI 2.12） | CSP 有意为 `null`；插件只用 `dialog` + `fs`，权限按需列举 |
| 后端 | Rust 1.87（edition 2021）、`axum` 0.8、`reqwest` 0.12（`rustls-tls`） | `reqwest` 刻意钉 0.12：0.13 移除了 webpki-roots，会失去内置 Mozilla 根证书集 |
| 存储 | SQLite（`rusqlite` bundled）+ `r2d2` 池，WAL + `foreign_keys=ON` | 单文件 `store.db`，数据目录由 `SIMPLE_POST_DATA_DIR` / `%APPDATA%/simple-post` 决定 |
| 脚本沙箱 | `rquickjs`（QuickJS-NG），关闭 `rust-alloc` | 只有它同时给墙钟打断 + 内存上限 + 栈上限 |
| 打包 | `tauri build` → NSIS；Docker 多阶段 → `debian-slim` + 一个二进制 | 运行阶段不装任何包，前提是 server crate 保持无 Tauri |

## 顶层目录地图

| 路径 | 角色 |
| --- | --- |
| `src/` | 前端。`features/`（13 个功能区）、`stores/`（Zustand 六个切片）、`lib/`（双传输分发、请求装配、各类规则）、`components/ui/`、`types/domain.ts` |
| `src-tauri/` | Tauri 壳：`commands/`（每个资源一个模块）、`lib.rs`（`generate_handler!`）、`main.rs`（CLI）、`tauri.conf.json`、`icons/`（由 `app-icon.svg` 生成） |
| `crates/simple-post-core/` | 领域层：`domain`、`db/`、`http_executor`、`script/`、`dynamic/`、`oauth2/`、`tls`、`url`、`spec`。无 Tauri、无 axum |
| `crates/simple-post-server/` | 无头宿主：`web_api/`（REST+SPA）、`daemon_supervisor`、`port_registry`、`auth`、`dto`、`http_config`，以及四个共享编排层 `project_api` / `interchange_api` / `spec_api` / `dynamic_api` |
| `crates/simple-post-interchange/` | 导入导出转换层：外来格式 ↔ IR ↔ `domain`。只有 server crate 依赖它 |
| `crates/simple-post-mcp/` | 48 个 MCP 工具、JSON-RPC 派发、daemon router、stdio 桥、`variable_fill` 解析器 |
| `fixtures/` | 10 份跨语言样本（见 08 篇），每份同时被 Rust 与 TS 的实现读取 |
| `scripts/` | 开发/测试/构建脚本：`dev-split.mjs`、`run-tests.mjs`、各集成测试、`e2e/preflight.mjs` |
| `docs/` | 仅 `screenshots/`（产品截图）+ 本目录 `notes/` |
| `.github/workflows/` | `ci.yml`、`release.yml`（桌面安装包）、`publish-image.yml`（GHCR 多架构） |
| `docker/` | 容器部署：`Dockerfile`（三阶段）、`docker-compose.yml`、`docker-swarm.yml`、`docker-portainer.yml`，以及旁置的 `Dockerfile.dockerignore` |
| `.worktrees/` | agent 运行时创建的**完整第二份 checkout**，gitignored；不要在里面当前仓库扫 |
| `.thinkrail/` | 本地 agent 工具状态（未进 `.gitignore`，只能靠自觉不扫） |
| `dist/`、`public/` | Vite 产物 / 静态资源（只有 `favicon.svg`） |
| 根配置 | `AGENTS.md`（150KB 硬性契约）、`README.md`（产品文档）、`vite.config.ts`、`eslint.config.js`、`.prettierrc`、`tsconfig.json`、`redocly.yaml` |

## 常用命令速查

| 命令 | 作用 | 需要先构建二进制？ |
| --- | --- | --- |
| `npm install` | 装前端依赖（Node `>=20 <23`，npm，lockfile v3） | 否 |
| `npm run dev` | Vite 开发服务器（5173）。只改前端够用 | 否 |
| `npm run dev:split` | Vite + `cargo run -- serve-web`（4173），`/api` 由 Vite 代理 | **是**，首次会冷编译 |
| `npm run build` | `tsc -b && vite build` → `dist/` | 否 |
| `npm run typecheck` / `lint` / `format:check` | 单项检查 | 否 |
| `npm test` / `test:watch` | 前端单测，自动发现 `src/**/*.test.ts` | 否 |
| `cargo test --workspace` | 全部 Rust 测试（`-p <crate>` 单跑） | 否（自己编译） |
| `npm run check` | 本地门禁七连：Prettier → ESLint → tsc → 单测 → OpenAPI lint → fmt → clippy | 否，但 `lint:openapi` 会 `cargo run` 一个 example，`clippy` 会编译全工作区 |
| `npm run tauri:dev` / `tauri:build` | 桌面壳开发 / 出安装包 | 是（Tauri 全量） |
| `npm run test:mcp` | MCP 端到端 | **是**，缺二进制退出码 2 |
| `npm run test:web:auth` / `test:web:basepath` | serve-web 鉴权 / 反代前缀集成测试 | **是**，需要 `target/debug/simple-post.exe` |
| `npm run perf:bench` | p50/p95/p99 延迟（`--assert` 卡 200ms p95） | 不需要二进制，但需要**已运行**的服务器 |
| `npm run mcp:repl` | 对运行中的 daemon 打 JSON-RPC | 需要运行中的 daemon |
| `npm run lint:openapi` | 用 interchange 的 example 导出 OpenAPI，再跑 Redocly | 否（会 `cargo run`） |
| `npm run e2e:preflight` | 解析 workspace 产物（二进制是否存在） | 检查既有产物 |
| `npm run icons` | 由 `src-tauri/app-icon.svg` 重生所有平台图标 | 否 |

## 第一次上手该跑什么

```bash
npm install
# 重要：先把数据目录挪走，见下方陷阱
export SIMPLE_POST_DATA_DIR=/tmp/sp-scratch   # Windows: set SIMPLE_POST_DATA_DIR=C:\temp\sp-scratch
npm run dev:split
# 浏览器打开 http://127.0.0.1:5173/
```

只想看界面就 `npm run dev`；要看真实后端（REST、MCP、导入导出落库）必须 `dev:split`，否则 `/api` 会 502。提交前跑一次 `npm run check`，CI 另外还会跑 `vite build`、`cargo test --workspace` 与容器 smoke test。

## 不变量

- **三种形态共用一份核心**：桌面、`serve-web`、MCP 都经由 `simple-post-core`；任何一侧单独实现的规则，另一侧会静默缺失（详见 02 的双传输不变量）。
- **MCP 的端点只有一个**：守护进程自己的监听端口。主端口（4173）不挂 `/mcp`、`/mcp/healthz`、`/mcp/tools`，所以给客户端配 `http://host:4173/mcp` 永远连不上。
- **版本号四处同步**：`package.json`、根 `Cargo.toml` 的 `[workspace.package]`、`src-tauri/tauri.conf.json`、`README.md` 徽章。`Cargo.lock` / `package-lock.json` 跟随。
- **数据只有一份**：默认数据目录就是用户在用的那份 `store.db`；`SCHEMA_VERSION` 不匹配时它会被**删除重建**（见 03 篇）。

## 陷阱

- **在 worktree 或旧分支里起服务会删掉用户的库。** `serve-web` 与 GUI 默认都用 `dirs::data_dir()`；`npm run dev:split` **不会**替你设 `SIMPLE_POST_DATA_DIR`。schema 版本不同的那份二进制会把真实 `store.db` 连 `-wal`/`-shm` 一起删掉。
- **`npm test` 只收 `src/**/*.test.ts`。** `scripts/` 下的集成测试与 `_verify_store.mjs` 不在 CI 里，删命令/改路由时它们不会红，需要手工扫一遍。
- **`npm run dev` 的 `/api` 代理只指向 4173。** `serve-web` 没起就是 502，不是 Vite 的 bug。
- **别用 `vite.config.ts` 里没有的 `/mcp` 代理**：MCP 端口在开发态由 OS 分配，代理无从知道。

## 相关笔记

- [02-architecture.md](02-architecture.md) — 依赖方向与五处协同编辑
- [09-engineering-process.md](09-engineering-process.md) — 门禁、CI、发布流程
- [10-glossary.md](10-glossary.md) — `daemon` / `surface` / `base URL cell`

## 文档与实现不一致

- `AGENTS.md §Important Files` 把 `vite.config.ts` 描述为「`/api` + `/mcp` proxy（ws disabled on `/mcp`）」，但实际的 `vite.config.ts:45-52` **只有 `/api`**，并有一段注释明确说明「这里**没有** `/mcp` 的代理，因为守护进程的端口在开发态由 OS 分配」。以源码为准。
- `AGENTS.md §Important Files` 称 MCP 工具注册表是「47-tool registry」，而 `README.md`、`crates/simple-post-mcp/README.md:122` 与实际 `REQUIRED_CAPABILITY` 表都是 **48**（`crates/simple-post-mcp/src/mcp/registry.rs:56` 起，48 条）。以 48 为准。
