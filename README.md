# Simple Post

一款面向 Windows 10/11 的现代化桌面 API 调试 / 管理工具，核心差异化：**自带 MCP Server**，让 AI（Claude Desktop、Cursor、Cline）能直接读写接口、生成文档、参与调试。

> 当前阶段：**UI 先行已完成**（2026-09-11）。脚手架 + Tailwind/shadcn 基线 + 完整数据层 + 四大主面板 + 所有 P0/P1 交互全部就位。下一步：进入真实业务层（v0.1 MVP），详见下方「路线图」。

## 跑起来

```bash
npm install
npm run dev        # 启动 Vite dev server → http://127.0.0.1:5173（LAN 可访问）
npm run build      # 产物到 dist/
npm run build      # 类型检查 + 打包（已有 tsc 内嵌）
npm run test       # 单元测试（http-build + http，纯函数 + node:http 集成）
```

> Vite 已配置 `host: '0.0.0.0'` + `allowedHosts: true`，局域网其他设备可直接访问本机 IP:5173。

## 已实现

### 数据层（Zustand + 内存 mock）
- 4 级层级：Project → Module → Collection → Endpoint，外加 Environment / Response
- 完整 CRUD：项目 / 模块 / 集合 / 接口的增删改查 + 复制接口 + 级联清理
- 排序：模块 / 集合 / 接口的上下移
- 响应：每个 endpoint 最多 10 条历史（FIFO）+ 用户保存的示例
- 偏好持久化：主题（dark/light/system）+ 请求设置（超时 / 跟随重定向 / 响应上限）写 localStorage

### 顶栏
- 项目 / 环境切换（点击下拉）
- 全局搜索（带 `Ctrl+K` 提示）
- MCP Server 状态指示器（running 绿点 / 离线灰点）
- 设置入口

### 侧边树（可展开 + 过滤）
- 4 级：项目 → 模块 → 集合 → 接口
- 每个集合 / 接口显示 method 彩色徽标
- 顶栏 `+` 真实弹新建弹窗
- 双击树项 inline 改名
- 右键菜单：新建子项 / 重命名 / 上下移 / 删除 / 复制接口
- 删除弹确认框

### 请求测试器
- Method 选择 + URL 输入（变量高亮显示）+ Send + **Copy as cURL** + Save
- Tabs：Params / Headers / Body / Auth / Docs / Tests
  - CodeMirror 6 JSON 编辑器，实时语法校验
  - Markdown 文档编辑器，分屏预览
  - Auth 4 种：None / Bearer / Basic / API Key
  - Tag 输入：chips 样式、逗号/回车添加、Backspace 删
- URL 输入 `{{` 触发 env 变量选择浮层（方向键选 + Enter 插入）
- 全局快捷键：`Ctrl+Enter` 发送 / `Ctrl+S` 保存 / `Ctrl+K` 命令面板

### Ctrl+K 命令面板
- 模糊搜索跨项目 / 模块 / 集合 / 接口
- 内置命令：切换环境、切换主题、打开设置、新建项目、复制当前 URL
- 键盘可达：↑↓ 选择 / Enter 触发 / Esc 关闭

### 响应区
- 状态码 / 耗时 / 大小 + 复制 / 下载
- Pretty / Raw / Headers 三个视图
- "Save as Example" 按钮（保存到接口示例）
- 响应历史折叠条（最近 10 条，可点击重放）
- 空集合 / 未发送时显示友好引导

### 设置抽屉
- 环境变量：3 个环境 × 3 个变量，`{{var}}` 实时编辑
- MCP Server 状态：running 指示 + 已注册 4 个工具列表
- 主题：dark / light / system 切换，立即生效并持久化
- 请求设置：超时 / 跟随重定向 / 最大响应体积
- 快捷键速查

### 主题
- dark / light / system 三选一
- 切换瞬时生效，刷新保留
- 全局 html class 切换，无白屏闪烁

### Mock 数据
- 3 项目 / 7 模块 / 8 集合 / 12 接口 / 3 环境
- 覆盖 SMS 登录、密码登录、用户资料、订单 CRUD、状态流转、收银台、抬头、开票、设备影子读/写

## 路线图

UI 先行阶段的所有打磨项（P0/P1/P2）已落地。下面是接 v0.1 起的四阶段路线，叠加 MCP 工具集（核心差异化）的渐进暴露。

### v0.1 MVP（约 4 周）

- [ ] **Tauri 壳**：`src-tauri/` 初始化、Cargo.toml、tauri.conf.json、图标
- [ ] **Rust 主进程**：SQLite（`rusqlite` + `r2d2`）、HTTP（`reqwest`）、MCP 子进程桥
- [ ] **真实持久化**：Zustand store → `tauri-plugin-sql`
- [ ] **真实请求**：渲染端 → Tauri command → `reqwest` → 返回结构化响应
- [ ] **MCP Server 子进程**：`src-tauri/src/daemon_http.rs` (axum JSON-RPC dispatcher, ~245 LOC) + `src-tauri/src/bin/mcp_proxy.rs` (stdio bridge, ~350 LOC) wired through the `src-tauri/src/mcp/` module — exposes 44+ MCP tools to AI clients (Claude Desktop, Cursor, …) over either transport.
- [ ] **打包**：NSIS 安装包 + 便携 zip

### v0.2（+2 周）

- 鉴权扩展：Bearer / Basic / API Key / OAuth2 简化版
- 响应历史 & 收藏（持久化到 SQLite）
- 导入 Postman v2.1 / OpenAPI 3.0
- 导出 OpenAPI 3.0
- MCP 工具扩展：`api_create` / `api_update` / `api_search` / `api_delete`
- MCP 进程可视化（设置页：PID、状态、tail 日志）

### v0.3（+2 周）

- AI 文档生成（端点 → Markdown 文档）
- AI 测试用例生成（schema → 测试）
- 请求脚本（pre-request / test 脚本，沙箱隔离）
- 集合批量运行 + 简易报告
- 团队协作（Git 同步 JSON 工作区，可选）

### v1.0

- WebSocket / SSE / GraphQL
- Mock Server
- 性能压测（简易）
- 插件系统（允许第三方 MCP 工具注册）

### MCP 工具清单（stdio 模式，给外部 AI 客户端）

> App 启动时同时拉起一个 **stdio MCP Server** 子进程，Claude Desktop / Cursor / Cline 等通过 stdio 与之通信。Server 不暴露网络，可选 token 校验。

| 工具 | 用途 |
|---|---|
| `api_list_projects` | 列出所有项目 |
| `api_list_endpoints` | 按项目 / 模块 / 集合过滤 |
| `api_search` | 按关键字、tag、method 搜索 |
| `api_get_endpoint` | 取单个接口完整定义 |
| `api_run_request` | 执行 HTTP 请求，返回结构化响应 |
| `api_create_endpoint` | 创建接口 |
| `api_update_endpoint` | 更新接口 |
| `api_delete_endpoint` | 删除接口 |
| `api_import_postman` | 导入 Postman collection JSON |
| `api_export_openapi` | 导出 OpenAPI 3.0 |
| `api_generate_docs` | AI 生成 / 补全接口文档 |
| `api_generate_tests` | AI 生成测试用例 |

Resources（AI 侧"读取"通道）：

- `simple-post://projects`
- `simple-post://projects/{id}`
- `simple-post://endpoints/{id}`
- `simple-post://environments/{projectId}`

## 协作方法（团队模式硬约束）

- **多 worker 并行只交付代码，禁止自行浏览器 / dev server 验证**——所有端到端验证、截图、流程跑通由**主会话统一做一次**。
- 详见 user memory 中的"多 worker 并行：只交付代码，不做端到端验证"条目。

## 决策记录

- **技术栈**：Tauri 2 + React 18 + TypeScript
- **存储**：SQLite（Rust 端 `rusqlite` / `tauri-plugin-sql`，MCP 端 `better-sqlite3`）
- **MCP 模式**：stdio（仅本地 AI 客户端可连）
- **UI 风格**：暗色为主，shadcn 变量层，可切亮色
- **第一阶段**：UI 先行 ✓（已完成 2026-09-11）