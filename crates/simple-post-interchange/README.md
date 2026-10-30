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
3. **导出侧读不到自己需要的东西。** URL 住在 `environments.module_base_urls_json` 里按
   `module_id` 一级索引，端点自己的地址在 `endpoint_details.base_url_override`。
   所以导出必须自己决定**取哪个环境**——这是投影逻辑，不属于任何一行 endpoint。

**指纹去重算在 `ir` 上，两种导入器因此互相认得**：同一份接口用 Postman 导一次、
OpenAPI 导一次，第二次能认出是同一个 endpoint。指纹若按格式各算各的，这个性质
就没有了。指纹**不含** name / description / auth / tags（改名不该造出副本）。

## 模块布局

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
└── import/
    ├── mod.rs        # 按内容嗅探格式 + 分派
    ├── postman/      # Postman v2.1：tree / request / body / auth / template / script
    └── openapi/      # OpenAPI 3.0：refs（$ref 展开）/ params / auth / servers
```

测试与源文件同目录同名前缀（`mod_test.rs` / `tree_test.rs` …），跨文件共用的夹具
放同目录的 `support.rs`：`commit/support.rs` 放 `commit_all` 与 `nested_exchange`，
`import/postman/support.rs` 放 `collection_fixture`，这类夹具只留一份。

`commit/` 住在本 crate 而不是 Tauri command 里，是刻意的：AGENTS.md 的
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
用显式栈而不是递归。本 crate 里的 `endpoint_keys_of`（`ir.rs`）与两个解析器
（`import/postman/tree.rs`、`import/openapi/refs.rs`）都是显式工作栈；server crate 里的
`all_endpoints` / `mark_duplicates`（`interchange_api.rs`）同样是。`retain_module` /
`retain_collection` 是唯一两个递归的：它们在下降的同时就把空分支剪掉了，深度上限就是
输入深度本身，而剪不剪得动与走不走得完是两件事。（`diagnostics_of` 不在这份名单上——
它筛的是 IR 自带的扁平诊断列表，不走集合树。）

## 导出侧的两个形状裁剪不是同一份

`Exchange::retain_selected` 与 `commit` 都接受一个选择集，但**刻意是两份**：
前者只需要形状裁剪（导出的文档里只留勾中的），后者要问指纹并记诊断（落库时要告诉
用户「按指纹跳过了 N 个」）。把它们合并会让导出侧背上指纹账本，而导出根本不查重。

## 导入的「不猜」

解析器遇到本仓库表达不了的构造时，一律出诊断 + 丢弃，**不静默降级**：

- Postman `formdata` 里 `type: file` 的分片 —— Postman 导出的文件字段只有**路径**，
  没有文件本身的内容，故那份字节拿不到（分片结构能存字节，装的是别处读来的字节）。
- Postman 的 `auth` 方案里 oauth2 / digest —— 导入器只映射 noauth / basic / bearer /
  apikey 四种，其余方案出诊断并按无 auth 处理（`import/postman/auth.rs`）。
- OpenAPI 的 `apiKey` `in: cookie` —— `ApiKeyLocation` 只有 `Header | Query`。
- OpenAPI `paths` 里固定动词键集以外的动词 —— 塞进去会让整份文档不合法。
- OpenAPI operation 的 `tags[1..]` —— 一个接口只归第一个 tag（见下）。

**请求体上的 schema 读得进来。** OpenAPI 的 `requestBody.content[media].schema` 收进
`body.schema`，本地 `$ref` **展开成内联内容** —— `body.schema` 存的是一段自足的文本，
而导出时那份 `components` 是导出器自己生成的，存一段带 `$ref` 的文本进去，导出时那份引用
指不到任何东西。布尔 schema（`true` / `false`）不是本仓库导出器认的那种（它要写成
`components.schemas` 里的一个对象），故不收，并出一条 `Info` 说明。Postman 集合里没有
请求体 schema 这个构造，那边没有可导入的东西。

**一处例外：multipart 降级成 `Raw` 时，那份定义不跟着走。** multipart 的请求体既没有
样例、schema 里也没有字段时，导入器降级成 `BodyMode::Raw`（存原始文本）。而 Raw 的
media type 是 `application/octet-stream`，multipart 的 schema 说的是有哪些表单字段 ——
把它挂在 octet-stream 上，导出的文档就在宣称一个错的请求格式，而存下来的内容已经是
原始文本了，这份定义描述的也不是它。故降级时分片与那份定义一起丢，诊断里点名。

**分组只认 `tags[0]`，没打 tag 的整份归到一个以 `info.title` 命名的分组里。**
落进已有模块时 IR 顶层模块一律降级成集合，故这条同时就是「OpenAPI tag → 目标模块下的
集合」。曾经按「一个 operation 一个模块」实现，于是**一个 tag 都不打的文档**（Apifox、
SpringDoc 导出的文件很常见）会变成 87 个同名集合——用户看到的是一屏重复的名字。
复制到第二个 tag 下同样不行：那是两条指纹相同的接口，落库时第二条被当成重复跳过，
预览里看起来像自己导了两份。多出来的 tag 全部留在端点的 `tags` 上，只是不再各自成组。

按路径前缀切分组是另一条路，但那是**猜**：文档没给的信息不猜。

## 脚本：导入，以及导入不了的那些

脚本引擎（issue #7）落地之后本仓库能存脚本也能跑脚本，故导入器再把外来格式里的
脚本丢掉就成了**真的丢数据**。Postman 的脚本挂在 `event[]` 上，逐条处理：

| 脚本挂在哪 | 落到哪 | 说明 |
|---|---|---|
| 条目级（请求） | 该端点的两段脚本 | 端点的脚本住在 `endpoint_details`，与 body / headers 同级 |
| 条目级（嵌套 folder） | 该集合的两段脚本 | 集合的脚本住在 `node_scripts` 表 |
| 顶层 folder | 该模块的两段脚本 | 模块**不能**存脚本；挂进已有模块时它降级成集合，脚本就有归宿了，落到项目顶层时它只能是模块 —— 那时出诊断说明 |
| **集合根** | **没有落点** | 「整份集合」不是一个节点：顶层条目各自成一个模块，挂进已有模块时那些模块降级成集合，根上的脚本挂不到任何一个上 —— 逐条出诊断 |

同一相位的**多项**脚本拼成一段，不是只留第一项：Postman 里它们依次执行，只留第一项
等于悄悄改掉了用户的集合行为。

认不出的相位（典型是测试运行器的监听器）与读不出源码的 `event[]` 条目都出诊断，
指针指到那一条本身（`/item/3/event/0`）。**脚本源码不写进诊断** —— 用户的脚本里常有
token 与环境变量名，诊断是要显示给人看的。

反过来也一样：`AuthConfig` 是「判别字段 + 可选载荷」的**扁平**结构，解析器只在
`kind` 匹配时才读对应载荷。未选中方案的载荷同样为 `Some`，按载荷存在性分派会把
用户没选中的凭据注入请求。

## 环境与地址的分工

Postman 的每个 environment 落成一个新 `Environment`，变量进 `variables`，
导入的环境不改变任何「当前环境」——当前环境是每个客户端自己的实时状态，不落库。

**环境不导成地址**：地址是 per (环境 × 模块) 那一格，而 Postman 的 `variable[]` 给不出
「模块」这一层，硬塞会造出项目里并不存在的模块地址。

## 域模型加宽契约

`BodyMode` 已加宽（决策：加宽而非冻结）。**加宽是存储决策，代价是发送链路必须跟上。**

新增的 body 变体若只有存储、没有发送实现，会出现最坏的一种失败：
导入进来的 endpoint **存得对、发得错**。用户看不出任何异常，只会觉得「这个接口导入
之后行为不一样了」。所以下面几处是**前置**，不是后续：

- `src/lib/body/` —— 七种 body 形态的内容形状与装配规则（与 Rust 侧 `core::body`
  是同一套规则的两份实现，由 `fixtures/body-parity.json` 钉住）
- `src/lib/multipart.ts` —— 真正拼出 multipart body（与 Rust 侧
  `mcp/http_ops/multipart.rs` 是同一套规则的两份实现）
- 请求体编辑器 —— 能渲染并编辑新模式
- `variable_fill` —— `ResolvedEndpoint.body_mode`（`variable_fill/types.rs`）带着套用过
  `overrides` 之后的 mode，`auto_content_type`（`variable_fill/request.rs`）对 `Multipart`
  返回 `None`，boundary 由发送方在 execute 之前补上

`form` 模式的内容形状也一并改了：它从「一个拼好的 urlencoded 串」变成「行数组的
JSON 文本」。导入器因此写出**行数组**（`import/postman/body.rs`），界面上的表格才有
地方放，而被 Postman 禁用的字段也留得下来（标成不参与发送）。旧内容仍能读——
读不出行数组时按旧格式读。

`BodyMode` 加变体对已存的 `body_json` 是向后兼容的（旧值继续反序列化成功），
**不要**为此 bump `SCHEMA_VERSION` —— 那会让 `ensure_current_schema` 删掉用户的库。

## 导出的三个约定

**`responses`：合成默认，不猜。** 每个 operation 输出一个 `200: {description: "OK"}`。
`StoredResponse` 存的是实际响应，里面没有响应 schema；请求体那份定义
（`RequestBody.schema`）说的是**这个接口接受什么**，不能拿来当响应形状。所以没有可信
数据源，猜出来的 schema 看不出不可靠，比不猜更糟。

**`servers`：逐模块取，不回退。** 地址是 per (环境 × 模块) 且**刻意没有任何
回退层**。导出必须先选定一个环境，然后每个模块各取各的；该模块没配就是没有地址，
不写 `servers` 条目，不借用别的模块的。

因为这条，文档**根上没有 `servers`** —— 提到根就成了全局默认，等于让没配的模块借到
兄弟模块的地址。`servers` 逐 operation 携带。

**`tags`：就是最外层集合的名字。** tag 是 OpenAPI 里唯一的分组机制，不写它集合这一层
在产物里就彻底消失，「导出再导入」会把所有接口压平进一个分组（`roundtrip_test.rs`
里那条 `a_collection_comes_back_as_a_tag_named_group` 就是钉这个的）。

两条限定：

- **端点自己填了 tags 就不动。** 那是用户在请求上写的分类，与树结构无关。
- **只落最外层，因为 tag 是扁平字符串。** 集合的嵌套在 OpenAPI 里没有对应构造，
  `订单/明细` 导出成 tag `订单`，多出来那层读不回来。取最外层而不是最内层，是因为
  两个不同的顶层集合下各有一个同名子集合时，取内层会让它们在重新导入时并成一处。

### 已拍板：嵌套集合在 OpenAPI 往返中最多保留一层

这是 issue #67 的结论，**选 A（保持现状）**，理由与被否掉的另外两条一起记在这里，
免得下一个人重新推一遍：

| 方案 | 代价 | 结论 |
|---|---|---|
| **A. 只落最外层**（现状） | 三层以上的树导出再导入会变浅一层 | **采用** |
| B. tag 用路径（`订单/明细`） | 交给第三方的文档里会出现一个字面叫「订单/明细」的 tag —— Swagger UI / Apifox / YAPI 都直接显示 tag 字符串，斜杠让那边看起来像路径而不是分组 | 否 |
| C. 一个接口挂多个 tag | 导入侧只认 `tags[0]`，多挂的 tag 不带来层级，只在产物里多出一个被忽略的 tag | 否 |

**B 的代价落在别人读的产物上，收益只对「导出再导回自己」这一条路径有意义** —— 这条
不对称决定了选 A。

三层的样子：

```
模块
└── 集合 订单            ← tag 是「订单」
    └── 集合 明细        ← 这一层在 OpenAPI 里没有位置
        └── 集合 支付    ← 同样没有位置
            └── 接口 z
```

导出成 `tags: [订单]`，读回来变成：

```
模块 → 集合 订单 → 接口 z
```

**`明细` 与 `支付` 两层都没了。** 这不是缺陷，是格式本身的限制；本仓库的树契约允许
集合无限嵌套，OpenAPI 的 tag 表达不了一层的概念。

**Postman 侧不受影响**：Postman 的 folder 树在导入侧是**原样保留**的（folder → 集合，
嵌套多深都留着），只有 OpenAPI 走 tag 这条路。故这条限制只影响 OpenAPI 往返，
Postman → 本仓库的导入是保真的。

## 导出的另外三个约定

**`operationId`：`动词 + 路径`，路径分隔符落成双下划线。**
`operationId` 在多数代码生成器里会被直接变成方法名，而 `/` 与 `{}` 都不是标识符
字符，故它们都要换掉。**分隔符必须与其余字符区分开**：否则 `/api/user-info` 与
`/api/user/info` 撞成同一个名字 —— 规范要求 `operationId` 在文档内唯一，撞名之后
代码生成器会生成两个同名方法。现在两者分别是 `get___api__user_info` 与
`get___api__user__info`（确切的值由 `parameters_test.rs` 钉住）。

这仍不是**单射**（`/a-b` 与 `/a_b` 仍会得到同一个名字），故另有一道编号规则：撞上时后一个拿
`_2` 后缀。规范要的是「文档内唯一」，不是「映射可逆」。

`openapiv3` 校验的是模型（不校验唯一性），Redocly 也没开那条规则，故这条由
`openapi_model_test.rs` 里那条 `every_operation_id_is_unique_in_the_document` 守住，
夹具里放了一对会撞的路径。

**`requestBody`：端点带 `body.schema` 就 `$ref`，没带就只声明「这是一个 object」。**
字段**不猜**：端点没写 `body.schema` 时，导出器不知道这个接口接受哪些参数，故只声明
「这是一个 object」，不编一份形状出来。

带 schema 时它进 `components.schemas`，`requestBody` 里写
`{"$ref": "#/components/schemas/<名字>"}`。**收集了却不被引用是三种选择里最差的一种**：
读文档的人看到一堆没人引用的定义会以为它们有意义，而每个 requestBody 都在宣称一个错的
请求体形状。故收集与引用**同进同出** —— `mode = none` 的端点没有 requestBody，收集了也
没人引用，导出器索性不收集。

**名字由内容派生，不按出现顺序编号。** 位置编号（`Schema1`、`Schema2`…）有两个毛病：
它与内容无关，读者什么也看不出来；而删掉一个接口之后，后面每一个的名字都往前挪，同一份
文档在两次导出之间对不上号。派生规则：

| 情况 | 名字 | 例子 |
|---|---|---|
| schema 里有非空 `title` | 用它，经 component key 规范化（非法字符换 `_`） | `{"title":"UserProfile",…}` → `UserProfile` |
| 没有 `title` | `Schema_` + 该内容规范文本的短哈希 | `{"type":"object",…}` → `Schema_2d0515b0` |
| 上面的名字已被**另一份内容**占用 | 追加 `_` + 内容短哈希 | 两份都叫 `User` → `User` 与 `User_5b1c…` |

三点补充：

- **`title` 是 JSON Schema 自己的字段**，用户在请求体的 Schema 编辑区里写它，就是在给
  这份定义起名。界面上那份 Schema 区与载荷区并排常驻，写了 `title` 的定义导出后就是这个
  可读的名字。
- **没有 `title` 的那份一律带哈希。** 若给它一个裸的 `Schema`，「哪个拿到 `Schema`」就
  取决于它在文档里的先后 —— 那还是按位置命名。
- **同一份内容恒定同一个名字**：去重按**规范文本**（键序无关）比对，故同一份定义出现在
  两个接口上时只写一份，两个 operation 都 `$ref` 它。

两份内容不同的定义**绝不共用一个名字**：共用之后，后写的那份会覆盖前一份，产物里只剩一份
定义而两个 operation 都指着它 —— 读到的是错的形状，比没有更糟。

短哈希用 FNV-1a 128 位取前 8 个十六进制字符，与 `fingerprint.rs` 同一套算法：不依赖
`DefaultHasher`（它不保证跨版本稳定），也不为一个短标识拖进哈希依赖。

**「前端能不能写 `body.schema`」的结论：能写，已经实现了。**
请求体编辑器分左右两栏 —— 左边是这一次发出去的**载荷**（`body.content`），右边是文档里
那份**参数定义**（`body.schema`）—— 两者是两个字段，不是一份输入框（合成一个的话，写接口
定义时会把 Schema 当载荷发出去）。界面上写的定义经 `endpoint_details` 落库，导出时读它。

导入侧相应地也读它：OpenAPI 的 `requestBody.content[media].schema` 会被收进
`body.schema`（本地 `$ref` 展开成内联内容），故「导出再导入」不会丢掉这份定义。
Postman 集合里没有请求体 schema 这个构造，故那边没有可导入的东西。

**请求头：写成 `in: header` 的参数条目，不写值。**
OpenAPI 没有独立的「请求头」段落，头就是参数的一种位置；导入侧正是按 `in: header`
把它映回 `headers`，故导出侧写 `in: header`，「导出再导入」这条路才接得上 —— 否则头
在往返里整个消失。

值不写：值里常有凭据与 `{{变量}}`，一份面向人的契约文档不该带它们（本仓库导出凭据时
也只写「有这个凭据」不写值）。头**在不在**这件事由条目本身表达了。

同一个头名只写一条 —— OpenAPI 要求 `(name, in)` 唯一，重复的条目会让文档不合规；
被丢的那条出 `Info` 说明，不静默。

## 导出侧还丢了什么：逐条结论

本 crate 的纪律是「IR 里有而文档表达不了的，必须回诊断」。逐条过一遍外来格式里
本仓库读不到、或者读了但写不出的构造：

| 构造 | 处理 | 理由 |
|---|---|---|
| Postman `event[]`（前置 / 测试脚本） | **导入**（条目级）；集合根级的没有落点，出诊断 | 脚本引擎落地后本仓库能存能跑脚本，导入器再丢掉它就成了真的丢数据。集合根不是节点，挂不上去 |
| Postman `protocolProfileBehavior` | 不读，**不诊断** | Postman 自己的传输层行为开关，本仓库没有对应概念，读了也没有地方放 |
| OpenAPI `webhooks` | 不读，**不诊断** | 3.1 的构造，本仓库只做 3.0；`paths` 之外的构造一律不认 |
| OpenAPI `jsonSchemaDialect` | 不读，**不诊断** | 声明 schema 方言，本仓库不校验 schema 方言 |
| OpenAPI `components.pathItems` | 不读，**不诊断** | 3.1 的构造 |
| OpenAPI `callbacks` | 不读，**不诊断** | 本仓库的请求模型是「一个端点一次请求」，回调没有对应形状 |
| OpenAPI `servers[1..]` | 不读，**不诊断** | 一个 operation 只带一个 `servers` 数组，取文档根的那一份；多出来的 host 是「同一操作打到多个地址」，本仓库的地址是 per (环境 × 模块) 的单元格，表达不了 |
| 请求头的**值** | 不写进文档（头名写成 `in: header` 的参数条目） | 值里常有凭据与 `{{变量}}`；本仓库导出凭据时也只写「有这个凭据」不写值 |
| 端点 / 集合 / 模块上的**脚本** | 出诊断 | OpenAPI 没有脚本的构造 |

## 产物怎么被校验：两层，都不是自家解析器

自家导入器对自家产物永远宽容：导出器漏写 `responses`，导入器不会报错，它只是安静地
少一个响应，而那份文档已经不合法了。故校验走两个外部视角：

| 层 | 工具 | 跑在哪 | 查什么 |
|---|---|---|---|
| 模型 | `openapiv3`（dev-dep） | `cargo test`（`src/openapi_model_test.rs`） | OpenAPI 3.0 的 REQUIRED：`info.title`/`version`、每个 operation 的 `responses` 与 response 的 `description`；请求体的 `$ref` 指得到 `components.schemas` 里那一份 |
| 规则 | Redocly | `npm run lint:openapi` → CI 的 web job | `path-parameters-defined`、`operation-operationId`、`security-defined`、`no-server-example.com`、`no-unused-components` 等 |

**两层读的是同一棵树**：`fixtures/sample-exchange.json`。Rust 测试与
`examples/export_sample.rs` 都从它出发，故 linter 看到的与 `cargo test` 校验过的是
同一份东西。两边各写一份构造的话，它们迟早只覆盖一半。

**夹具里必须有带 `body.schema` 的接口。** 夹具是这两层唯一的输入，而 Redocly 的
`no-unused-components`（`recommended` 里没有，故 `redocly.yaml` 显式打开）查的正是
「`components.schemas` 里躺着没人引用的定义」。夹具里一个带 schema 的接口都没有时，
那条规则看不见任何东西 —— 这条缺陷当初正是这样在门禁里全绿的。故夹具里放了两个用同一份
定义的接口（只写一份、两个 operation 都 `$ref` 它）与一个没有 `title` 的定义
（名字由内容派生）。

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

- **`body_schema` 不是一列。** `schema.sql:155` 只有 `body_json`，整个 `RequestBody`
  （含 `schema`）塞在里面。全仓库三个写入点：`domain/auth.rs:42`（定义）、
  `src/types/domain/request.ts:37`（TS 镜像）、MCP 的 `body_schema` 入参
  （`project_ops/endpoint/args.rs:51` → `write.rs:189`）。**界面的请求体编辑器也写它**
  （右栏那块 Schema 区，见 `src/features/http-tester/BodyEditor.tsx`）—— 按列名去找会找不到。
- **模板语法两边同形。** `crates/simple-post-core/src/template.rs:102` 的 `is_word` 只认
  `[A-Za-z0-9_]`，Postman 的 `{{var}}` 同形，所以转换基本白送。但 Postman 实际会写
  `{{my-var}}`（连字符）与 `{{$timestamp}}`（动态变量）——两者都被现有解析器**原样
  放过**，既不报错也不解析。**这必须变成诊断**，否则就是静默失效。
  注意解析器住在 `simple-post-mcp`，而依赖方向不能反过来，所以它要被提升到 core。
- **`ApiKeyLocation` 只有 `Header | Query`**（`domain/auth.rs:117`），OpenAPI 的
  apiKey 还允许 `cookie`。导入遇到 `in: cookie` 出诊断。
- **`HttpMethod::Custom` 导不出 OpenAPI。** `domain/http.rs:22-27` 能装 `PROPFIND`，
  但 OpenAPI 3.0 的 `paths` 是固定键集（get/put/post/delete/options/head/patch/trace）。
  导出遇到集合外的动词出诊断。反向没问题：OpenAPI 的 `trace` 落成
  `HttpMethod::Custom("TRACE")`。
- **URL 拉取文档要显式抬高体积上限。** `http_executor::HttpRequestInput`
  的 `max_response_size_kb`（`http_executor.rs:104`）由调用方给，一份 3 MB 的
  OpenAPI yaml 会撞上 UI 的默认值。走 `execute_with_policy` 以保留 SSRF 防护
  （`check_url`，`http_executor.rs:349`）——不要另开一条裸 `reqwest` 通道。

## YAML 依赖（已核实，勿重新推导）

工作区的 MSRV 是 `rust-version = "1.87"`（根 `Cargo.toml`，本 crate 用
`rust-version.workspace = true` 继承它）。四个候选的 MSRV 都低于它，**不存在 MSRV
降级问题**：

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
