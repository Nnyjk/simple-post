# Simple Post — 桌面端 API 工具产品规划

> 一款面向 Win10/Win11 的现代化桌面 API 调试 / 管理工具，核心差异化：**自带 MCP Server**，让 AI（Claude Desktop、Cursor、Cline 等）能直接读写接口、生成文档、参与调试。

---

## 1. 产品定位

| 维度 | 说明 |
|---|---|
| 目标用户 | 后端 / 全栈 / 测试工程师，特别是需要 AI 协作的个人或小团队 |
| 竞品参照 | Postman、Bruno、Hoppscotch、Insomnia |
| 差异化 | 内置 MCP Server、原生 Win10/11、现代化 UI、离线优先 |
| 形态 | 单机桌面 App（不强制联网），所有数据本地存储 |

---

## 2. 技术选型

> **已确定：Tauri 2 + React + TypeScript**（用户决策 2026-09-11）

| 组件 | 选型 | 理由 |
|---|---|---|
| 运行时 | **Tauri 2.x** | 包体小（~10MB）、启动快、Win10/11 打包稳；Rust 后端性能强 |
| UI 框架 | **React 18 + TypeScript** | 生态最大，AI 协作方便 |
| 构建工具 | **Vite 5** | 官方推荐、HMR 极快 |
| UI 组件 | **shadcn/ui 风格 + TailwindCSS 3** | 现代化、可定制、不锁死版本 |
| 状态管理 | **Zustand** | 轻量、TS 友好 |
| 数据存储 | **SQLite（Rust：`rusqlite` + `r2d2`）** | 本地、零配置、事务强；UI 先行阶段先接 `tauri-plugin-sql` |
| HTTP 引擎 | **Rust：`reqwest`** + 渲染端 `fetch` 走 Tauri command | 主进程统一执行 |
| MCP | **`@modelcontextprotocol/sdk`（Node 子进程）** | 官方 SDK，stdio 模式 |
| 文档/Markdown | **CodeMirror 6 + react-markdown** | 接口 body / 响应 / 文档编辑 |
| 打包 | **Tauri bundler**（NSIS + MSI） | Win10/11 原生安装包 |

---

## 3. 架构设计

```
┌──────────────────────────────────────────────────────────────┐
│  Renderer (React + TS, sandbox)                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │ Project  │ │ HTTP     │ │ Docs /   │ │ Settings /       │ │
│  │ Tree     │ │ Tester   │ │ AI Panel │ │ Env / Var Mgr    │ │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────────┬─────────┘ │
│       │            │            │                │           │
│       └────────────┴─────┬──────┴────────────────┘           │
│                    Zustand Store + TanStack Query             │
└────────────────────────┬─────────────────────────────────────┘
                         │  @tauri-apps/api  invoke()
┌────────────────────────┴─────────────────────────────────────┐
│  Rust Core (Tauri 主进程)                                    │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐  │
│  │ Tauri Cmd    │ │ HTTP Engine  │ │ SQLite Repository   │  │
│  │ (typed)      │ │ (reqwest)    │ │ (rusqlite + r2d2)    │  │
│  └──────┬───────┘ └──────┬───────┘ └──────────┬───────────┘  │
│         └────────────────┼─────────────────────┘             │
│                          │                                   │
│                  ┌───────┴────────┐                          │
│                  │ MCP Bridge     │  ← 启动 Node 子进程      │
│                  │ (stdin/stdout) │                          │
│                  └────────┬───────┘                          │
│                           │ stdio                           │
│                  ┌────────┴───────┐                          │
│                  │ MCP Server     │  ← Node 子进程           │
│                  │ (@mcp/sdk)     │                          │
│                  └────────────────┘                          │
└──────────────────────────────────────────────────────────────┘
                          │
                  ┌───────┴────────┐
                  │ %APPDATA%      │  ← 本地数据目录
                  │  \simple-post\ │
                  │  ├─ db.sqlite  │
                  │  ├─ logs/      │
                  │  └─ mcp.json   │
                  └────────────────┘
```

### 3.1 进程边界

- **Renderer**：只跑 UI，所有 `fs` / `net` / `db` 走 Tauri command。
- **Rust Core**：唯一持有 SQLite / HTTP / 文件系统访问权限。
- **MCP Bridge**：Rust 侧通过 `tokio::process::Command` 拉起 Node 子进程，转发 JSON-RPC。
- **MCP Server**：独立 Node 进程，stdio 模式，复用同一 SQLite（独立 connection）。

### 3.2 关键 Tauri Commands（草案）

```rust
#[tauri::command]
async fn list_projects() -> Result<Vec<Project>, Error>;

#[tauri::command]
async fn run_endpoint(id: String, overrides: Option<RunOverrides>) -> Result<Response, Error>;

#[tauri::command]
async fn mcp_status() -> Result<McpStatus, Error>;

#[tauri::command]
async fn mcp_config() -> Result<McpConfig, Error>;
```

---

## 4. 数据模型

### 4.1 层级关系

```
Project (项目)
  └─ Environment[]  (dev / staging / prod 等)
  └─ Module[]       (一级业务域，例如 "用户中心"、"订单")
        └─ Collection[]  (接口分组，例如 "登录注册"、"CRUD")
              └─ Endpoint[]  (单个接口)
                    ├─ Request:  method, url, params, headers, body
                    ├─ Response: status, headers, body, time, size
                    └─ Meta:     tags, docs, tests, examples
```

> 之所以"Module → Collection"二级，而不是"Project → Collection → Endpoint"三级，是因为实际业务里一个项目通常会按业务域先切一刀，UI 树更清晰，也方便做权限/标签。

### 4.2 核心表结构（SQLite）

```sql
CREATE TABLE projects (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  description   TEXT,
  base_url      TEXT,
  color         TEXT,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE modules (
  id            TEXT PRIMARY KEY,
  project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  description   TEXT,
  sort_order    INTEGER DEFAULT 0
);

CREATE TABLE collections (
  id            TEXT PRIMARY KEY,
  module_id     TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  description   TEXT,
  sort_order    INTEGER DEFAULT 0
);

CREATE TABLE endpoints (
  id            TEXT PRIMARY KEY,
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  method        TEXT NOT NULL CHECK(method IN ('GET','POST','PUT','PATCH','DELETE','HEAD','OPTIONS')),
  url           TEXT NOT NULL,
  params        TEXT,        -- JSON: [{key,value,enabled,description}]
  headers       TEXT,        -- JSON: [{key,value,enabled,description}]
  body          TEXT,        -- JSON: {mode:'none'|'json'|'form'|'raw', content, schema}
  auth          TEXT,        -- JSON: {type:'none'|'bearer'|'basic'|'apikey', ...}
  docs          TEXT,        -- Markdown
  tags          TEXT,        -- JSON array
  sort_order    INTEGER DEFAULT 0,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE environments (
  id            TEXT PRIMARY KEY,
  project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,         -- e.g. "dev"
  variables     TEXT NOT NULL,         -- JSON: {key:value}
  is_active     INTEGER DEFAULT 0
);

CREATE TABLE response_history (
  id            TEXT PRIMARY KEY,
  endpoint_id   TEXT NOT NULL REFERENCES endpoints(id) ON DELETE CASCADE,
  status        INTEGER,
  duration_ms   INTEGER,
  size_bytes    INTEGER,
  headers       TEXT,
  body          TEXT,
  error         TEXT,
  created_at    INTEGER NOT NULL
);

CREATE TABLE mcp_logs (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  direction     TEXT,         -- 'in' / 'out'
  payload       TEXT,
  created_at    INTEGER NOT NULL
);
```

---

## 5. 功能规划

### 5.1 MVP（v0.1，~4 周）

| 模块 | 功能 |
|---|---|
| 基础 | 项目/模块/集合/接口的 CRUD、拖拽排序、搜索 |
| HTTP 调试 | method / url / params / headers / body（json/form/raw） |
| 响应 | 状态码、耗时、大小、Headers、Body（JSON 高亮 / 预览 / 原始） |
| 环境变量 | `{{var}}` 模板替换、项目级多环境切换 |
| 存储 | SQLite 本地持久化 |
| MCP 基础 | stdio 模式启动，暴露 `api_list` / `api_get` / `api_run` 三个工具 |
| 打包 | NSIS 安装包 + 便携 zip |

### 5.2 v0.2（+2 周）

- 鉴权（Bearer / Basic / API Key / OAuth2 简化版）
- 响应历史 & 收藏
- 导入 Postman v2.1 / OpenAPI 3.0
- 导出 OpenAPI 3.0
- MCP 工具扩展：`api_create` / `api_update` / `api_search`
- MCP 进程可视化（设置页显示 PID、状态、tail 日志）

### 5.3 v0.3（+2 周）

- AI 文档生成（端点 → Markdown 文档）
- AI 测试用例生成（schema → 测试）
- 请求脚本（pre-request / test 脚本，类 Postman，但沙箱隔离）
- 集合批量运行 + 简易报告
- 团队协作（Git 同步 JSON 格式的工作区，可选）

### 5.4 v1.0

- WebSocket / SSE / GraphQL
- Mock Server
- 性能压测（简易）
- 插件系统（允许第三方 MCP 工具注册）

---

## 6. MCP 集成（核心差异化）

### 6.1 工作模式

App 启动时同时拉起一个 **stdio 模式的 MCP Server** 子进程，外部 AI 客户端（Claude Desktop、Cursor、Cline 等）通过 stdio 与之通信。

```
[Claude Desktop]  ──stdio──▶  [simple-post-mcp]  ──in-proc──▶  [SQLite]
                                          ▲
                                          │ 状态展示
                                   [Main Process]
                                          ▲
                                          │ IPC
                                   [Renderer UI]
```

### 6.2 工具清单（MVP + v0.2）

| 工具名 | 用途 |
|---|---|
| `api_list_projects` | 列出所有项目 |
| `api_list_endpoints` | 按项目/模块/集合过滤列出接口 |
| `api_search` | 按关键字、tag、method 搜索 |
| `api_get_endpoint` | 取单个接口完整定义 |
| `api_run_request` | 执行 HTTP 请求，返回结构化响应 |
| `api_create_endpoint` | 创建接口 |
| `api_update_endpoint` | 更新接口 |
| `api_delete_endpoint` | 删除接口 |
| `api_import_postman` | 导入 Postman collection JSON |
| `api_export_openapi` | 导出 OpenAPI 3.0 |
| `api_generate_docs` | AI 生成/补全接口文档 |
| `api_generate_tests` | AI 生成测试用例 |

### 6.3 Resources

除了 Tools，也暴露 Resources，让 AI 可以"读取"：

- `simple-post://projects`
- `simple-post://projects/{id}`
- `simple-post://endpoints/{id}`
- `simple-post://environments/{projectId}`

### 6.4 配置分发给外部 AI

设置页提供 "复制 MCP 配置" 按钮，一键生成：

```json
{
  "mcpServers": {
    "simple-post": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\simple-post\\simple-post.exe",
      "args": ["mcp"],
      "env": { "SIMPLE_POST_DATA_DIR": "..." }
    }
  }
}
```

### 6.5 安全

- MCP Server **不暴露给网络**，仅 stdio。
- 可选 token 校验：MCP 调用需要 App 启动时生成的本地 token，校验写入 stdio 头。
- 危险操作（删除、批量改）二次确认（设置页可关掉"AI 写操作确认"）。

---

## 7. UI / UX 草图

```
┌─────────────────────────────────────────────────────────────────┐
│ ⬢ Simple Post        project ▾   env: dev ▾    🔌 MCP ●      ⚙ │
├──────────┬──────────────────────────────────────────────────────┤
│ ▾ 用户中 │  POST  /api/v1/users/login           [Send] [Save]   │
│   ▾ 登录 │ ┌────────────────────────────────────────────────┐  │
│     • 登 │ │ Params │ Headers │ Body │ Auth │ Docs │ Tests │  │ │
│     • 注 │ ├────────────────────────────────────────────────┤  │
│   ▾ CRUD │ │  key        value          desc        ☑        │  │
│ ▾ 订单中 │ │  page       1              当前页       ☑        │  │
│         │ │  size       20              每页大小     ☑        │  │
│  + 项目  │ └────────────────────────────────────────────────┘  │
│  + 模块  │                                                     │
│  + 集合  │  Response  200  ·  142ms  ·  1.2KB                  │
│          │  ┌────────────────────────────────────────────────┐ │
│          │  │ Pretty │ Raw │ Headers │ Timeline              │ │
│          │  ├────────────────────────────────────────────────┤ │
│          │  │ { "code": 0, "data": { "token": "..." } }     │ │
│          │  └────────────────────────────────────────────────┘ │
└──────────┴──────────────────────────────────────────────────────┘
```

UI 关键原则：
- 暗色为主，亮色可选
- 树 + 详情两栏；详情用 Tab 组织，body 编辑器用 CodeMirror（JSON 高亮 + 校验）
- 响应区可拖拽分隔
- 所有耗时操作（请求、保存、导入）有 Skeleton / Spinner
- 全局快捷键：`Ctrl+Enter` 发送、`Ctrl+S` 保存、`Ctrl+K` 命令面板

---

## 8. 实施阶段

### Phase 0 — 脚手架（1–2 天）
- 初始化 `electron-vite` + React + TS
- 配置 Tailwind、shadcn/ui、ESLint、Prettier
- 目录结构、IPC 类型、SQLite 仓库层

### Phase 1 — 数据层 + 基础 UI（1 周）
- 建表、迁移
- 树形导航（项目/模块/集合/接口）
- 详情页骨架（Tab 结构）

### Phase 2 — HTTP 调试核心（1 周）
- 渲染端表单（params/headers/body）
- Main 端 `runRequest`（undici）
- 响应展示、变量替换

### Phase 3 — MCP 集成（1 周）
- 独立 `mcp-server` 入口，stdio 模式
- Tools / Resources 实现
- 设置页：状态、配置、日志

### Phase 4 — 打磨 + 打包（3–5 天）
- 导入导出、快捷键、错误处理
- electron-builder 打包、自动更新（可选）
- 图标、安装页、关于页

---

## 9. 目录结构

```
simple-post/
├─ package.json
├─ vite.config.ts
├─ tsconfig.json
├─ tailwind.config.js
├─ postcss.config.js
├─ index.html
├─ src/                          # 渲染端
│  ├─ main.tsx
│  ├─ App.tsx
│  ├─ features/
│  │  ├─ project-tree/           # 侧边树
│  │  ├─ http-tester/            # 请求调试
│  │  ├─ response-viewer/        # 响应展示
│  │  ├─ environment/            # 环境变量
│  │  ├─ mcp-panel/              # MCP 状态/配置
│  │  └─ settings/               # 设置抽屉
│  ├─ components/                # 通用 UI（shadcn 风格）
│  ├─ stores/                    # zustand
│  ├─ lib/                       # utils, mock data
│  ├─ types/                     # 共享 TS 类型
│  └─ styles/
├─ src-tauri/                    # Rust 主进程
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ commands/               # Tauri commands
│  │  ├─ db/                     # rusqlite 仓库
│  │  ├─ http/                   # reqwest 封装
│  │  └─ mcp/                    # MCP 子进程桥
│  └─ icons/
├─ src-mcp/                      # 独立 MCP Server（Node）
│  ├─ package.json
│  └─ index.ts                   # @mcp/sdk stdio entry
└─ resources/                    # 应用图标
```

---

## 10. 关键依赖

**渲染端 (package.json)**
```jsonc
{
  "dependencies": {
    "@tauri-apps/api": "^2.0.0",
    "@tauri-apps/plugin-sql": "^2.0.0",
    "zustand": "^5.0.0",
    "@tanstack/react-query": "^5.0.0",
    "react": "^18.3.0",
    "react-dom": "^18.3.0",
    "@codemirror/lang-json": "^6.0.0",
    "@uiw/react-codemirror": "^4.0.0",
    "react-markdown": "^9.0.0",
    "lucide-react": "^0.460.0",
    "clsx": "^2.1.0",
    "tailwind-merge": "^2.5.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "autoprefixer": "^10.4.0",
    "postcss": "^8.4.0",
    "tailwindcss": "^3.4.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0"
  }
}
```

**MCP Server (src-mcp/package.json)**
```jsonc
{
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.0.0",
    "zod": "^3.23.0",
    "better-sqlite3": "^11.0.0"
  }
}
```

**Rust 端 (src-tauri/Cargo.toml)**
```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-sql = { version = "2", features = ["sqlite"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
rusqlite = { version = "0.32", features = ["bundled"] }
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
thiserror = "1"
```

---

## 11. 已确认的决策

| # | 项 | 决策 | 时间 |
|---|---|---|---|
| 1 | 技术栈 | Tauri 2 + React + Rust | 2026-09-11 |
| 2 | 存储 | SQLite（Rust 端用 rusqlite / tauri-plugin-sql；MCP 端用 better-sqlite3 独立 connection） | 2026-09-11 |
| 3 | MCP 工作模式 | stdio 模式 | 2026-09-11 |
| 4 | 第一阶段 | UI 先行（脚手架 + 假数据 + 主界面） | 2026-09-11 |

## 12. 仍待决策

1. **包名 / 应用名**用什么？是否沿用 `simple-post`？
2. **UI 风格**：暗色为主还是亮色为主？是否有现成设计参考？
3. **是否需要登录 / 云同步**？当前规划是纯本地。
4. **目标用户语言**：界面是否需要中英双语，还是先中文？
5. **MCP**：是否允许用户关闭（部分用户不想要）？是否需要远程 MCP（HTTP 模式）？
