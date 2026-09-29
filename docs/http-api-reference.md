# Simple Post — HTTP API 参考

> `docs/openapi.yaml` 是机器可读权威，由 `web_api.rs::public_route_specs()` 生成。
> **本文不重复它**——端点清单、入参出参、稳定性标注都去查那份 yaml。
> 这里只留下从 yaml 里读不出来的东西：会让请求静默走错路的坑。

## 1. 公开契约 vs 内部接口

| 分类 | 含义 | 在 OpenAPI 中 |
|---|---|---|
| `public` | 稳定 v1 契约；breaking change 需要 `/api/v2/*` 迁移 | 列举 |
| `internal` | 内部运维 / UI 投影；无版本承诺 | **不**列举 |

`web_api.rs::public_route_specs()` 是 single source of truth —— 加新 endpoint 时
必须显式标注 stability，否则它不会出现在 yaml 里。已知未列入的有
`/api/permissions/:projectId` 与 `/api/request-settings`（路由可用，只是没写进 spec）。

## 2. ⚠️ axum 0.7 路径参数是 `:id`，不是 `{id}`

URL 模板必须用冒号语法：

```
/api/projects/:id
/api/nodes/:id/duplicate
```

写成 `{id}` 时 axum 会把它注册成**字面 segment**，路由匹配不上，请求静默落到
SPA fallback —— 返回 200 加一个 HTML 页面，看起来像"接口没反应"。
OpenAPI 的规范写法恰好是 `{id}`，从 yaml 复制路径时**必须手工改回冒号**。

## 3. 错误信封

所有错误统一来自 `crate::error::AppError`：

```json
{ "code": "validation", "message": "name must not be empty" }
```

`code` 是稳定 snake_case 字符串（与 `AppError` 变体一一对应）；前端 dispatch
只认 `code`，不解析 `message`。新增变体时要同时改 `error.rs` 里的 `code()`
match，否则 wire 形状会静默改变。

## 4. 认证

| 部署形态 | 认证 |
|---|---|
| 本地 localhost，未设 `SIMPLE_POST_WEB_PASSWORD` | 全部开放（本地信任环） |
| `SIMPLE_POST_WEB_PASSWORD` 已设 | `/api/*` + `/mcp` 必须带 `Authorization: Bearer <password>` |
| `/healthz`、`GET /` | 任何形态都无需 auth（健康检查 + SPA 首屏） |

**远程接入必须设 password。** 反代配置见
[`reverse-proxy-nginx.conf`](reverse-proxy-nginx.conf) /
[`reverse-proxy-caddyfile`](reverse-proxy-caddyfile)，两者都含 `/sp` base path
与 WebSocket 升级处理。

## 5. 维护 openapi.yaml

```bash
node scripts/dump-openapi.mjs > docs/openapi.yaml    # 重新生成
npx @apidevtools/swagger-parser validate docs/openapi.yaml
```

### ⭐ 拆 `web_api.rs` 时的回归护栏

`openapi.yaml` 完全由 `public_route_specs()` 生成，所以它是判断"重构有没有
悄悄弄丢路由"的最强手段——比任何测试都确定，因为丢一个 `.route()` 或改错
method / path 都会立刻反映在 diff 上：

```bash
node scripts/dump-openapi.mjs > /tmp/before.yaml   # 动手前
# ……拆分……
node scripts/dump-openapi.mjs > /tmp/after.yaml    # 动手后
diff /tmp/before.yaml /tmp/after.yaml             # 必须为空
```

## 6. 什么时候需要 v2

触发条件（任一）：删除或重命名 public endpoint、改 request body 字段名/类型、
改 response body 字段名/类型、改 URL 路径或 method。

非破坏性扩展（加字段、加可选 query param、加新 endpoint）直接在 v1 上加。
