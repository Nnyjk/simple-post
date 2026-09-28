# 项目状态总览（单一事实源）

> 最后核对：2026-09-28 · 对应 HEAD `9328d89` + 本文所述的未提交修复
> 本文的数字全部由代码实测得出，**不是**估算。若本文与代码冲突，以代码为准并请修正本文。

## 一句话定位

面向 Windows 的桌面 API 调试器，核心差异化是**内置 MCP Server**——让 Claude Desktop / Cursor / Cline 直接列举、搜索、发送、增删改接口。四种分发形态共用一套 Rust 核心。

## 四个运行时

| 运行时 | 路径 | 行数 | 职责 |
|---|---|---|---|
| 前端 | `src/` | 18.9k | React 18 + TS，Zustand 状态，Vite 5 |
| Tauri 壳 | `src-tauri/src/` | 4.7k | GUI、`serve-web`、daemon 监管、端口注册 |
| 共享核心 | `crates/simple-post-core/src/` | 5.8k | 域模型、SQLite、HTTP 执行器、权限、错误 |
| MCP 服务 | `crates/simple-post-mcp/src/` | 7.5k | 44 工具 JSON-RPC dispatcher + axum daemon + stdio 桥 |

合计约 **36.9k 行**。测试分布：前端 7 个文件 / 161 例，Rust 23 个模块 / 244 例。

## 四种分发形态

1. **Tauri 桌面应用**（`npm run tauri:dev` / `tauri:build`）— 默认形态
2. **无头 Web**（`serve-web`）— axum REST + 静态 SPA
3. **Docker web 镜像**（`Dockerfile.web`）— alpine + musl，多架构推 GHCR
4. **独立 MCP 二进制 + npm 包**（`@simple-post/mcp-server`）— 供外部 AI 客户端

数据目录：`SIMPLE_POST_DATA_DIR` 或 `%APPDATA%/simple-post`。

## MCP 权限模型（两道人造闸，取严）

```
工具注册表 tool_table()  (44)
        │
        ├─ 闸 A：全局 capability 策略  ← 用户在设置页选的 read_only/safe_write/full_access
        │     REQUIRED_CAPABILITY 表必须「穷尽」，缺行 = 拒绝
        │
        └─ 闸 B：按项目 Permission   ← Read < ReadCall < Full，默认 read_call
              required_permission()，未知工具 = Full
```

`Capability` 总序：`Read < Create < Update < Delete < Send`。
`Send`（出站 HTTP）**只有** `full_access` 覆盖——这是刻意的「安全写 ≠ 自由外呼」边界。

⚠️ **加新工具必须改两处**：`tool_table()` 加一行，`REQUIRED_CAPABILITY` 也加一行。
漏了第二处，闸 A 会 fail-closed 拒绝该工具。回归网是
`every_registered_tool_has_a_capability` / `every_capability_row_names_a_registered_tool`
（`crates/simple-post-mcp/src/mcp/mod.rs`）。

## 已知技术债

| 项 | 位置 | 说明 |
|---|---|---|
| 巨型文件 | `ProjectTree.tsx` 1792 行 | 单组件承担整个树，建议拆虚拟滚动 + 节点子组件 |
| 巨型文件 | `web_api.rs` 1765 行 | REST 路由集中，可按资源拆模块 |
| 死命令 | `base_url_defs_list` | `lib.rs:278` 注册、`commands.rs:592` 实现，但无 `COMMANDS` 键、无 REST 路由 → 两种形态都调不到 |
| 缺 REST 路由 | `/api/permissions*`、`/api/request_settings*` | 仅 Tauri IPC 可用，OpenAPI 有意不收录 |
| 跨层 move | `module_move` 语义不对称 | MCP 走真 UPDATE（跨项目）；前端 `modulesMove` 只在项目内重排 |

## 质量门禁

`npm run check` = format:check → lint → typecheck → test → fmt:check → clippy。
CI（`.github/workflows/ci.yml`）跑同一套：web（Node 22）与 rust（windows-latest）两个 job。

集成测试（`test:mcp` / `test:web:*` / `perf:bench`）**需要已编译的 binary**，没有则退出码 2。
`build:tests` 是 CI 用的软门禁版本。

## 当前里程碑

**v0.1 MVP 已交付**。v0.2 剩余项见 `README.md` 路线图（勾选状态以代码为准——已有多项提前完成）。
