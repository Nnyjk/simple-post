# Simple Post — 形态扩展方案（2026-09-18）

> 在已有的 Tauri 桌面形态基础上补全「Web server」形态，使浏览器
> 不再启动本地 MCP 子进程，而是把工具调用委托给远程 server。同时为
> MCP 进程启动引入持久化开关（默认关闭）。

## 1. 形态矩阵（决策结果）

| 形态 | 启动入口 | 渲染端 → 后端 | MCP daemon |
|---|---|---|---|
| **Tauri 桌面**（Win/Linux/macOS） | `simple-post`（无子命令） | `invoke()` → `#[tauri::command]` | 尊重 `mcp.auto_start` 偏好；默认不启动 |
| **Web server**（浏览器跨设备访问） | `simple-post serve-web --bind=0.0.0.0 --port=4173 --data-dir=...` | `fetch('/api/<verb>')` | 尊重 `mcp.auto_start` 偏好；默认不启动 |
| **Dev 浏览器**（Vite） | `npm run dev` | `invoke()` 失败 → 回退 fetch；无 server 端 | 不可达（每 3s 报一次 warn，预期） |
| **MCP daemon headless** | `simple-post mcp --transport=http --port=0` | n/a | 这就是 daemon 本身（`--transport` 默认即 `http`） |

三种"用户能看到的"形态（Tauri / Web / Dev）共享同一份 SQLite、同一份偏好（`db/preferences.rs`）、同一份 MCP 配置。

## 2. 决策记录

| 决策 | 选项 | 取舍 |
|---|---|---|
| Web server 启动入口 | **`simple-post serve-web` 子命令**（vs 独立 binary / 复用 mcp） | 一个 binary 涵盖 desktop + mcp + web 三形态；CI 只产一个 .exe/.deb/.dmg。复用 mcp 子命令会污染语义（"mcp" 不再纯粹指 daemon）。 |
| 浏览器 ↔ server 协议 | **REST**（vs JSON-RPC / Hybrid） | 浏览器 surface 本质是 CRUD + execute，REST 命名直接对应 UI 操作；curl 调试友好；与 MCP 客户端的 JSON-RPC 隔离。MCP 给 AI 客户端的 `/mcp` 路由保持不变。 |
| MCP 启动开关 | **持久化偏好，默认关闭**（vs 仅 CLI 标志 / 分形态开关） | 桌面用户多由管理员按需启动；一个键跨三形态共享；Tauri 形态 `.setup()` 也尊重该键，避免"桌面形态每次都自启"的隐式行为。 |

## 3. 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                     用户的浏览器（任意设备）                       │
│  ┌───────────────────────┐                                       │
│  │  React SPA            │  ← 同一份 dist/ 产物                 │
│  │  (Vite build output)  │                                       │
│  └─────────┬─────────────┘                                       │
│            │                                                     │
│   形态判断：detect-runtime.ts                                     │
│   ┌────────┴────────┬───────────────────────┐                     │
│   │ Tauri shell     │ 裸浏览器（web / dev）  │                     │
│   │ invoke()        │ fetch('/api/...')      │                     │
│   └────────┬────────┴──────────┬────────────┘                     │
└────────────┼──────────────────┼──────────────────────────────────┘
             │                  │
    Tauri shell invoke     HTTP (same-origin if window, cross-origin if LAN)
             │                  │
┌────────────┴──────────────────┴──────────────────────────────────┐
│                     Rust 后端（simple-post.exe）                 │
│                                                                   │
│  ┌────────────────────────┐  ┌────────────────────────────────┐  │
│  │ Tauri .setup()         │  │ `serve-web` 子命令              │  │
│  │ manage(McpDaemonHandle)│  │ axum::Router::new()            │  │
│  │ manage(DbPool)         │  │   .route("/api/*", ...)        │  │
│  │ invoke_handler!        │  │   .route("/", get(spa_static)) │  │
│  │                        │  │   .route("/mcp", post(...))    │  │
│  │ 启动时读 mcp.auto_start│  │   .with_state(state)           │  │
│  │ = false → 不自启       │  │                                │  │
│  └────────────────────────┘  └────────────────┬───────────────┘  │
│                                                │                  │
│           ┌────────────────────────────────────┴─────────┐       │
│           │ 共享层（无形态依赖）                            │       │
│           │   • db::*（CRUD、preferences）                  │       │
│           │   • http_executor::*（真实 HTTP 发送）          │       │
│           │   • mcp::*（44+ 工具，可被 /mcp 和 /api/* 共用） │       │
│           │   • daemon_supervisor::McpDaemonHandle          │       │
│           │     (两形态都尊重 mcp.auto_start)                │       │
│           └────────────────────────────────────────────────┘       │
│                                                                   │
│  数据：~/.local/share/simple-post/simple-post.db (rusqlite + WAL)  │
└───────────────────────────────────────────────────────────────────┘
```

## 4. 形态边界（哪些能力在哪）

| 能力 | Tauri | Web | Dev |
|---|---|---|---|
| 项目/模块/集合/接口 CRUD | Tauri commands | REST `/api/projects` 等 | fetch 直连 daemon (失败) |
| 发送 HTTP 请求（绕过 CORS） | `http_request` command | REST `/api/http/send` | 浏览器 fetch（受 CORS 限制） |
| MCP 进程管理 | Tauri commands + supervisor | REST `/api/mcp/status` `/restart` | 不可用 |
| MCP 客户端连接（Claude Desktop 等） | stdio proxy (`simple-post-mcp-proxy`) | 直连 HTTP `http://server:port/mcp` | 不可用 |
| 偏好持久化 | Tauri command | REST `/api/preferences` | 不可用 |
| 静态文件服务 | n/a（Tauri 内嵌） | axum serve dist/ | Vite dev server |
| 自动更新 / 文件系统访问 | Tauri APIs | 受限（仅 server data dir） | 受限 |

## 5. MCP 启动配置（默认关闭）

### 5.1 配置键

新增持久化偏好：`mcp.auto_start: bool`（默认 `false`）。

存储位置：`db/preferences.rs`，复用现有的 `preferences` 表 key/value schema。

### 5.2 三形态下的生效逻辑

| 形态 | 启动时检查点 | 关闭后行为 |
|---|---|---|
| Tauri | `lib.rs::run()` 的 `.setup()` 块：`auto_start == false` 时**不调用** `ensure_started` | `mcp_status` 持续返回 `stopped`；ConnectionSection 显示「MCP 已关闭」+ 一个 Toggle 引导开启 |
| Web | `serve_web()` 的 spawn 块：同上 | `mcp_status` 持续返回 `stopped`；REST `/api/mcp/start` `/api/mcp/stop` 控制 |
| Dev | 浏览器无法启动 daemon（无 Tauri runtime） | N/A；ConnectionSection 显示「Web/Tauri 形态下才可用」 |

### 5.3 设置面板 Toggle

`ConnectionSection` 增加：
- 「自动启动 MCP」开关（默认关）
- 关闭后保留「立即启动一次」按钮（手起）
- Toggle 修改走 Tauri command `preferences_set`，server 形态走 `PUT /api/preferences`

### 5.4 CLI 旁路

为 ops / CI 场景保留 CLI 旁路： `--mcp-auto-start` / `--no-mcp-auto-start` 启动标志覆盖偏好。

## 6. Web server 形态的 REST API（最小可用集）

第一阶段先打通以下路由，其余按 MCP tool 一一映射补全：

| Method | Path | 后端 | 说明 |
|---|---|---|---|
| GET    | `/api/healthz` | 静态 | `{status:"ok",version,shape:"server"}` |
| GET    | `/api/projects` | `db::projects::list_all` | 项目列表 |
| GET    | `/api/projects/{id}` | `db::projects::get` | 单项目 |
| POST   | `/api/projects` | `db::projects::create` | 创建项目 |
| PATCH  | `/api/projects/{id}` | `db::projects::update` | 更新项目 |
| DELETE | `/api/projects/{id}` | `db::projects::delete` | 删除项目 |
| POST   | `/api/http/send` | `http_executor::execute` | 真实 HTTP 发送（绕过 CORS） |
| GET    | `/api/mcp/status` | `McpDaemonHandle::snapshot()` | daemon 快照 |
| POST   | `/api/mcp/start` | `McpDaemonHandle::ensure_started` | 启动 daemon |
| POST   | `/api/mcp/stop` | `McpDaemonHandle::shutdown` | 停止 daemon |
| GET    | `/api/preferences` | `db::preferences::get_all` | 全量偏好 |
| PUT    | `/api/preferences` | `db::preferences::set` | 单键覆盖 |
| GET    | `/mcp` | `daemon_http` | AI 客户端 JSON-RPC 入口（保留） |
| GET    | `/mcp/healthz` | `daemon_http` | AI 客户端健康检查 |
| GET    | `/mcp/tools` | `daemon_http` | AI 客户端 tools/list |
| GET    | `/` | 静态 SPA | `dist/index.html` 兜底 |
| GET    | `/assets/*` | 静态 SPA | `dist/assets/*` 兜底 |

MCP HTTP 入口 `/mcp` 完全保留原状，AI 客户端的配置不受形态切换影响。

## 7. 前端 transport 抽象

`src/lib/tauri-commands.ts` 增加形态判断：

```ts
const isTauriShell = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const serverBase   = (typeof window !== 'undefined' && (window as any).__SIMPLE_POST_SERVER__?.base)
  ?? '/';

export async function invokeCmd<T>(cmd: TauriCommand, args?: Record<string, unknown>): Promise<T> {
  if (isTauriShell) {
    return invoke<T>(cmd, args ?? {});
  }
  // web / dev：先尝试 fetch server，daemon 不可达时降级
  const url = `${serverBase}api/${commandToRoute(cmd)}`;
  const res = await fetch(url, {
    method: commandToMethod(cmd),
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args ?? {}),
  });
  if (!res.ok) {
    throw new Error(`server ${res.status}: ${await res.text()}`);
  }
  return res.json() as Promise<T>;
}
```

`commandToRoute` / `commandToMethod` 是查表函数：把 `COMMANDS.projectsList = 'projects_list'` 映射成 `GET /api/projects`，把 `COMMANDS.projectsCreate = 'projects_create'` 映射成 `POST /api/projects`。

`http_request` 走 `POST /api/http/send`（不依赖 MCP daemon，与 /mcp 路由隔离）。

## 8. 落地步骤

按依赖顺序逐个 worker 串行推进（共享文件冲突控制），主会话负责设计 + 端到端验证：

1. **本 doc 落地**：先把方案固化到 `docs/distribution-modes-2026-09-18.md`（已完成）
2. **MCP 启动配置**：
   - `db/preferences.rs` 加 `mcp.auto_start` 默认值
   - `commands.rs` 加 `preferences_set_mcp_autostart` 命令
   - `daemon_supervisor.rs` / `lib.rs::run()` 改 `.setup()` 读偏好
   - `serve-web` 启动时也读同一偏好
   - `ConnectionSection.tsx` 加 Switch + 文案
3. **Web server 入口**：
   - `main.rs` 加 `Subcommand::ServeWeb { port, bind, data_dir }`
   - `lib.rs::run_serve_web()` 绑 axum；装配 router；启动 daemon supervisor（如 auto_start）
   - 静态文件服务（`dist/`）
4. **REST API 层**：
   - 新文件 `src-tauri/src/web_api.rs`：按 §6 的路由表实现
   - 共用 `db::*` / `http_executor::*` / `daemon_supervisor::*` / `mcp::*` 内部函数
5. **前端 transport 抽象**：
   - `tauri-commands.ts` 加 `isTauriShell` 判断 + `invokeCmd` 双形态分发
   - `src/lib/http.ts` `sendRequest` 在 web 形态下走 `/api/http/send`
6. **构建产物**：
   - `npm run build` 产 dist/（现状已 OK）
   - Tauri `bundle.targets` 仍是 `["nsis"]`（仅 Windows）；macOS/Linux deb/dmg 已在 CI 配置（commit `286082a`）
   - 启动 server 时用同一个 `dist/` 目录
7. **验证**：
   - dev 形态：浏览器跑 `npm run dev`（现状 OK；bootstrap 失败的 warn 是预期的）
   - Tauri 形态：`npm run tauri:dev`，验证 MCP 不自启、Toggle 正常
   - Web 形态：`npm run build` + `simple-post serve-web --bind=127.0.0.1 --port=4173`；浏览器访问 `http://127.0.0.1:4173/`，验证 CRUD + `/api/http/send` + `/api/mcp/status`

## 9. 兼容性 / 风险

- **既有 MCP 客户端配置不变**：AI 客户端（Claude Desktop、Cursor）的 `mcpServers.simple-post.command = simple-post` + `args = [mcp]` 完全不受影响——daemon 子命令照旧。
- **back-compat 偏好**：现有 `preferences` 表 row 升级不破坏——只新增 row。
- **dev 模式 `mcp_status` 反复 warn**：是预期（无 daemon）；不在本次修复范围（dev mode 本来就是测试形态）。
- **Web 形态安全边界**：默认 `bind=127.0.0.1`；`bind=0.0.0.0` 时应在 README 醒目提示"暴露在 LAN 上任何人可访问"，由用户显式承担风险。
- **CORS**：浏览器在 web 形态下访问 `http://127.0.0.1:4173` 时是同源（页面与 /api 同 host/port），不需要 CORS；跨设备访问（`http://server.lan:4173`）是同源；只有 dev Vite（`localhost:5173` → `127.0.0.1:4173`）需要 CORS。v1 默认不开跨域，本地开发靠 vite proxy。

## 10. 不在本次范围内（defer）

- **完整 REST API 覆盖 44+ MCP tool**：先把 §6 的最小可用集打通，剩下按工具一一映射，列入 v0.2 backlog
- **认证 / token / 多租户**：server 形态 v1 默认假设「单机本地 server」，认证列入 v0.3
- **TLS / HTTPS**：v1 假设反向代理（caddy / nginx）处理，server 形态不内置 TLS
- **dev 形态的 mock data 恢复**：当前 dev 形态数据为空是预期（v0.1 设计如此），不是本次修复