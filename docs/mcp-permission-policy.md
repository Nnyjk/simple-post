# Simple Post — MCP 权限策略

> v0.2 起生效。记录 simple-post MCP 服务的全局"能力策略"如何工作、与 dbx 的对照、以及如何扩展。

## 1. 三档策略

| 档位 | wire 值 | 可见工具 | 写入操作 | 删除 | HTTP 外发 |
|---|---|---|---|---|---|
| 只读（默认） | `read_only` | 仅 `Read` 类 | ❌ | ❌ | ❌ |
| 读写 | `safe_write` | `Read` + `Create` + `Update` | ✅ | ❌ | ❌ |
| 全部 | `full_access` | 全员 | ✅ | ✅ | ✅ |

**默认 `read_only`**——与 dbx 的 `read_only` 一致。AI 客户端接入后默认只能 list / get 现有数据，**无法** 修改任何状态，需要用户在「AI 写入权限」面板手动调档才能写入。

## 2. 与 dbx 的对照

dbx（`https://github.com/t8y2/dbx`）的 MCP 服务提供三档：`read_only` / `safe_write` / `high_risk_write`，machine-readable token 由 dbx 桌面 app 的 MCP 设置面板统一管理，**不** 直接传 `mcpServers` env。本实现的档位命名直接对齐 dbx（`high_risk_write` 在本项目里重命名为 `full_access`，更符合"全能力"语义；wire 字符串保持 dbx 风格）。

差别：
- dbx 的"destructive write"（删）与"high risk write"（外发 HTTP）合为一档；本项目把"删除"与"外发 HTTP"统一放到 `full_access`。
- dbx 的 token 走设置面板统一管理；本项目走 `preferences.mcp_policy` 设置项，等效。

## 3. 数据流

```
┌─────────────────────────────────────────────────────────┐
│ Renderer (React)                                       │
│   ConnectionSection.tsx                                  │
│   "AI 写入权限" Radio Group                              │
│     → invokeCmd(mcpCapabilitySet, { policy })           │
└─────────────────────────────────────────────────────────┘
                       │
                       ▼
        Tauri IPC  /  fetch /api/mcp/capability
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│ Rust                                                    │
│  commands::mcp_capability_get / set / list_tools        │
│  web_api::GET / PUT /api/mcp/capability                 │
│  web_api::GET /api/mcp/capability/tools                 │
└─────────────────────────────────────────────────────────┘
                       │
                       ▼
        db::preferences::mcp_policy / set_mcp_policy
                       │
                       ▼
        sqlite preferences(key='mcp.policy', value='read_only' | ...)
```

## 4. 过滤生效点

两层 gate，确保 AI 客户端无论用 `tools/list` 还是直接 `tools/call` 都不能越界：

1. **过滤 `tools/list`**：`mcp::tool_descriptors(state)` 按当前 `CapabilityPolicy` 过滤 `REQUIRED_CAPABILITY` 表，未通过的工具**不返回**。AI 客户端的下次 `tools/list` 看到的就是缩小的工具集。
2. **二次 gate `tools/call`**：`mcp::dispatch` 在 handler 之前再检查 capability。直接绕过 `tools/list` 调用未授权工具的，返 `AppError::PermissionDenied`。

未在 `REQUIRED_CAPABILITY` 表里的工具按"默认拒绝"处理——新增 tool 时必须显式标注 capability，否则只能在 `full_access` 下可见。

## 5. 持久化

`preferences.mcp.policy` 在 sqlite `preferences` 表里，类型 TEXT。默认 `read_only`。

| 写入路径 | 写入函数 |
|---|---|
| Tauri IPC | `commands::mcp_capability_set` |
| Web REST | `PUT /api/mcp/capability` body `{ policy }` |

读取路径：
- `tools/list` / `tools/call` 路径：`mcp::resolve_capability(pool, None)` → `db::preferences::mcp_policy(c)`
- 设置面板 hydration：`commands::mcp_capability_get` / `GET /api/mcp/capability`

## 6. 修补的 env_ops 漏洞

commit 1 同时修补了 5 个 env 工具的"project_id 假值"漏洞：

```
tool_env_update          -> "global" (漏洞)
tool_env_delete          -> "global" (漏洞)
tool_env_set_var         -> "global" (漏洞)
tool_env_set_baseurl     -> "global" (漏洞)
tool_env_set_active      -> "global" (漏洞)
```

这些 handler 之前传字符串 `"global"` 当 project_id 给 `check_permission()`，导致默认 `ReadCall` < 必需的 `Full`，**所有** 写入操作都会被静默拒绝。修补后：

```rust
let project_id = db::with_conn(&state.pool, |c|
    db::environments::lookup_project_id(c, &env_id))?;
mcp::check_permission(&state.pool, &project_id, "env_update").await?;
```

`env_id` 找不到时返 `AppError::NotFound`，不再静默失败。

## 7. 与 per-project 权限的关系

旧的 `permissions(project_id, permission)` 表保留并继续工作。两层叠加：

```
effective_capability = max(global_policy, project_level)
```

合并规则（保守优先）：
- `permissions.get(project_id) == "Read"` → 视为 `read_only`
- `permissions.get(project_id) == "ReadCall"` → 视为 `safe_write`
- `permissions.get(project_id) == "Full"` → 视为 `full_access`
- `global mcp.policy` 也覆盖同样的三档

二者取**较高**的那档。即：项目级 `Read` 不会让全局 `FullAccess` 升级，反过来全局 `ReadOnly` 也不会让项目级 `Full` 升级——**默认保守**。

## 8. 不破坏的兼容：
- `mcp.auto_start` 偏好独立运行（v0.1 锁定）
- `serve-web` 子命令 + 现有 32 个 REST 端点不受影响
- Tauri 内嵌 MCP daemon 行为不变，只是 `tools/list` 现在按 policy 过滤

## 9. 验证

```bash
cargo test --workspace
# 默认 mcp.policy=read_only → tools/list 仅返回 7 个 Read 类工具

node scripts/mcp-integration.test.mjs
# 切到 safe_write → tools/list 增加 Create/Update 类（不含 http_send、Delete）
# 切到 full_access → 全员

# env_update 在修补后能写
# env_update 在修补前会因 "global" 撞默认 ReadCall 而失败（regression 锁）
```

## 10. 已知限制 / 未来工作

- 客户端需要主动 `tools/list` 才能感知到工具列表变化。如果客户端缓存了上次的 `tools/list` 并继续调用旧工具，会被 `dispatch` 二次 gate 拒绝（`AppError::PermissionDenied`），但不强制 kill in-flight。v0.3 跟 MCP 2025-03 `tools/list_changed` notification 走。
- 项目级权限 UI 仍然是 v0.1 的 `AgentPermissionsTable`（per-project 三档），未与全局策略做"叠加显示"。v0.3 引入。
- `safe_write` 不区分"创建"和"修改"，统一放行。如果需要更细粒度（如"只能 update 现有 endpoint，不能 create 新 endpoint"），v0.3 引入 `Capability::CreateUpdate` 子档。