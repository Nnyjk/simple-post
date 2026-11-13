# 09 工程流程

- 适用范围：本地开发、测试、CI、发布与容器部署的流程面；架构与数据模型见其它篇。
- 权威来源：`package.json`、`.github/workflows/{ci,release,publish-image}.yml`、`docker/`（`Dockerfile`、`docker-*.yml`、`Dockerfile.dockerignore`）、`scripts/`、`AGENTS.md`、`.github/CONTRIBUTING.md`。
- 最后核对：2026-11-13（v0.1.34）
- 前置阅读：01-overview.md

## 结论

1. 本地只有一条总门禁：`npm run check`，它是七步串行链，不是并行聚合。
2. 前端用 Node 自带测试器（`tsx --test` + `node:assert/strict`），没有 Vitest/Jest/RTL；文件名必须是 `src/**/*.test.ts` 才会被收集。
3. Rust 测试默认是「兄弟文件 + `#[path]`」，只有五个文件仍是 inline `mod tests`。
4. 仓库里有十余个**读源码文本**的哨兵测试；改注释、改字符串字面量都可能让它们变红，这通常是契约真的变了的信号，不要先放宽判据。
5. `scripts/` 下的辅助脚本既不在 `npm test` 的收集范围内，也不进 CI；删除路由/命令/MCP 工具后必须手工清扫。
6. CI 三道必需闸（web / rust / docker），其中 docker 按变更范围跳过、但跳过被报告为成功。
7. 容器运行阶段**不装任何包**，其前提是 `simple-post-server` 保持 Tauri-free。
8. 发布是「四份版本号 + annotated tag + draft release 手填 notes + amd64 镜像」；`main` 受保护，版本提交必须走 PR 且用 rebase merge。
9. 本地最危险的一次误操作是让 server/GUI 用到桌面 app 的 `store.db`；`SCHEMA_VERSION` 不匹配会**删掉**它。

## 结构

### 命令表（`package.json` 共 29 条 script）

| script | 作用 | 需先构建二进制 | 进 CI |
|---|---|---|---|
| `dev` | Vite 开发服务器（5173） | 否 | 否 |
| `dev:split` | Vite + `cargo run … serve-web`（4173）并行 | 否（cargo 自行编译） | 否 |
| `build` | `tsc -b && vite build` → `dist/` | 否 | 是（web job 最后一步） |
| `typecheck` | `tsc -b`，只做类型检查 | 否 | 是 |
| `lint` | `eslint .` | 否 | 是 |
| `lint:fix` | `eslint . --fix` | 否 | 否 |
| `format` | `prettier --write .` | 否 | 否 |
| `format:check` | `prettier --check .` | 否 | 是 |
| `lint:openapi` | `cargo run --example export_sample` → `redocly lint` | 否（cargo 现编） | 是 |
| `fmt` | `cargo fmt --all` | 否 | 否 |
| `fmt:check` | `cargo fmt --all --check` | 否 | 是（rust job 直跑 cargo） |
| `clippy` | `cargo clippy --workspace --all-targets -- -D warnings` | 否 | 是（rust job 直跑 cargo） |
| `check` | 七步链，见下 | 否 | 否（CI 拆成 job） |
| `build:tests` | `scripts/run-mcp-tests.cjs`：预检产物后跑 MCP 集成，缺产物 exit 2 | **是** | 否 |
| `e2e:preflight` | `scripts/e2e/preflight.mjs`：探测工作区二进制与 `--help` | **是** | 否 |
| `preview` | `vite preview` | 否 | 否 |
| `tauri` | `tauri` CLI 透传 | 否 | 否 |
| `icons` | 由 `src-tauri/app-icon.svg` 重生成各平台图标 | 否 | 否 |
| `mcp:repl` | 对运行中的 daemon 发 JSON-RPC 帧 | 否（需运行中的实例） | 否 |
| `test:mcp` | MCP 端到端 | **是** | 否 |
| `test:web:auth` | serve-web 鉴权两阶段（开放 + 加锁） | **是** | 否 |
| `test:web:basepath` | serve-web 反代前缀 `/sp` | **是** | 否 |
| `perf:bench` | p50/p95/p99 压测；`--assert` 卡 p95 < 200ms | 否（需运行中服务） | 否 |
| `tauri:dev` | 桌面壳开发 | 否 | 否 |
| `tauri:build` | 桌面安装包（NSIS 等） | 否 | 否（`release.yml` 调 tauri-action） |
| `tauri:build:debug` | 同上，debug profile | 否 | 否 |
| `tauri:check` | `cargo check --manifest-path src-tauri/Cargo.toml` | 否 | 否 |
| `test` | `scripts/run-tests.mjs` → `tsx --test` 全部前端用例 | 否 | 是 |
| `test:watch` | 同上，`--watch` | 否 | 否 |

`npm run check` 的串行链（任一失败即停）：

```
format:check → lint → typecheck → test → lint:openapi → fmt:check → clippy
```

CI 额外跑三样本地不在 `check` 里的东西：`vite build`、`cargo test --workspace`、容器运行冒烟。

### 测试体系

- **前端**：`scripts/run-tests.mjs` 递归遍历 `src/`，只收 `*.test.ts`（跳过 `node_modules`/`dist`/`target`/`build`），拼成一条 `tsx --test <files>` 命令。用例放在被测模块旁的 `__tests__/`；跨仓库哨兵放 `src/__tests__/`。`--watch` 的文件表在启动时固定，增删测试文件要重启。
- **Rust**：默认写兄弟文件，在模块里挂：

  ```rust
  #[cfg(test)]
  #[path = "projects_test.rs"]
  mod tests;
  ```

  仍用 inline `mod tests { … }` 的五个特例：`src-tauri/src/main.rs`、`crates/simple-post-server/src/{auth,main}.rs`、`crates/simple-post-server/src/web_api/static_files.rs`、`crates/simple-post-core/src/oauth2/http_client.rs`。
- **DB 测试**：每个测试文件自带 `fresh_pool()`，在 `std::env::temp_dir()` 下用进程 id + 时间戳（+ 计数器）建唯一目录，避免并行测试抢同一个 `store.db`。
- **集成测试前提**：`test:mcp` / `test:web:auth` / `test:web:basepath` / `build:tests` 都先经 `scripts/e2e/artifact-resolver.mjs` 找 `simple-post` 二进制（debug 或 release），找不到直接失败；`perf:bench` 则需要一个已经在跑的服务。
- **覆盖盲区**：`scripts/_verify_store.mjs`、`mcp-integration.test.mjs`、`mcp-repl.mjs`、`dump-openapi.mjs`、`e2e/*` 都不在 `npm test` 与 CI 内。删改路由、命令或 MCP 工具后要手工过一遍这些脚本。

### 源码级哨兵（读源码文本，不是行为）

| 文件 | 钉住的字符串约束 |
|---|---|
| `crates/simple-post-mcp/src/mcp/mod_test.rs` | ① 调了 `check_permission(` 的文件不得以 `.ok();` 收尾（权限拒绝被吞）；② `check_permission(` 的参数不得是字面量 `"global"`；③ `daemon_http.rs` 手写错误封包必须是 `"isError": true`。扫描整个 crate 的 `*.rs`，跳过 `*_test.rs` 与 `mod_test.rs`。 |
| `crates/simple-post-server/src/daemon_supervisor_test.rs` | `ensure_started_with(` 函数体内不得出现 `AppHandle`/`tauri::`/`Emitter`/`EmitterExt`；`ensure_started(` 签名必须仍收 `AppHandle` 且调 `tauri_finish`。 |
| `crates/simple-post-server/src/daemon_supervisor_reuse_test.rs` | `ReuseDecision::Adopt =>` 分支不得含 `teardown`（至少两处）；`Restart` 分支必须含 `teardown`；spawn 路径必须写 `write_with_fingerprint`。 |
| `crates/simple-post-server/src/project_api_shells_test.rs` | 四个薄壳必须调 `project_api::`；REST 两个还要调 `ensure_server_id(`；不得自己调 `projects::create_with_defaults(` / `nodes::create(` / `nodes::root_id_of(`；环境×模块格只能走 `set_module_base_url(`，不得直接 `module_base_urls.insert/remove`。 |
| `crates/simple-post-server/src/web_api/serve_test.rs` | `serve.rs` 不得含 `restart_without_app_handle`，且必须含 `start_without_app_handle`。 |
| `crates/simple-post-core/src/script/stdout_test.rs` | 脚本模块源码不得出现 `println!`/`print!`/`dbg!`（`eprintln!` 先剔除）；新增文件要手动加进 `SOURCES`。 |
| `src-tauri/src/commands/interchange_test.rs` | 命令参数名（camelCase 后）必须与 `dto.rs` 的 `*Opts` 字段一致，并与 `useTransfer.ts` 的调用点一致。 |
| `src-tauri/src/commands/spec_test.rs` | REST 侧必须走 `crate::spec_api::upsert`，MCP 侧必须走 `crate::spec::ops::upsert`；字段名须与实际线上名一致。 |
| `src-tauri/src/http_request_test.rs` | `lib.rs` 里 `http_request` 的命令参数必须与 `web_api/dto.rs` 的 body 字段一一对应；body 必须含 `endpoint_id`、`env`。 |
| `src/features/workspace/__tests__/tab-strip-animation.test.ts` | `WorkspaceTabs.tsx` 去注释后：依赖数组含 `isCompact`、无 `transition-[width`、无 `scrollIntoView`、有 `scrollTargetForActiveTab(`/`gsap.to(strip`/`scrollLeft: target`、`tween.progress(1)`、`previous.add(Flip.getState(entering))` 等十余条。 |
| `src/__tests__/release-image-ref.test.ts` | `release.yml`、`.github/CONTRIBUTING.md`、`AGENTS.md`、`README.md` 里的 `ghcr.io/<path>` 必须全小写，且不得出现 `ghcr.io/${{`；`release.yml` 必须有一条 `docker pull ghcr.io/…:`。 |

相邻但不同类：`src/lib/__tests__/dead-components.test.ts` 走真实 import 图，断言 `components/ui` 下没有零引用的原语。

**后果**：这些测试读的是文本。改一句注释里恰好包含被禁字符串、或者把 `ensure_started_with(` 写成别的重载名，都可能变红。先判断契约是否真的变了，不要在没读懂的情况下改判据。

### CI 三道闸（`.github/workflows/ci.yml`）

触发：push 到 `main`、所有 PR、手动。`concurrency: ci-${{ github.ref }}`，`cancel-in-progress: true`。

- `changes`（`Change scope`）：`dorny/paths-filter` 计算 `docker` 输出，覆盖 `docker/**`、Cargo/package 清单、`crates/**`、`src-tauri/**`、`src/**`、`scripts/**`、构建配置等。
- `web`（必需检查，名 `Web — format, lint, typecheck, test, build`，ubuntu，Node 22）：`npm ci --include=dev` → `format:check` → `lint` → `typecheck` → `npm test` → 装 Rust 工具链 + `lint:openapi` → `build`。
- `rust`（必需检查，名 `Rust — fmt, clippy, test`，windows-latest）：`cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace`。
- `docker`（必需检查，名 `Docker — image build + container smoke test`，75 分钟）：`needs: changes`，`if: ${{ !cancelled() && needs.changes.outputs.docker != 'false' }}`（fail-closed）。步骤：Buildx → 构建 `linux/amd64` 并 `load` 为 `simple-post:local` → 起容器等 HEALTHCHECK → 断言 `/healthz` 响应体、SPA 外壳与哈希资源、未鉴权 `/api/projects` = 401 → REST 建项目 → `id -u == 10001` 且无 libgtk/libwebkit → 用同一命名卷重建容器 → 项目仍在 → 清理。

三个 display name 就是 `main` 上配置的必需状态检查。**改 job 名 = 改仓库设置**，不是纯 YAML 编辑。

- `release.yml`：`v*` tag 或手动触发；三平台矩阵（macOS universal dmg/app、Ubuntu deb/appimage、Windows NSIS）；`releaseDraft: true`；`cancel-in-progress: false`。
- `publish-image.yml`：`v*` tag；`if: startsWith(github.ref, 'refs/tags/v')`；构建并推送 `linux/amd64`；四个标签由 metadata-action 生成（`x.y.z`、`x.y`、`vX.Y.Z`、`latest`，预发布不带 `-` 才给 `latest`）；`provenance: false`；`cancel-in-progress: false`。

两个发布工作流都用 `cancel-in-progress: false`：取消一次进行到一半的发布会留下半成品，比重复一次更糟；第二次触发排队而不是并行。

### 发布流程

1. 四份版本号同改：`package.json`、根 `Cargo.toml` 的 `[workspace.package]`、`src-tauri/tauri.conf.json`、`README.md` 的版本徽章；`Cargo.lock` 与 `package-lock.json` 随之更新。
2. `main` 受保护，版本提交走 `chore/release-<version>` 分支 + PR，用 **rebase merge**（squash 会用 PR 标题覆盖提交正文）。
3. 打 **annotated** tag：`git tag -a vX.Y.Z <commit> -m "Simple Post vX.Y.Z"`，再推 tag。
4. 两个工作流跑完后，draft release 的「这一版改动」是手写的；补完后 `gh release edit <tag> --notes-file <file> --draft=false`。
5. 镜像路径一律小写（`ghcr.io/nnyjk/simple-post`），发布说明里的 `docker pull` 必须能照抄执行。

版本提示与自动更新的链路（谁读 `latest.json`、谁读 release 正文、「立即执行更新」的两道门）见 [12-update-channel.md](12-update-channel.md)。

### 部署

- `docker/Dockerfile` 三阶段：`node:22-bookworm-slim` 构建 SPA → `rust:bookworm` 编 `-p simple-post-server` → `debian:bookworm-slim` 运行。运行阶段**不装任何包**，探针用 bash `/dev/tcp` 手写；它以 uid 10001 运行，`SIMPLE_POST_DATA_DIR=/data`，入口 `serve-web --bind 0.0.0.0 --port 4173`。前提是 server crate 不链接 Tauri/GTK/WebKit；CI 会断言镜像里没有 `libgtk*`/`libwebkit*`。
- `docker/docker-compose.yml`：单机 Compose/`docker stack deploy` 两用；无 `build:`、无顶层 `version:`、无 `container_name` 之外的花活；`SP_IMAGE` 可指向发布镜像。
- `docker/docker-swarm.yml`：`replicas: 1`、`endpoint_mode: vip`、`update_config.order: start-first`、**开启** `placement.constraints: node.labels.simple_post_data == 'true'`。
- `docker/docker-portainer.yml`：`version: "3.8"`、`endpoint_mode: dnsrr`、端口 `mode: host`、`update_config.order: stop-first`、默认**不写** placement（多节点需手工取消注释）。

`replicas: 1` 解决的是「两个写者抢同一个 WAL 文件」，**不是**放置问题；placement 约束解决的是「任务落到没有那个 node-local 卷的节点、Docker 建空卷、服务健康但数据像消失」。两者不能互相替代。

MCP 专用端口必须是**独立的容器端口**（同一容器端口只能承载一个发布端口，第二个映射会被静默丢弃）；用 `mode: host` 发布时更新必须 `stop-first`，否则新任务抢不到宿主端口。

### 本仓库的 ignore 纪律（扫描时照此排除）

| 路径 | 归属 |
|---|---|
| `target/` | Rust workspace 构建产物（根目录） |
| `src-tauri/target/` | 历史构建产物，两处都保留规则 |
| `src-tauri/gen/` | tauri-build 生成物 |
| `dist/` | Vite 输出 |
| `node_modules/` | npm 依赖 |
| `.worktrees/` | 桌面 runtime 建的完整第二 checkout |
| `.scratch/`、`scripts/.scratch/`、`.playwright-mcp/` | 本地临时/调试 |
| `coverage/`、`.cache/`、`.vite/`、`.turbo/` 等 | 覆盖率与工具缓存 |
| `*.tsbuildinfo` | `tsc -b` 增量信息 |
| `.minimax/` | 本地 agent 运行状态 |
| `build-exit.txt`、`build-output.txt`、`*.tmp.ts`、`*.tmp.cjs`、`*.bak`、`*.removed` | 临时构建/备份 |

`docker/Dockerfile.dockerignore`（与 Dockerfile 同目录的旁置 ignore）另把 `.git/`、`.github/`、`docs/`、`*.md`、tooling 配置、`.env*` 排除出构建上下文。

## 不变量

- `npm run check` 必须本地全绿；CI 的 web/rust 两闸跑的就是同一批检查。
- 前端测试文件只能叫 `*.test.ts` 且位于 `src/` 下，否则 `npm test` 不会收。
- Rust 新测试优先用兄弟文件 + `#[path]`，与本仓库主流一致。
- 三个必需检查的 display name 不得随意改。
- `docker` job 的 `if` 必须保留 `!cancelled()`（fail-closed）；`changes` job 不可删。
- 运行阶段不得引入 `curl`/`libssl`/`libgtk`/`libwebkit`；加了就等于撤销 Tauri 拆分。
- 发布必须四份版本号同步、annotated tag、draft 手填 notes；镜像路径全小写。
- 工作树工具必须在 `.gitignore`、`.prettierignore`、`eslint.config.js` 三处都忽略 `.worktrees/`。
- 全仓 LF，仅 `*.bat`/`*.cmd` 保持 CRLF；编辑器与 git 属性双层约束。
- Node `>=20 <23`，只用 npm；不得新增 pnpm/yarn/bun 锁文件。

## 陷阱

- **数据目录**：`serve-web` 与 GUI 默认都取 `dirs::data_dir()`（Windows 即 `%APPDATA%`），`store.db` 直接落在那层、不带 `simple-post` 后缀；`mcp.port` 则在同层的 `simple-post/` 子目录。三种形态传给 `db::init` 的目录并不相同，`serve-web --data-dir` 也不会传给 daemon 子进程——落点与差异见 `03-data-model.md` 的「数据目录与落点」。库的 `user_version` **低于**本构建的 `SCHEMA_VERSION` 时 `ensure_current_schema` 会**删除** `store.db`（连 `-wal`/`-shm`），不可恢复；反过来旧构建打开新库只报校验错误、不动文件。跑 worktree 或任何验证前设 `SIMPLE_POST_DATA_DIR=<scratch>`（不要只给 `--data-dir`）。
- `npm run dev:split` **不设** `SIMPLE_POST_DATA_DIR`，必须自己先在环境里设好再启动。
- `.worktrees/<session>/` 是独立 checkout；漏掉任一处 ignore，lint/格式化会爬进去并把它的文件当本树的文件报错。
- **写 UTF-8 要分 PowerShell 版本**：PowerShell 6+ 用 `-Encoding utf8NoBOM`；Windows PowerShell 5.1 没有这个值（照抄会直接报参数错），用 `[System.IO.File]::WriteAllText($p, $t, (New-Object System.Text.UTF8Encoding($false)))`。5.1 的默认编码也不统一：`>` 重定向是 UTF-16LE，`Set-Content` 默认 ANSI，两者都会写坏 UTF-8。
- `npm ci` 在 CI 里必须 `--include=dev`：Actions 默认 `NODE_ENV=production`，否则 `tauri`/`eslint`/`tsx` 都不在。
- 容器里 MCP 端口要同时改三处：应用设置里的端口、compose/swarm 里的 `target`、应用监听地址保持 `0.0.0.0`；改完要重启 daemon。
- 不设 `SIMPLE_POST_WEB_PASSWORD` 等于把 `/api/*`（含任意 URL 出站）与 `/mcp` 暴露在 `0.0.0.0`。但也不能写成空串——`auth.rs` 判的是「变量是否设置」，空串会变成要求空 bearer。
- `SIMPLE_POST_PUBLIC_BASE_PATH` 留空串会触发「必须以 `/` 开头」校验失败；不用就整个键注释掉。
- schema 变更：判据是**兼容性**，不是可空性。只要是 `ALTER TABLE … ADD COLUMN` 能表达、且旧行读回来语义等价的列，就写进 `ADDED_COLUMNS`、不 bump——`TEXT NOT NULL DEFAULT '{}'`（`schema.rs:151`）就是现成的例子。只有需要改写既有行、或旧二进制读不了新形状的改动才 bump，而 bump 会删掉所有用户的 store。
- 探针与冒烟测试必须断言响应体：无路由路径会落到 SPA fallback 返回 `200 text/html`，只看状态码会在错误的路由上通过。

## 相关笔记

- [README.md](README.md)：这组笔记的索引与建议阅读顺序。
- [01-overview.md](01-overview.md)：项目定位与三形态（桌面 / serve-web / MCP）。
- [02-architecture.md](02-architecture.md)：Tauri split 与 workspace 分层。
- [03-data-model.md](03-data-model.md)：SQLite schema、`store.db` 与核心数据模型。
- [04-request-pipeline.md](04-request-pipeline.md)：请求构建/发送链路，以及 Web 形态与鉴权。
- [05-mcp.md](05-mcp.md)：MCP 工具注册、能力闸与项目权限闸。
- [06-dynamic-and-scripts.md](06-dynamic-and-scripts.md)：动态变量与请求脚本沙箱（含 stdout 纪律）。
- [07-frontend.md](07-frontend.md)：前端状态、工作区与标签条行为。
- [08-interchange-and-parity.md](08-interchange-and-parity.md)：导入导出与跨语言 parity fixtures。
- [10-glossary.md](10-glossary.md)：术语表。
- `AGENTS.md`：运行时契约与逐条出处；`.github/CONTRIBUTING.md`：给人看的 PR/发版清单。

## 文档与实现不一致

1. **`docker/docker-portainer.yml:70` 把镜像固定成 `ghcr.io/nnyjk/simple-post:0.1.34`**，但发布流程只列了四份版本号（`package.json`、根 `Cargo.toml`、`tauri.conf.json`、README 徽章）。这一行不在清单里，升级时容易漏改，导致 Portainer 栈仍拉旧镜像。
2. `docker/docker-compose.yml:26` 的注释示例同样写着 `:0.1.34`（仅注释，不影响运行）；`README.md` 另有多处 `0.1.34`（拉取命令、`SP_IMAGE` 示例、标签说明、v0.3 描述），只有徽章被列为「版本源」，其余是叙述性副本，会随版本漂移。
3. `.github/workflows/release.yml` 的 `tagName` 传的是 `format('refs/tags/{0}', github.ref_name)`，即 `refs/tags/vX.Y.Z` 而非裸 tag 名；tauri-action 是否原样接受该前缀 `[未核实]`，但发布说明与 `publish-image.yml` 都按裸 `vX.Y.Z` 叙述。
