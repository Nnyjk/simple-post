# simple-post-interchange

`simple-post-interchange` 是外来格式与 `simple-post-core::domain` 标准格式之间的转换适配层，不持有持久化模型。

标准格式为 `simple-post-core::domain`（`ResourceNode` / `EndpointDetail` / `Environment`），本 crate 仅负责两端适配。

依赖方向为单向箭头：`simple-post-interchange → simple-post-core`。core 不感知 Postman 或 OpenAPI。`simple-post-mcp` **不依赖**本 crate；MCP 无导入导出需求，解耦可避免引入 YAML 解析器与外来格式数据结构。

## 三条边，三种性质

三条转换边的性质各不相同：`format → ir` 有损且必带诊断，`ir → domain` 为无损全函数，`domain → ir` 为依赖环境选择的投影。

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

`ir` 独立于 `domain` 存在：导入预览期节点尚无 ID，格式差异需诊断载荷承载，导出依赖跨表环境投影，多格式查重依赖通用指纹。

1. **IR 节点在预览期无 ID。** `EndpointDetail.node_id` 必填且 `ResourceNode.id` 为主键，但导入要求「先预览数量、确认后再落库」，预览阶段持久化 ID 尚不存在。
2. **`domain` 无法承载非致命诊断。** `EndpointDetail` 字段均为标称值，无法表达「multipart file 字段已被舍弃」等非阻断信息；缺乏诊断通道将导致静默丢弃无法被审计。
3. **导出投影依赖环境。** 模块 base URL 存放在 `environments.module_base_urls_json`（按 `module_id` 索引），端点自身地址存放在 `endpoint_details.base_url_override`；导出需指定目标环境以合成完整 URL，属于投影计算而非单行端点属性。

**指纹去重基于 `ir` 计算，跨格式去重全局一致。** 同一接口分别通过 Postman 与 OpenAPI 导入时可准确识别为同一端点。指纹由请求核心定义派生，**不包含** name / description / auth / tags，避免因重命名产生冗余条目。

## 模块布局

模块按职责解耦划分：`import/` 负责外来格式嗅探与解析，`ir.rs` 与 `fingerprint.rs` 提供中间态与去重指纹，`commit/` 处理事务落库，`openapi/` 处理导出生成。

```
src/
├── lib.rs            # 本文件所述契约
├── diagnostics.rs    # Severity + Diagnostic —— 已定，三种实现共用
├── spec.rs           # 两侧共用的词汇表：PATH_METHODS、JSON Pointer 转义
├── ir.rs             # 交换格式：ir::Exchange / IrModule / IrCollection / IrEndpoint
├── ir_test.rs
├── fingerprint.rs    # 稳定请求指纹
├── project.rs        # domain → ir 投影（导出侧的三表 join）+ module_to_ir
├── commit/
│   ├── mod.rs        # ir → db::with_tx：补 id、校验放置；commit_in_tx 在此
│   ├── base_url.rs   # plan_base_url —— 预览与落库共用的那一个地址判定
│   ├── dedupe.rs     # decide —— 按指纹裁定新增 / 重复 / 被勾选裁掉
│   └── support.rs    # 测试夹具：commit_all / nested_exchange
├── openapi/
│   ├── mod.rs        # 导出生成器：ir → OpenAPI 3.0 文档（JSON / YAML）
│   └── builder.rs    # 持有跨 operation 状态的 Builder
├── roundtrip_test.rs # 导出产物被同一个 crate 的导入器读回
├── project_test/     # project.rs 的用例：模块投影 + 夹具
└── import/
    ├── mod.rs        # 按内容嗅探格式 + 分派
    ├── postman/      # Postman v2.1：tree / request / body / auth / template / script
    └── openapi/      # OpenAPI 3.0：refs（$ref 展开）/ params / auth / servers
```

测试与源文件同目录同名前缀（如 `mod_test.rs`、`tree_test.rs`），跨文件夹具统一定义于同目录 `support.rs`（`commit/support.rs` 维护 `commit_all` 与 `nested_exchange`，`import/postman/support.rs` 维护 `collection_fixture`），禁止多处复制。

落库逻辑统一收敛于 `commit/` 模块，杜绝在传输层（Tauri command / REST route）编写业务逻辑，满足 dual-transport 不变量。

`commit_in_tx` 显式暴露事务连接以保证原子性。Postman 导入需同步写入环境表与资源树，若在 `db::with_tx` 闭包内部再次建连会因 `BEGIN IMMEDIATE` 触发自身死锁，编排层必须复用已持有事务连接。

## `key`：IR 还没成为节点的那一半身份

`key` 是 IR 节点的勾选身份句柄：导入期对应源格式指针（对齐诊断定位），导出期对应 `nodes.id`。

`IrModule`、`IrCollection` 与 `IrEndpoint` 均包含 `#[serde(default)] key: String`：

- **导入时**对应源格式位置指针（如 `/item/0/item/2/request`），与 `Diagnostic::pointer` 保持一致寻址规则，使诊断信息能按路径前缀挂回对应数据行；
- **导出时**对应已持久化的 `nodes.id`。

`key` 既是勾选集合的身份标识，也是跨端引用条目的唯一句柄。保留 `#[serde(default)]` 可保证旧版未携带 `key` 的 IR 数据仍能兼容反序列化。

所有只读遍历代码一律采用显式栈（防无限嵌套爆栈），仅剪除空分支的 `retain_*` 使用递归。集合树层级无上限约束，显式栈遍历包括 `endpoint_keys_of`（`ir.rs`）、解析器（`import/postman/tree.rs`、`import/openapi/refs.rs`）及上层服务的 `all_endpoints` / `mark_duplicates`（`interchange_api.rs`）。递归操作仅限于 `retain_module` 与 `retain_collection`，其下降过程即时剪枝，调用深度严格受限于输入本身。（`diagnostics_of` 直接过滤扁平诊断列表，不遍历树结构。）

## 导出侧的两个形状裁剪不是同一份

导出裁剪（`Exchange::retain_selected`）与落库裁剪（`commit`）职责分离：前者仅按勾选过滤树形状，后者需计算指纹并统计跳过项。

两处裁剪刻意保持独立：导出仅需依据勾选状态剔除未选项；落库除按勾选裁剪外，还需比对指纹并产出「已按指纹跳过 N 个」的诊断反馈。合并两处逻辑将导致导出流程无端背负指纹查重开销。

## 导入的「不猜」

导入器对无法映射的外来构造一律产出诊断并丢弃，绝不基于猜测进行静默兼容或降级。

- Postman `formdata` 中 `type: file` 的分片：Postman 导出仅提供文件路径而无字节内容，无法载入（分片结构仅承载预读入的实体字节）。
- Postman `auth` 中的 oauth2 与 digest：导入器仅支持 noauth、basic、bearer 与 apikey，其余认证出诊断并按无认证回退（`import/postman/auth.rs`）。
- OpenAPI `apiKey` 的 `in: cookie`：`ApiKeyLocation` 仅支持 `Header | Query`，遇到 cookie 出诊断（详见 [§已核实的陷阱](#已核实的陷阱)）。
- OpenAPI `paths` 包含标准动词外的自定义操作：导出规范不支持，导入时遇到集合外动词出诊断（详见 [§已核实的陷阱](#已核实的陷阱)）。
- OpenAPI operation 的多重 tags（`tags[1..]`）：单接口仅归入首个 tag 分组（详见下方分组规则）。

**请求体 schema 内联展开。** OpenAPI 的 `requestBody.content[media].schema` 提取至 `body.schema`，本地 `$ref` 一律展开为内联内容——`body.schema` 需独立自足，避免导出时因依赖重新生成的 `components` 导致引用悬空（存储结构详见 [§已核实的陷阱](#已核实的陷阱) §`body_schema 不是一列`）。布尔 schema（`true` / `false`）非对象定义，导入予以忽略并输出 `Info` 诊断。Postman 无请求体 schema 构造，导入时不作处理。

**multipart 降级为 `Raw` 时丢弃表单 schema。** 当 multipart 缺乏样例且无表单字段定义时，导入器降级为 `BodyMode::Raw`（媒体类型映射为 `application/octet-stream`）。此时保留针对表单字段的 schema 会导致导出的文档格式矛盾，因此降级时分片与 schema 一同丢弃并输出诊断说明。

**分组严格依据 `tags[0]`，缺失 tag 则收敛为项目文档标题组。** 导入已有模块时 IR 顶层模块均降级为集合，对应「OpenAPI tag → 目标模块子集合」。未声明 tag 的文档统一归入以 `info.title` 命名的单一分组，避免按 operation 拆分产生大量同名碎片集合。接口不在多个 tag 下重复生成，否则会因指纹冲突被落库查重过滤；多余的 tag 保留在端点 `tags` 属性中但不参与树分组。

路径前缀切分属于推断行为，未在文档中明示的信息一律不作猜测。

## 脚本：导入，以及导入不了的那些

条目级与 folder 级 Postman 脚本分别映射至端点与集合节点；集合根脚本因无宿主节点出诊断并丢弃；诊断日志严格过滤脚本源码。

Postman 脚本按 `event[]` 逐条转换至对应层级：

| 脚本挂在哪 | 落到哪 | 说明 |
|---|---|---|
| 条目级（请求） | 该端点的两段脚本 | 端点的脚本住在 `endpoint_details`，与 body / headers 同级 |
| 条目级（嵌套 folder） | 该集合的两段脚本 | 集合的脚本住在 `node_scripts` 表 |
| 顶层 folder | 该模块的两段脚本 | 模块**不能**存脚本；挂进已有模块时它降级成集合，脚本就有归宿了，落到项目顶层时它只能是模块 —— 那时出诊断说明 |
| **集合根** | **没有落点** | 「整份集合」不是一个节点：顶层条目各自成一个模块，挂进已有模块时那些模块降级成集合，根上的脚本挂不到任何一个上 —— 逐条出诊断 |

同一相位的多个脚本按声明顺序拼接为单段执行脚本，避免仅保留首项破坏原有执行行为。

未知执行相位或无法提取源码的 `event[]` 条目均输出诊断并定位至原始指针（如 `/item/3/event/0`）。诊断信息不包含脚本源码，防止泄漏脚本内的敏感 token 或环境凭据。

`AuthConfig` 采用「判别字段 + 可选载荷」的扁平结构，解析器仅在 `kind` 匹配时读取对应载荷，未选中的载荷虽存有 `Some` 亦不参与分派，避免注入无效认证信息。

## 环境与地址的分工

Postman 的环境只变成一个新建的 `Environment` 及其 `variables` 键值。它不改客户端当前环境（那是各客户端的运行时状态，不落库），也不被当成 base URL：模块地址属于 (环境 × 模块) 单元格，Postman 的全局 `variable[]` 没有模块维度，推断出来的只会是错的基准地址。

## 域模型加宽契约

`BodyMode` 变体加宽属于存储层决策，要求协议装配、编辑器与请求发送全链路同步支持，且不修改 `SCHEMA_VERSION`。

新增 body 变体必须前置落实端到端支持，防止出现「存储有效但发送异常」的隐式缺陷：

- `src/lib/body/` —— 覆盖七种 body 类型的结构定义与装配逻辑（与 Rust `core::body` 逻辑统一，由 `fixtures/body-parity.json` 约束）。
- `src/lib/multipart.ts` —— 实现 multipart 请求体组装（与 Rust `mcp/http_ops/multipart.rs` 逻辑统一）。
- 请求体编辑器 —— 支持新模式的渲染与交互编辑。
- `variable_fill` —— `ResolvedEndpoint.body_mode`（`variable_fill/types.rs`）携带应用 `overrides` 后的 mode；`auto_content_type`（`variable_fill/request.rs`）针对 `Multipart` 返回 `None`，统一在发送执行前补全 boundary。

`form` 模式的内容现在是行数组的 JSON 文本，导入器直接产出这个形状（`import/postman/body.rs`），表格与字段禁用状态因此存得下；早期存的是拼好的 urlencoded 串，读的时候仍然认得。

`BodyMode` 新增变体不影响既有 `body_json` 的反序列化。这条改动不许递增 `SCHEMA_VERSION`：版本号一涨，`ensure_current_schema` 会删掉用户的库。

## 导出的三个约定

导出有三条约定：`responses` 用端点自己的定义，没定义才合成 `200`；`servers` 逐模块取，根上没有回退；`tags` 只取最外层集合名。

**`responses` 用端点自己的定义，一条都没有才合成默认值。** 导出 operation 用端点声明的 `ep.responses`；一条都没配时才写 `200: {description: "OK"}`。`StoredResponse` 存的是运行时响应实例，不是 schema，故不从调用历史反推入参结构。

**`servers` 逐模块取，没有全局回退。** 地址属于 (环境 × 模块) 单元格，本身就没有继承层。导出是对选定的目标环境做的：没配地址的模块不输出 `servers`，也不借用兄弟模块的地址。

文档根上不声明 `servers`，否则没配地址的模块会借到全局默认地址；`servers` 一律挂在 operation 上。

**`tags` 只取最外层集合名。** OpenAPI 分组全靠 tag；缺 tag 时重新导入会把所有接口并成一个平铺分组（`roundtrip_test.rs` 的 `a_collection_comes_back_as_a_tag_named_group` 守着这条）。

导出规则约束：

- **端点自定义 tags 优先保留。** 端点自带的 tags 属于用户自定义分类，与资源树层级解耦。
- **层级仅取最外层。** OpenAPI tag 仅支持扁平字符串，深层路径无法映射；取最外层可防止不同父集合下的同名子集合在重新导入时发生冲突合并。

### 已拍板：嵌套集合在 OpenAPI 往返中最多保留一层

OpenAPI 仅支持扁平 tag，故嵌套集合导出时仅保留最外层作为 tag，往返导入后深层集合会被压平（Postman 导入不受此限）。

依据 issue #67 裁定保持方案 A：

| 方案 | 代价 | 结论 |
|---|---|---|
| **A. 只落最外层**（现状） | 三层以上的树导出再导入会变浅一层 | **采用** |
| B. tag 用路径（`订单/明细`） | 第三方工具（Swagger UI / Apifox / YAPI）直接展示原始 tag 字符串，导致路径字符破坏界面分组 | 否 |
| C. 一个接口挂多个 tag | 导入端仅识别 `tags[0]`，其余 tag 均被忽略，且产物冗余 | 否 |

方案 B 的兼容代价影响所有外部阅读者，且仅在「导出后再导回自身」的单一闭环路径获益，故维持方案 A。

层级映射示例：

```
模块
└── 集合 订单            ← tag 是「订单」
    └── 集合 明细        ← 这一层在 OpenAPI 里没有位置
        └── 集合 支付    ← 同样没有位置
            └── 接口 z
```

导出生成 `tags: [订单]`，再次导入还原为：

```
模块 → 集合 订单 → 接口 z
```

中间层级丢失是 OpenAPI 格式本身的能力上限，不是实现缺陷：资源树允许无限制嵌套。

Postman 导入不受此限制：Postman 的 folder 层级结构在导入期可保真映射为多层嵌套集合。

## 导出的另外三个约定

导出扩展约定覆盖：`operationId` 生成与防重、`requestBody` 独立抽取与引用、请求头作为参数导出且不输出值。

**`operationId` 格式为「动词 + 双下划线路径」，保证文档内唯一。** 多数代码生成器将 `operationId` 映射为方法标识符，需将 `/` 与 `{}` 替换为合规符号。路径分隔符使用双下划线，以区分 `/api/user-info`（生成 `get___api__user_info`）与 `/api/user/info`（生成 `get___api__user__info`），确切规则由 `parameters_test.rs` 约束。

为处理非单射碰撞（如 `/a-b` 与 `/a_b`），冲突标识符递增 `_2` 后缀，满足规范对文档内唯一性的要求。该约束由 `openapi_model_test.rs` 中的 `every_operation_id_is_unique_in_the_document` 守住。

**`requestBody` 引用具名 Schema，缺失定义时声明为通用 Object。** 未提供 `body.schema` 时仅声明类型为 object，不猜测字段。携带 schema 时将其存入 `components.schemas`，并在 `requestBody` 中以 `{"$ref": "#/components/schemas/<名字>"}` 引用。定义收集与引用保持同步，`mode = none` 的端点不收集 schema，避免产生孤立未引用定义。

**Schema 命名基于内容哈希派生，不依赖出现位置。** 派生规则如下：

| 情况 | 名字 | 例子 |
|---|---|---|
| schema 里有非空 `title` | 用它，经 component key 规范化（非法字符换 `_`） | `{"title":"UserProfile",…}` → `UserProfile` |
| 没有 `title` | `Schema_` + 该内容规范文本的短哈希 | `{"type":"object",…}` → `Schema_2d0515b0` |
| 上面的名字已被**另一份内容**占用 | 追加 `_` + 内容短哈希 | 两份都叫 `User` → `User` 与 `User_5b1c…` |

命名约束：

- `title` 取自 JSON Schema 自身属性，用户在编辑器 Schema 区输入的标题即为导出名称。
- 无 `title` 的 schema 强制追加哈希，避免依据扫描顺序分配裸名称导致命名不稳定。
- 相同内容依据规范文本去重并共用唯一命名与 `$ref`，不同内容严禁重名以防定义覆盖。
- 短哈希采用 FNV-1a 128 位截取前 8 个十六进制字符，与 `fingerprint.rs` 算法保持一致。

端点 Schema 由前端独立维护并落库。请求体编辑器区分左栏载荷（`body.content`）与右栏定义（`body.schema`），定义经由 `endpoint_details` 持久化并供导出读取；导入时读取逻辑详见 [§导入的「不猜」](#导入的不猜)。

**请求头导出为 `in: header` 参数，不输出具体值。** OpenAPI 将头信息作为参数建模，导出端写入 `in: header` 条目以保障往返导入完整还原。请求头参数仅保留名称，省略实际值以避免泄露环境敏感凭据。重复头名称按规范去重，被忽略项输出 `Info` 诊断。

## 导出侧还丢了什么：逐条结论

IR 存在而目标格式无法表达的构造，统一产出诊断并做丢弃处理；非请求模型的构造则直接忽略且不诊断。

| 构造 | 处理 | 理由 |
|---|---|---|
| Postman `event[]`（前置 / 测试脚本） | **导入**（条目级）；集合根级出诊断 | 详见 [§脚本：导入，以及导入不了的那些](#脚本导入以及导入不了的那些) |
| Postman `protocolProfileBehavior` | 不读，**不诊断** | Postman 传输层私有开关，本仓库没有对应概念 |
| OpenAPI `webhooks` | 不读，**不诊断** | 属 OpenAPI 3.1 构造，当前仅支持 3.0，忽略 `paths` 外结构 |
| OpenAPI `jsonSchemaDialect` | 不读，**不诊断** | `body.schema` 不按方言校验 |
| OpenAPI `components.pathItems` | 不读，**不诊断** | 属 OpenAPI 3.1 构造 |
| OpenAPI `callbacks` | 不读，**不诊断** | 本仓库只有单次请求模型，没有异步回调拓扑 |
| OpenAPI `servers[1..]` | 不读，**不诊断** | 单个 operation 只带一个 server；地址以 (环境 × 模块) 为单元，表达不了多主机矩阵 |
| 请求头的**值** | 不写进文档（仅导出 `in: header` 参数条目） | 值常包含凭据或动态变量，文档仅声明参数存在性 |
| 端点 / 集合 / 模块上的**脚本** | 出诊断 | OpenAPI 规范无脚本定义模型 |

## 产物怎么被校验：两层，都不是自家解析器

OpenAPI 导出产物通过 `openapiv3` 模型断言与 Redocly 规范规则两层外部校验，并共用 `fixtures/sample-exchange.json` 输入源。

| 层 | 工具 | 跑在哪 | 查什么 |
|---|---|---|---|
| 模型 | `openapiv3`（dev-dep） | `cargo test`（`src/openapi_model_test.rs`） | OpenAPI 3.0 的 REQUIRED：`info.title`/`version`、每个 operation 的 `responses` 与 response 的 `description`；请求体的 `$ref` 指得到 `components.schemas` 里那一份 |
| 规则 | Redocly | `npm run lint:openapi` → CI 的 web job | `path-parameters-defined`、`operation-operationId`、`security-defined`、`no-server-example.com`、`no-unused-components` 等 |

单测与 linter 统一以 `fixtures/sample-exchange.json` 为基准输入源，避免多份测试用例产生覆盖盲区。

测试夹具必须包含携带 `body.schema` 的接口，以确保 Redocly 的 `no-unused-components` 规则能够有效覆盖组件引用校验。夹具中配对设置了共用 Schema 的端点及无 `title` 的端点。

Redocly 针对磁盘实体文件进行校验，通过 `cargo run -p simple-post-interchange --example export_sample` 动态生成目标文档，禁止提交静态快照。校验规则由根目录 `redocly.yaml` 管理，关闭的规则均显式注明原因。

## Redocly 抓出来的三处真缺陷（都已修）

引入 Redocly 规则校验修复了三项隐藏规范缺陷：路径参数错写为 query、缺失参数占位符漏声明、以及 operation 缺少唯一标识符。

1. **路径模板参数错写为 `in: query`。** `/users/{id}` 的路径占位符被标记为 query 参数，导致外部工具无法识别路径变量（`path-parameters-defined` 检出）。
2. **URL 占位符缺失对应参数行时未显式声明。** 用户未在配置行显式声明但 URL 包含占位符时，文档未作补全声明。
3. **operation 缺少 `operationId`。** 规范要求必须具备标识符，现通过「动词 + 双下划线路径」生成（`operation-operationId` 检出）。

未配置认证的端点由省略 `security` 改为显式输出空数组 `[]`，以满足 `security-defined` 规范匹配要求。

## 已核实的陷阱

跨模块与外来格式交互存在存储隐蔽字段、模板字符集分歧、枚举取值不对称及网络策略约束等实测边界陷阱。

- **`body_schema` 不是一列。** `crates/simple-post-core/src/db/schema.sql:124` 仅包含 `body_json`，`RequestBody`（含 `schema`）完整内嵌于该 JSON 字段中。全仓写入涉及 `domain/auth.rs:53`（类型定义）、`src/types/domain/request.ts:114`（前端接口定义）、MCP 入参（`project_ops/endpoint/args.rs:50` → `write.rs:206`）及界面请求体编辑器（`src/features/http-tester/BodyEditor.tsx`）。
- **模板语法看似同形但字符集存在分歧。** `crates/simple-post-core/src/template.rs:96` 的 `is_word` 仅识别 `[A-Za-z0-9_]`。Postman 支持的 `{{my-var}}`（连字符）与 `{{$timestamp}}`（动态变量）会被原样保留，导入器须针对此类标识符输出诊断说明。判定逻辑复用 core 的 `is_word`，确保诊断信息与运行时行为统一。
- **`ApiKeyLocation` 仅支持 `Header | Query`。** `crates/simple-post-core/src/domain/auth.rs:197` 仅声明 Header 与 Query，OpenAPI 的 `in: cookie` 无法映射，导入时输出诊断（详见 [§导入的「不猜」](#导入的不猜)）。
- **`HttpMethod::Custom` 无法导出为 OpenAPI 动词。** `crates/simple-post-core/src/domain/http.rs:25` 支持任意自定义动词（如 `PROPFIND`），而 OpenAPI 3.0 `paths` 仅支持标准动词集合，遇到自定义动词在导出时输出诊断；反向导入时标准外的 `trace` 映射为 `HttpMethod::Custom("TRACE")`。
- **URL 拉取文档需显式配置体积上限并遵循安全策略。** `http_executor::HttpRequestInput` 的 `max_response_size_kb`（`crates/simple-post-core/src/http_executor.rs:118`）默认配置可能拦截大体积 OpenAPI 文件。网络请求必须调用 `execute_with_policy` 触发 `check_url`（`crates/simple-post-core/src/http_executor.rs:458`）进行 SSRF 防御，禁止直连裸 `reqwest` 通道。

## YAML 依赖（已核实，勿重新推导）

工作区统一选用活跃维护的 `serde_norway` 处理 YAML 编解码，满足 MSRV 约束且免于解释已弃用依赖。

工作区 MSRV 为 `rust-version = "1.87"`（根 `Cargo.toml`），候选依赖均满足要求：

| crate | 最新版 | MSRV | 备注 |
|---|---|---|---|
| `serde_yaml` | 0.9.34+deprecated | 1.64 | 上游已弃用，不再维护 |
| **`serde_norway`** | **0.9.42** | **1.71.1** | **维护中的 fork，API 同 0.9 线，推荐** |
| `serde_yaml_ng` | 0.10.0 | 1.64 | 另一个 fork |
| `serde_yml` | 0.0.13 | 1.56.1 | 另一个 fork |

选用 `serde_norway` 可避免在 `Cargo.toml` 中引入弃用解释。YAML 解析映射为 `serde_json::Value`，OpenAPI 规范严格限定键为字符串类型，`$ref` 寻址直接按 JSON Pointer 处理，无需维护冗余抽象结构。单个依赖同时满足读取（#4）与导出（#5）双向诉求。
