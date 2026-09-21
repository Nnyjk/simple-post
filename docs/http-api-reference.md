# Simple Post — HTTP API 参考

> v0.2 起生效。`docs/openapi.yaml` 是机器可读权威；本文档是 curl 可复制的工作示例集。

## 1. 启动 serve-web

```bash
# 本地默认端口 4224 + 默认绑定 0.0.0.0（用户偏好）
simple-post serve-web --bind=0.0.0.0:4224 --data-dir=./data

# 跨机接入：加 password 后必须带 bearer
SIMPLE_POST_WEB_PASSWORD=secret \
  simple-post serve-web --bind=0.0.0.0:4224 --data-dir=/var/lib/simple-post
```

## 2. 公开 API vs 内部 API

| 分类 | 含义 | 在 OpenAPI 中 |
|---|---|---|
| `public` | 稳定 v1 契约；breaking change 需要 `/api/v2/*` 迁移 | 列举 |
| `internal` | 内部运维 / UI 投影；无版本承诺 | **不**列举 |

`web_api.rs::public_route_specs()` 是 single source of truth — 加新 endpoint 时必须显式标注 stability。

## 3. 公共契约示例

所有 curl 示例假设 `localhost:4224` 是本地实例；远程接入替换 host 即可。

### 3.1 健康检查（无需 auth）

```bash
curl http://localhost:4224/api/healthz
```

```json
{ "status": "ok", "version": "0.2.0" }
```

### 3.2 Projects

```bash
# 列出所有项目
curl http://localhost:4224/api/projects

# 创建项目
curl -X POST http://localhost:4224/api/projects \
  -H 'content-type: application/json' \
  -d '{"name":"Acme","description":"ACME API","color":"#3b82f6"}'

# 获取单个
curl http://localhost:4224/api/projects/p_demo

# 修改（PATCH）
curl -X PATCH http://localhost:4224/api/projects/p_demo \
  -H 'content-type: application/json' \
  -d '{"description":"updated"}'

# 删除
curl -X DELETE http://localhost:4224/api/projects/p_demo
```

### 3.3 Modules / Collections / Endpoints / Environments

每个资源都有相同的 REST 形态：`GET /api/<resource>` 列表、`POST` 创建、`GET /api/<resource>/:id`、`PATCH /api/<resource>/:id`、`DELETE /api/<resource>/:id`。

特别动词：

```bash
# 移动 module 到另一 collection
curl -X POST http://localhost:4224/api/modules/move \
  -H 'content-type: application/json' \
  -d '{"id":"m_1","parentId":"…"}'

# 搜索（按 method / name / url）
curl -X POST http://localhost:4224/api/endpoints/search \
  -H 'content-type: application/json' \
  -d '{"query":"users","method":"GET"}'

# 复制 endpoint
curl -X POST http://localhost:4224/api/endpoints/ep_1/duplicate

# 标记 environment 为 active
curl -X POST http://localhost:4224/api/environments/active \
  -H 'content-type: application/json' \
  -d '{"id":"env_1"}'
```

### 3.4 HTTP 请求执行

```bash
curl -X POST http://localhost:4224/api/http/send \
  -H 'content-type: application/json' \
  -d '{
    "method": "GET",
    "url": "https://api.example.com/v1/users",
    "headers": [{"key":"authorization","value":"Bearer …"}],
    "params": [],
    "timeoutMs": 30000
  }'
```

返回 `HttpResult`：

```json
{
  "ok": true,
  "status": 200,
  "headers": {"content-type": "application/json"},
  "body": "…",
  "elapsedMs": 187,
  "finalUrl": "…",
  "request": { "method": "GET", "url": "…" }
}
```

`HttpResult` 始终序列化（即便 transport 失败也是 `ok: false` + 完整 request 元数据）— 渲染端可以无差别展示。

### 3.6 Environments / Deployments 额外动词

```bash
# 设置单个变量
curl -X POST http://localhost:4224/api/environments/env_1/vars \
  -H 'content-type: application/json' \
  -d '{"key":"API_HOST","value":"api.example.com"}'

# 设置 base URL
curl -X POST http://localhost:4224/api/environments/env_1/baseurls \
  -H 'content-type: application/json' \
  -d '{"definitionId":"def_1","url":"https://api.example.com"}'
```

（v0.1.6 起这两个端点的实际路径请查 `web_api.rs`，本文档以 commit 2 后续 amendment 为准。）

## 4. 内部 API（不公开）

不保证兼容。常见用途：

```bash
# 看 daemon 状态
curl http://localhost:4224/api/mcp/status

# 启动 / 停止 / 重启 daemon
curl -X POST http://localhost:4224/api/mcp/start
curl -X POST http://localhost:4224/api/mcp/stop
curl -X POST http://localhost:4224/api/mcp/restart

# mcp_proxy binary 路径
curl http://localhost:4224/api/mcp/proxy-path

# MCP 能力策略（commit 1）
curl http://localhost:4224/api/mcp/capability
curl -X PUT http://localhost:4224/api/mcp/capability \
  -H 'content-type: application/json' \
  -d '{"policy":"safe_write"}'
curl http://localhost:4224/api/mcp/capability/tools

# 原始 preferences dump
curl http://localhost:4224/api/preferences
curl -X PUT http://localhost:4224/api/preferences \
  -H 'content-type: application/json' \
  -d '{"mcp.auto_start":true}'
```

## 5. 认证

| 部署形态 | 认证 |
|---|---|
| 本地 `localhost:4224`，未设 `SIMPLE_POST_WEB_PASSWORD` | 全部开放（默认本地信任环） |
| `SIMPLE_POST_WEB_PASSWORD` 已设 | `/api/*` + `/mcp` 必须带 `Authorization: Bearer <password>` |
| `/healthz`、`GET /` | 任何形态都无需 auth（健康检查 + SPA 首屏） |

远程接入必须设 password —— 见 commit 5。

## 6. 与 axum 0.7 path param 语法对齐

URL 模板使用 **`:name`**（不是 `{name}`），与 axum 0.7 + matchit 0.7 的语法一致：

```
/api/projects/:id
/api/endpoints/:id/duplicate
```

如果用 `{id}` 写，axum 会注册成字面 segment，请求会落到 SPA fallback。

## 7. 错误格式

所有错误返回标准 envelope（来自 `crate::error::AppError`）：

```json
{
  "code": "validation",
  "message": "name must not be empty"
}
```

`code` 是稳定的 snake_case 字符串（与 `AppError` 变体名一致）；前端 dispatch on `code`，不解析 `message`。

## 8. 文档生成

```bash
# 重新生成 OpenAPI yaml（从 web_api.rs::public_route_specs() 抽取）
node scripts/dump-openapi.mjs > docs/openapi.yaml

# 校验 OpenAPI schema 合法
npx @apidevtools/swagger-parser validate docs/openapi.yaml
```

commit 7 起还会加 `scripts/check-openapi-coverage.mjs` 在 CI 里强制保证 `public_route_specs()` 与 `tauri-commands.ts::WEB_ROUTES` 一致。

## 9. 何时升级 v2

触发条件（任一）：
- 删除或重命名一个 public endpoint
- 改变 request body 的字段名或类型
- 改变 response body 的字段名或类型
- 改变 URL 路径或 method

不带破坏性的扩展（增字段、加可选 query param、加新 endpoint）不需要 v2 — 直接在 v1 加。