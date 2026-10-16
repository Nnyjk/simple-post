# simple-post-interchange

导入导出的转换层：**外来格式 ↔ 本仓库的标准格式**。

标准格式是 `simple-post-core::domain`（`ResourceNode` / `EndpointDetail` / `Environment`）。
本 crate 不持有任何自己的持久化模型，只做标准格式两侧的适配。

依赖方向单箭头：`simple-post-interchange → simple-post-core`。core 不知道 Postman 或
OpenAPI 的存在。`simple-post-mcp` **不依赖**本 crate —— scope 内没有任何 MCP 导入导出
工具，独立成 crate 就是为了让它不必背上 YAML 解析器与两套外来格式模型。

## 三条边，三种性质

这是整个设计的支点，不要把它们当同一种转换处理。

| 边 | 性质 | 失败形态 |
|---|---|---|
| `format → ir` | **有损** | 必须产出 `Diagnostic`。表达不了的构造不能静默丢 |
| `ir → domain` | **无损、全函数** | 不应失败。补 id / `root_id` / 时间戳 |
| `domain → ir` | **投影** | 导出要自己决定取哪个环境，因为 host 不在 endpoint 上 |

```
        特定格式                      标准格式                        存储
   ┌──────────────┐   ┌────────────────────────────┐   ┌────────────────────┐
   │ Postman v2.1 │──▶│                            │──▶│ db::with_tx        │
   └──────────────┘   │  ir（交换格式 / 中间态）    │   │  落库 + 指纹去重     │
   ┌──────────────┐   │  · 树（无 id）             │   └────────────────────┘
   │ OpenAPI 3.0  │──▶│  · 诊断                    │
   │   yaml/json  │──▶│  · 丢失集                  │   ┌────────────────────┐
   └──────────────┘   │  · server 提示             │──▶│ 读 4 张表投影        │
                     └────────────▲───────────────┘   └────────────────────┘
        ┌──────────────┐          │
        │ OpenAPI 3.0  │──────────┘
        │   （导出）   │
        └──────────────┘
```

## 为什么 `ir` 不能就是 `domain`

三条独立理由，都指向同一个结论。缺任何一条都会有人重新发明它。

1. **IR 里的节点还没有 id。** `EndpointDetail.node_id` 必填，`ResourceNode.id` 是主键。
   而导入要求「先预览数量、确认后再落库」——预览阶段这些东西**根本不存在**。
2. **诊断无处安放。** `EndpointDetail` 每个字段都是标称值，没有一个位置能说
   「这里原本是个 multipart file 字段，我丢掉了」。没有诊断通道，「不静默丢弃」
   只能靠 review 保证。
3. **导出侧读不到自己需要的东西。** `base_url_definition_id` 为空表示「用项目的 ★」，
   ★ 在 `projects` 表，URL 在 `environments.module_base_urls_json` 里按
   `service_id → module_id` 二级索引。所以导出必须自己决定**取哪个环境**——
   这是投影逻辑，不属于任何一行 endpoint。

**指纹去重算在 `ir` 上，两种导入器因此互相认得**：同一份接口用 Postman 导一次、
OpenAPI 导一次，第二次能认出是同一个 endpoint。指纹若按格式各算各的，这个性质
就没有了。指纹**不含** name / description / auth / tags（改名不该造出副本）。

## 模块布局

```
src/
├── lib.rs           # 本文件所述契约
├── diagnostics.rs   # Severity + Diagnostic —— 已定，三种实现共用
├── ir.rs            # 交换格式：ir::Exchange / IrModule / IrCollection / IrEndpoint
├── ir_test.rs
├── fingerprint.rs   # 稳定请求指纹
├── commit.rs        # ir → db::with_tx：补 id、校验放置、指纹去重
├── commit_in_tx     # 同上，但调用方自己开事务（编排层要用它把环境一起写）
├── project.rs       # domain → ir 投影（导出侧的四表 join）+ module_to_ir
├── openapi.rs       # 导出生成器：ir → OpenAPI 3.0 文档（JSON / YAML）
├── openapi_test.rs
├── roundtrip_test.rs # 导出产物被同一个 crate 的导入器读回
├── import/
│   ├── mod.rs       # 按内容嗅探格式 + 分派
│   ├── postman.rs   # Postman v2.1
│   └── openapi.rs   # OpenAPI 3.0（含 $ref 展开）
└── import/*_test.rs
```

`commit.rs` 住在本 crate 而不是 Tauri command 里，是刻意的：AGENTS.md 的
dual-transport invariant 说的就是逻辑泄漏到传输层的代价。落库逻辑只有一份，
Tauri command 与 REST route 都退化成薄壳。

`commit_in_tx` 单独暴露的理由是事务边界：一次导入要写的不止树，Postman 的环境是
项目级的另一张表，它必须与树同生共死；而 `db::with_tx` 的闭包拿不到第二个连接
（`BEGIN IMMEDIATE` 会与外层那个自己死锁）。编排层拿到的是「已持有连接」的那一版。

## `key`：IR 还没成为节点的那一半身份

`IrModule` / `IrCollection` / `IrEndpoint` 各有一个 `#[serde(default)] key: String`：

- **导入时**是源格式的位置指针（`/item/0/item/2/request`），与 `Diagnostic::pointer`
  同一套寻址纪律，故诊断能按前缀挂回它所在的行；
- **导出时**是 `nodes.id`。

它是勾选集的成员身份，也是两侧引用同一个条目的唯一句柄。`#[serde(default)]` 是为了
让没有 `key` 的旧 IR 仍能反序列化——新增字段不该让存量序列化数据失效。

集合树深度**没有上限**（外来文件可以套一百层 folder），故凡是**只读走查**它的代码一律
用显式栈而不是递归：`endpoint_keys_of`、`all_endpoints`、`mark_duplicates`、
`diagnostics_of`，以及两个解析器都是。`retain_module` / `retain_collection`
是唯一两个递归的：它们在下降的同时就把空分支剪掉了，深度上限就是输入深度本身，
而剪不剪得动与走不走得完是两件事。

## 导出侧的两个形状裁剪不是同一份

`Exchange::retain_selected` 与 `commit` 都接受一个选择集，但**刻意是两份**：
前者只需要形状裁剪（导出的文档里只留勾中的），后者要问指纹并记诊断（落库时要告诉
用户「按指纹跳过了 N 个」）。把它们合并会让导出侧背上指纹账本，而导出根本不查重。

## 导入的四个「不猜」

解析器遇到本仓库表达不了的构造时，一律出诊断 + 丢弃，**不静默降级**：

- Postman `formdata` 里 `type: file` 的分片 —— multipart 分片结构存不下文件内容。
- Postman 的 `auth` 方案里 oauth2 / digest —— `AuthConfig` 只有四种。
- OpenAPI 的 `apiKey` `in: cookie` —— `ApiKeyLocation` 只有 `Header | Query`。
- OpenAPI `paths` 里固定动词键集以外的动词 —— 塞进去会让整份文档不合法。
- OpenAPI operation 的 `tags[1..]` —— 一个接口只归第一个 tag（见下）。

**分组只认 `tags[0]`，没打 tag 的整份归到一个以 `info.title` 命名的分组里。**
落进已有模块时 IR 顶层模块一律降级成集合，故这条同时就是「OpenAPI tag → 目标模块下的
集合」。曾经按「一个 operation 一个模块」实现，于是**一个 tag 都不打的文档**（Apifox、
SpringDoc 导出的文件很常见）会变成 87 个同名集合——用户看到的是一屏重复的名字。
复制到第二个 tag 下同样不行：那是两条指纹相同的接口，落库时第二条被当成重复跳过，
预览里看起来像自己导了两份。多出来的 tag 全部留在端点的 `tags` 上，只是不再各自成组。

按路径前缀切分组是另一条路，但那是**猜**：文档没给的信息不猜。

反过来也一样：`AuthConfig` 是「判别字段 + 可选载荷」的**扁平**结构，解析器只在
`kind` 匹配时才读对应载荷。未选中方案的载荷同样为 `Some`，按载荷存在性分派会把
用户没选中的凭据注入请求。

## 环境与 baseUrl 的分工

Postman 的每个 environment 落成一个新 `Environment`，变量进 `variables`，
`is_active` 恒 `false`（导入一个环境不等于切换当前环境）。

**不导成 baseUrl**：服务的*列表*是项目级事实（`base_url_definitions.project_id`），
而*取值*是 per (环境 × 模块 × 服务)。外来格式给不出「模块 × 服务」这一层，硬塞会
造出一堆项目里并不存在的服务定义。

## 域模型加宽契约

`BodyMode` 已加宽（决策：加宽而非冻结）。**加宽是存储决策，代价是发送链路必须跟上。**

新增的 multipart 变体若只有存储、没有发送实现，会出现最坏的一种失败：
导入进来的 endpoint **存得对、发得错**。用户看不出任何异常，只会觉得「这个接口导入
之后行为不一样了」。所以下面三处是**前置**，不是后续：

- `src/lib/http-build.ts` —— 真正拼出 multipart body
- 请求体编辑器 —— 能渲染并编辑新模式
- `variable_fill` —— 能填充 multipart 分片的变量

`BodyMode` 加变体对已存的 `body_json` 是向后兼容的（旧值继续反序列化成功），
**不要**为此 bump `SCHEMA_VERSION` —— 那会让 `ensure_current_schema` 删掉用户的库。

## 导出的两个约定

**`responses`：合成默认，不猜。** 每个 operation 输出一个 `200: {description: "OK"}`。
`StoredResponse` 存的是实际响应，没有响应 schema；而 `RequestBody.schema` 那个字段
UI 从不写、仓库自己的测试喂的也是 `{"limit":"number"}` 这种形状提示而非合法
JSON Schema。所以没有可信数据源，猜出来的 schema 看不出不可靠，比不猜更糟。

**`servers`：逐模块取，不回退。** URL 是 per (环境 × 模块 × 服务) 且**刻意没有任何
回退层**。导出必须先选定一个环境，然后每个模块各取各的；该模块没配就是没有 URL，
不写 `servers` 条目，不借用别的模块的，也不落回项目 ★。

因为这条，文档**根上没有 `servers`** —— 提到根就成了全局默认，等于让没配的模块借到
兄弟模块的地址。`servers` 逐 operation 携带。

## 产物怎么被校验：两层，都不是自家解析器

自家导入器对自家产物永远宽容：导出器漏写 `responses`，导入器不会报错，它只是安静地
少一个响应，而那份文档已经不合法了。故校验走两个外部视角：

| 层 | 工具 | 跑在哪 | 查什么 |
|---|---|---|---|
| 模型 | `openapiv3`（dev-dep） | `cargo test`（`src/openapi_model_test.rs`） | OpenAPI 3.0 的 REQUIRED：`info.title`/`version`、每个 operation 的 `responses` 与 response 的 `description` |
| 规则 | Redocly | `npm run lint:openapi` → CI 的 web job | `path-parameters-defined`、`operation-operationId`、`security-defined`、`no-server-example.com` 等 |

**两层读的是同一棵树**：`fixtures/sample-exchange.json`。Rust 测试与
`examples/export_sample.rs` 都从它出发，故 linter 看到的与 `cargo test` 校验过的是
同一份东西。两边各写一份构造的话，它们迟早只覆盖一半。

Redocly 要的是磁盘上的文档，所以 `npm run lint:openapi` 先跑
`cargo run -p simple-post-interchange --example export_sample` 现场生成——**不提交
快照**，快照会长过期。规则取舍在仓库根的 `redocly.yaml`，**关掉的每一条都写了理由**；
关规则不是消错音的手段，下一个报错该去改生成器。

## Redocly 抓出来的三处真缺陷（都已修）

接 linter 之前，单测全绿、自家导入器也读得回自己的产物：

1. **路径模板参数一律写成 `in: query`。** `/users/{id}` 的 `id` 被当成 query 参数，
   工具会认为 URL 里那个 `{id}` 无人认领，生成的客户端也就不会把它当路径参数
   （`path-parameters-defined` 报 4 处）。
2. **占位符没有对应参数行时完全不声明。** 用户删了参数行但 URL 里占位符还在——
   缺声明的是文档，不是用户的配置。
3. **operation 没有 `operationId`**（`operation-operationId` 报 17 处）。现在由
   `动词 + 路径` 拼，**不取接口名**：改名是用户随手做的事，而 operationId 是代码
   生成器拿来给方法取名的。

另外把「没有 auth 就不写 `security`」改成**显式写空数组**。语义等价（都不要求凭据），
但缺席要读者自己推断一遍，且 `security-defined` 只认显式的那种。

## 已核实的陷阱

带 `file:line` 的都是实测，不是推测。

- **`body_schema` 不是一列。** `schema.sql:147` 只有 `body_json`，整个 `RequestBody`
  （含 `schema`）塞在里面。全仓库三个写入点：`domain/auth.rs:38`（定义）、
  `src/types/domain.ts:30`（TS 镜像）、MCP 的 `body_schema` 入参
  （`project_ops/endpoint/args.rs:30` → `write.rs:60`）。**UI 从不写它。**
  按列名去找会找不到。
- **模板语法两边同形。** `variable_fill/template.rs:93-95` 的 `is_word` 只认
  `[A-Za-z0-9_]`，Postman 的 `{{var}}` 同形，所以转换基本白送。但 Postman 实际会写
  `{{my-var}}`（连字符）与 `{{$timestamp}}`（动态变量）——两者都被现有解析器**原样
  放过**，既不报错也不解析。**这必须变成诊断**，否则就是静默失效。
  注意解析器住在 `simple-post-mcp`，而依赖方向不能反过来，所以它要被提升到 core。
- **`ApiKeyLocation` 只有 `Header | Query`**（`domain/auth.rs:72-78`），OpenAPI 的
  apiKey 还允许 `cookie`。导入遇到 `in: cookie` 出诊断。
- **`HttpMethod::Custom` 导不出 OpenAPI。** `domain/http.rs:22-27` 能装 `PROPFIND`，
  但 OpenAPI 3.0 的 `paths` 是固定键集（get/put/post/delete/options/head/patch/trace）。
  导出遇到集合外的动词出诊断。反向没问题：OpenAPI 的 `trace` 落成
  `HttpMethod::Custom("TRACE")`。
- **URL 拉取文档要显式抬高体积上限。** `http_executor::HttpRequestInput`
  的 `max_response_size_kb`（`http_executor.rs:96`）由调用方给，一份 3 MB 的
  OpenAPI yaml 会撞上 UI 的默认值。走 `execute_with_policy` 以保留 SSRF 防护
  （`check_url`，`http_executor.rs:226`）——不要另开一条裸 `reqwest` 通道。

## YAML 依赖（已核实，勿重新推导）

`rust-version = "1.77.2"`。四个候选的 MSRV 都兼容，**不存在 MSRV 降级问题**：

| crate | 最新版 | MSRV | 备注 |
|---|---|---|---|
| `serde_yaml` | 0.9.34+deprecated | 1.64 | 上游已弃用，不再维护 |
| **`serde_norway`** | **0.9.42** | **1.71.1** | **维护中的 fork，API 同 0.9 线，推荐** |
| `serde_yaml_ng` | 0.10.0 | 1.64 | 另一个 fork |
| `serde_yml` | 0.0.13 | 1.56.1 | 另一个 fork |

选 `serde_norway`：依赖一个已弃用的 crate 需要在 `Cargo.toml` 里解释为什么还留着它，
而这个理由不成立。

YAML→`serde_json::Value` 可行：OpenAPI 的键本来就得是字符串，非字符串键不在范围内。
`$ref` 展开因此就是 JSON Pointer 走查，不需要额外的解析结构。

一个依赖同时覆盖两个方向：#4 读 YAML，#5 写 YAML。
