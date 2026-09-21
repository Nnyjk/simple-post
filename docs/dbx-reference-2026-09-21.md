# Simple Post — 外部参考与下一轮方向（2026-09-21）

> **本轮不动**。本文件记录 simple-post 跨端架构的外部参考对齐结论，以及 v0.1 之后可推进的方向候选，供下一轮决策参考。
> 已锁定的 v0.1 形态见 [`distribution-modes-2026-09-18.md`](./distribution-modes-2026-09-18.md)；本文件不替代也不修改那份方案，只在其基础上补充"还差什么、可以往哪儿走"。

## 1. 为什么看 dbx

[`t8y2/dbx`](https://github.com/t8y2/dbx) 是同领域的现成参考实现：Tauri 2 + Rust 同源后端，对外暴露 Desktop / Web(Docker) / CLI / MCP Server 四个端，20k+ stars，Apache-2.0。

simple-post 与 dbx 的形态高度同构（自带 MCP、桌面优先、考虑 Docker / Web 部署），dbx 是值得持续对照的成熟参照，但不复制其 UI 与功能集。

## 2. simple-post vs dbx 形态对照

| 端 | dbx | simple-post (v0.1 锁定) | gap |
|---|---|---|---|
| Desktop (Tauri) | Tauri 2 + Vue | Tauri 2 + React | — |
| Web / Docker | 完整镜像 + `:4224` HTTP API + reverse-proxy base path | `serve-web` 子命令 + axum，未 Docker 化 | 反代适配、镜像、Nginx/Caddy 文档 |
| MCP Server | 独立 `packages/mcp-server/` Rust crate + npm `@dbx-app/mcp-server` 包 | `src/bin/mcp_proxy.rs` stdio proxy，与主 binary 同 crate | 独立 crate / 独立 npm 包、可指向任意 web 后端（`SIMPLE_POST_WEB_URL`） |
| CLI | `@dbx-app/cli`（`dbx connections list --json`、`dbx query local "..."`） | 无 | 主 binary 内 `simple-post projects list --json` 等子命令 |
| HTTP API 公开 | 文档化的 `web-api` reference | 有 REST handler，未明确公开契约 | OpenAPI / 文档化 |
| MCP 权限策略 | `read_only` / `safe_write` / `high_risk_write` 三档，UI 设置面板配置 | 未分层 | MCP tool 标注能力等级 + 设置面板 Toggle |

## 3. 值得借鉴的三个具体模式

### 3.1 MCP server 与桌面 app 解耦，可指向任意后端

dbx 的 MCP server 是**独立 npm 包 / 独立 Rust 二进制**，与桌面 app 不捆绑。运行时不要求本机有 DBX 桌面端，通过环境变量指向任意 web 部署：

```json
{
  "mcpServers": {
    "dbx": {
      "command": "npx",
      "args": ["-y", "@dbx-app/mcp-server"],
      "env": {
        "DBX_WEB_URL": "http://your-server:4224",
        "DBX_WEB_PASSWORD": "your-web-password"
      }
    }
  }
}
```

对我们的启发：MCP 安装到 A 机器、A 想接入 B 机器部署的 web server 是合理诉求。simple-post 当前 stdio proxy 只能在同一台机器上访问 `simple-post serve-web`，不支持跨机部署被 MCP 接入。

### 3.2 MCP 权限三档（`read_only` / `safe_write` / `high_risk_write`）

dbx 在桌面 app 的 MCP 设置面板中提供三档权限，机器可读的 token 是 `read_only` / `safe_write` / `high_risk_write`，客户端配置不直接传 scope 环境变量，统一从设置面板读取。

对我们的启发：simple-post 当前的 MCP tool 是平铺的，AI 客户端接入后立刻获得全部能力。后续应：

- 给每个 MCP tool 标一个 `capability: Read | SafeWrite | Create`（或在 create 之上再拆 destructive 子档）
- `preferences` 加 `mcp.capability` 键（默认 `read_only`）
- 启动时按当前 capability 过滤 tool 列表；变更 capability 不需要重启 daemon，但要客户端感知（list tools 实时刷新）

back-compat 策略：保留一条环境变量（如 `SIMPLE_POST_MCP_ALLOW_WRITES=0|1`）作为"设置面板尚未首次落盘前"的兼容层，语义同 dbx 的 `DBX_MCP_ALLOW_WRITES`（首次保存后让位给持久化偏好）。

### 3.3 CLI 子命令 + `--json` 输出

dbx 的 `@dbx-app/cli` 把 domain 操作（`connections list`、`query local`）暴露成可脚本化的 CLI，子命令输出 `--json` 给 Codex / CI 用。

对我们的启发：simple-post 已经在 `commands.rs` 里有一套完整的 domain logic，加 CLI 子命令几乎零成本（套 clap 子命令壳）。形态候选：

```bash
simple-post projects list --json
simple-post projects get <id> --json
simple-post http send --project=<id> --endpoint=<id> --json
simple-post mcp status --json
simple-post serve-web --bind=0.0.0.0 --port=4224 --data-dir=<path>
```

CLI 与 web server 共用同一份 domain / http_executor / mcp supervisor，零业务代码重复。

## 4. v0.2+ 候选 backlog

按 dbx 形态清单，从依赖最少的排起：

1. **CLI 子命令**（最小）— 主 binary 加 clap 子命令 + `--json` 输出，复用 `commands.rs` / `db::*` / `http_executor::*`。零新依赖，纯路由层。
2. **MCP 权限三档**（小）— `preferences.mcp.capability` + tool 标签 + 启动过滤；保留环境变量兼容层。
3. **MCP server 独立 crate**（中）— `src/mcp/` + `src/bin/mcp_proxy.rs` 拆出 `crates/simple-post-mcp/`，引入 `SIMPLE_POST_WEB_URL` / `SIMPLE_POST_WEB_PASSWORD` 指向任意 web 部署。
4. **Web / Docker 部署**（中）— 出 `simple-post:web` 镜像，`-p 4224:4224`，固化数据卷；反代 base path（`SIMPLE_POST_PUBLIC_BASE_PATH=/sp`）；Docker Compose 文件。
5. **HTTP API 公开契约**（小）— 把现有 REST handler 整理成 OpenAPI / Markdown 文档，明确区分"公开稳定 API" vs "内部实现细节"。

不抄的实现决策（即便 dbx 有，simple-post 也不跟进）：

- dbx 的 SQL 方言/SQL 编辑器/驱动矩阵（simple-post 不做数据库客户端，定位不同）
- dbx 的多语言 UI（simple-post 中文优先，需要时再加）
- dbx 的 Marketplace 插件市场（simple-post 不做平台化）

## 5. 与 v0.1 方案的兼容性

本文件不引入对 v0.1 已锁定形态的破坏性变更：

- `mcp.auto_start` 偏好（v0.1.5 锁定）继续保留，独立于本文件提到的 `mcp.capability`（v0.2+）。二者并存：`auto_start` 决定 daemon 是否起，`capability` 决定起之后 AI 能用什么 tool。
- `serve-web` 子命令 + REST 路由（v0.1.6 锁定）继续保留，独立 crate / Docker 化是在其基础上的外延。
- 前端 `detect-runtime.ts` + `tauri-commands.ts` 抽象（v0.1.7 锁定）继续保留，CLI / 独立 MCP crate 都在 Rust 侧不影响前端 transport 抽象。

## 6. 决策记录

| 日期 | 决策 | 备注 |
|---|---|---|
| 2026-09-21 | 本轮**不**推进任何 dbx 对齐项 | 仅沉淀文档，作为决策依据 |
| 2026-09-21 | 下一轮候选入口 = CLI 子命令（依赖最少） | 等用户启动 v0.2 推进时再具体规划 |