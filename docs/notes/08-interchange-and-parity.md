# 08 导入导出（interchange）与跨语言 parity

- **适用范围**：`crates/simple-post-interchange/`、`crates/simple-post-server/src/interchange_api.rs`、四个薄壳入口、`src/lib/transfer-file-save.ts`、仓库根的 `fixtures/*.json` 与两侧读取器。
- **权威来源**：`crates/simple-post-interchange/README.md`（448 行，契约与已核实陷阱）、`AGENTS.md` 的 Interchange 段、`crates/simple-post-interchange/src/{lib,ir,spec,project,diagnostics,fingerprint}.rs` 的模块文档、两侧读取器源码。
- **最后核对**：2026-11-13（v0.1.34）
- **前置阅读**：`01-overview.md`、`02-architecture.md`

## 结论

1. `interchange` 不是「domain 的另一个名字」，它是**三条性质不同的边**的适配层。把三条边当一种转换处理，就会在「有损」那条边上静默丢数据、在「投影」那条边上让导出自己选错环境。
2. 导入导出的**逻辑只有一份**，住在 server crate 的 `interchange_api.rs`；Tauri command 与 REST handler 是四个参数搬运壳。这条是 dual-transport invariant 的落地，少写一侧不会报错，只会静默少一半功能。
3. `fixtures/` 下的十个样本钉的是**已经写了两遍的规则**（Rust + TS 各一份实现），不是「测试数据」。改一条规则 = 一个 fixture + 两个实现 + 两侧读取器，同一个 commit。
4. 导出产物**不由自家解析器自证**：`openapiv3` 在 `cargo test` 里按官方 3.0 模型反序列化，Redocly 按规则集检查，两者读同一棵 `sample-exchange.json`。
5. 落库全程一个 `BEGIN IMMEDIATE` 事务，去重账本在写之前就用**克隆**跑一遍判定，空容器不写。
6. 导入后必须 `reloadWorkspace()`：`loadWorkspace()` 在 `nodesLoaded` 上早退，树已加载时按它等于什么都没做。

## 结构

### crate 布局与依赖方向

```
simple-post-server ──▶ simple-post-interchange ──▶ simple-post-core
        └──▶ simple-post-mcp ──▶ simple-post-core
```

- `simple-post-mcp` **刻意不依赖** interchange：MCP 的 scope 里没有导入导出工具，独立成 crate 就是为了让它不必背上 YAML 解析器（`serde_norway`）与两套外来格式模型。`crates/simple-post-mcp/Cargo.toml` 里只有 `simple-post-core`，这是可核对的事实，不是口头约定。
- `interchange` 自身不含 Tauri 也不含 axum；`server` 同时依赖它和 `mcp`。`src-tauri` 依赖的是 `simple-post-server`（`features = ["gui"]`），并不直接依赖 interchange——所以「桌面和浏览器走同一份编排」在依赖图上成立。
- 文件职责：`ir.rs` 交换树、`diagnostics.rs` 诊断通道、`fingerprint.rs` 请求身份、`import/{postman,openapi}/` 解析、`commit/` 落库、`project.rs` 反向投影、`openapi/` 产出文档、`spec.rs` 两侧共用的词汇表（`PATH_METHODS`、`MultipartPart` 的复用、JSON Pointer 转义）。

### 编排与四个薄壳

`crates/simple-post-server/src/interchange_api.rs`（462 行）是唯一实现，四个入口：

| 入口 | Tauri（`src-tauri/src/commands/interchange.rs`） | REST（`web_api/routes/interchange.rs`） |
|---|---|---|
| 预览 | `interchange_import_preview`（async） | `POST /api/interchange/import/preview` |
| 落库 | `interchange_import_commit` | `POST /api/interchange/import/commit` |
| 导出候选 | `interchange_export_preview` | `POST /api/interchange/export/preview` |
| 导出文档 | `interchange_export_document` | `POST /api/interchange/export/document` |

**为什么不住在 Tauri command 里**：逻辑写进某一侧，另一侧就静默少一半，而两边各自的测试都不会红。command 里的参数必须**逐个扁平列出**（`#[tauri::command]` 按参数名取 payload 键，不做结构体反序列化），否则桌面侧全部调用报 `missing required key opts`，而 REST 侧与 Rust 测试全绿。

`import_commit` 用 `commit_in_tx` 而非 `commit`：一次导入要写的不止树，Postman 环境是项目级另一张表，必须同生共死；而 `db::with_tx` 的闭包拿不到第二个连接（`BEGIN IMMEDIATE` 会与外层自死锁），所以编排层自己开事务、把「已持有连接」的版本传下去。

### 导入流水线

```
内容（文件文本 / URL 拉取）
  → sniff_and_parse：按已解析 JSON 的顶层键嗅探格式（YAML 才回落到行首匹配），只解析一次
  → Exchange（IR）：节点还没有 id，key 是源格式位置指针
  → import_preview：TransferNode 树 + 指纹命中标记 + plan_base_url 候选 + 诊断
  → 用户勾选（key 集合）
  → import_commit：重新解析同一份 content（纯函数，不缓存），commit_in_tx
        ├─ decide(...)：克隆账本上判定每个 key 的下场（写 / 跳过）
        ├─ 只建有「会被写入」端点的容器
        ├─ wire_base_url：地址写进 (环境 × 模块) 那一格
        └─ 同事务写 Postman 环境
  → 报告：created / skipped / base_urls / diagnostics
```

- URL 分支必须经 `simple_post_core::http_executor::execute_with_policy`（`check_url` 是唯一的 SSRF 防护），且必须显式给 `max_response_size_kb`——不给是 0，超限会被**截断后照样返回 200**，错误信息退化成「JSON 解析失败」。
- 预览只把**已落库**的行灌进账本，并且用 `contains` 而不是 `seen`：预览不能替落库先记一遍账。
- `plan_base_url` 是只读的，预览与落库共用同一个函数（落库在事务内重算）。它收集模块格与端点 override 的地址去重成候选，并取项目默认环境（最早创建的那个）。没有默认环境 → 地址无处可落，报告里 `environment_name` 为 `None`，用户能看见而不是以为已生效。
- 诊断呈现：`Diagnostic` 按端点归属到 `diagnostic_indexes`，否则同一个 `pointer` 在两个分组下各显示一次；`TransferDialog` 把 `info` 与 `warn/error` 分两栏。

### 导出

`project::module_to_ir` 读四张表（`nodes`、`endpoint_details`、`node_scripts`、环境格子）投影成只含一个模块的 `Exchange`，再 `retain_selected` 裁形状，最后 `openapi::to_document`。三个约定：`responses` 恒为合成的 `200 OK`（`StoredResponse` 里没有 schema，不能拿请求体冒充）；`servers` 逐 operation、先端点 override 再模块格、**不回退**；`tags` 只落最外层集合名。

`components.schemas` 只收集真的会被引用的定义：`mode = none` 的端点根本不写 `requestBody`，也就不收集——收藏了却没人引用会在读者眼里变成有意义的定义，Redocly 的 `no-unused-components`（显式打开为 `error`）正是查这个。

两个外部校验者：`openapiv3`（dev-dep）在 `cargo test` 里把两种编码都反序列化进官方模型，缺 `info.title` / 缺 operation 的 `responses` 都会炸；`npm run lint:openapi` 先用 `cargo run --example export_sample` 生成 `.scratch/openapi-export-sample.yaml` 再交给 Redocly。两者读同一棵 `crates/simple-post-interchange/fixtures/sample-exchange.json`（23 个端点，其中 `PROPFIND` 因在 `PATH_METHODS` 之外被带诊断跳过）。**自家导入器永远宽容**：往返测试只能证明「我们能读回自己写的东西」，证明不了「那是合法文档」，所以外部模型与 linter 不可省。

### 前端保存

`src/lib/transfer-file-save.ts`：桌面形态动态 `import('@tauri-apps/plugin-dialog')` 弹原生另存为，再动态 `import('@tauri-apps/plugin-fs')` 的 `writeTextFile` 写用户刚选的路径；浏览器形态回落 `Blob` + `<a download>`，并 `revokeObjectURL`。动态 import 是必须的——静态引入会让 `serve-web` 的 bundle 背上没有实现的 Tauri 代码。

`src-tauri/capabilities/default.json` 不给 `fs:default`（那会开放整个用户目录的读写），而是逐 verb 授权 `fs:allow-write-text-file` 并把作用域限定在 `$HOME/**/*`；`dialog:allow-save` 单独一条。越界错误长得像 IO 故障，`humanizeSaveError` 把它翻成「请存到文档 / 下载 / 桌面目录下」。

### parity 一览

所有路径相对仓库根。样本在**仓库根** `fixtures/`；`crates/simple-post-interchange/fixtures/` 只有 `sample-exchange.json`，两个目录不要混。

| fixture（条目数） | 钉住的规则 | Rust 读取器 → 实现 | TS 读取器 → 实现 |
|---|---|---|---|
| `resolve-parity.json`（15 cases） | 整条请求的装配：baseUrl 拼接、`{{var}}` 替换、收斜杠、query/header 排序、鉴权派生、body「先判空后模板化」；`missingVars` / `missingPathParams` 比**集合**不比顺序 | `crates/simple-post-mcp/src/variable_fill/resolve_parity_test.rs` → `variable_fill/resolve.rs::resolve_endpoint` | `src/lib/__tests__/http-build-parity.test.ts` → `src/lib/http-build/prepare.ts::prepareRequest` |
| `multipart-parity.json`（2 cases） | boundary 现生成故样本写 `<BOUNDARY>` 占位符；Content-Type 追加位置；用户自写的 Content-Type 怎么处理 | `crates/simple-post-mcp/src/mcp/http_ops/multipart_parity_test.rs` → `http_ops/multipart.rs` + `resolve_endpoint` | `src/lib/__tests__/http-build-multipart-parity.test.ts` → `src/lib/multipart.ts` + `prepareRequest` |
| `body-parity.json`（21 cases） | 逐字段 vs 逐段模板化、空键与停用行算不算、百分号编码字符集、产物是文本还是字节 | `crates/simple-post-core/src/body_parity_test.rs` → `body.rs::resolve_body` | `src/lib/__tests__/body-parity.test.ts` → `src/lib/body/index.ts::resolveBody` |
| `dynamic-parity.json`（5 merge + 15 serialize + 11 scan） | 池合并优先级（动态值胜同名环境变量，停用的环境变量不影响动态变量）、值序列化、哪些名字算被引用 | `crates/simple-post-core/src/dynamic/resolve_parity_test.rs` → `dynamic/{merge,serialize,names}.rs` | `src/lib/__tests__/dynamic-parity.test.ts` → `src/lib/dynamic-vars.ts::{mergePool,serializeValue,listDynamicNames}` |
| `path-params-parity.json`（32 cases） | 占位符识别范围（含 BOM 不是空白、未闭合大括号）与替换、`expectedMissing` | `crates/simple-post-core/src/path_params_test.rs` → `path_params.rs::{placeholders,substitute}` | `src/lib/__tests__/path-params.test.ts` → `src/lib/path-params.ts` |
| `base-url-parity.json`（11 cases + 13 rewriteCases） | 端点 override 优先、否则读 (环境 × 模块) 那一格、**无回退层**；改写命中要求路径边界，与地址来源无关 | `crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs` → `variable_fill/base_url.rs::resolve_base_url` + `core::url::apply_rewrite` | `src/lib/__tests__/base-url-parity.test.ts` → `src/lib/url.ts::{resolveEndpointBaseUrl,applyRewrite}` |
| `oauth2-parity.json`（13 cases） | token 挂上已解析请求的结果（注入后的 URL 与头列表），不比写法 | `crates/simple-post-core/src/oauth2/inject_parity_test.rs` → `oauth2/inject.rs::inject` | `src/lib/__tests__/oauth2-parity.test.ts` → `src/lib/oauth2.ts::applyOAuth2Token` |
| `oauth2-config-parity.json`（6 cases + 两份字段清单） | 只有 `tokenUrl` / `authorizationUrl` / `clientId` / `scope` 被替换；`clientSecret` / `password` / `authCode` 刻意不替换 | `crates/simple-post-core/src/oauth2/config_parity_test.rs` → `oauth2/config.rs::substitute_config` | `src/lib/__tests__/oauth2-config-parity.test.ts` → `src/lib/http-build/auth.ts::{OAUTH2_SUBSTITUTED_FIELDS,resolveAuth}`（**不替换**，只报缺变量） |
| `collection-order-parity.json`（8 cases） | 容器**直接持有**的接口先出，再子集合，递归；不是纯 DFS 前序 | `crates/simple-post-core/src/db/nodes_read_test.rs` → `db/nodes/read.rs::ordered_subtree_endpoints` | `src/features/batch-run/__tests__/collect.test.ts` → `src/features/batch-run/model/collect.ts::collectSubtreeEndpoints` |
| `placement-parity.json`（9 matrix + projectTopLevel） | `may_under` 的 3×3 矩阵；项目顶层只放得下 module | `crates/simple-post-core/src/domain/placement_parity_test.rs` → `domain/resource.rs::{may_under,may_sit_at_project_top}` | `src/lib/__tests__/placement-parity.test.ts` → `src/lib/node-placement.ts::mayUnder` |

注意 `oauth2-config-parity` 与其它九份不同：替换规则在 Rust 只有一份实现，TS 侧不复制实现，只复制**字段清单**并验「渲染进程不替换」。读表时不要以为两侧各有一份 `substitute`。

## 不变量

### 1. 三条边，三种性质

| 边 | 性质 | 失败形态 |
|---|---|---|
| `format → ir` | **有损**，必须产出 `Diagnostic` | 表达不了的构造静默消失 |
| `ir → domain` | **无损、全函数** | 落库时因某个 IR 形状而报错（不该发生） |
| `domain → ir` | **投影** | 导出自行选错环境 / 回退到别的模块地址 |

### 2. `ir` 必须自有类型的三条理由

1. **节点还没有 id**：预览阶段 `EndpointDetail.node_id` 与 `ResourceNode.id` 根本不存在，「先看数量、确认后落库」要求一个无身份形状。
2. **诊断无处安放**：`domain` 每个字段都是标称值，没有位置能说「这里原本是个 multipart file 字段，被丢掉了」；把「不静默丢弃」变成类型要求，只能靠独立通道。
3. **导出要跨四张表**：host 在环境格子里、端点自己的地址在 `base_url_override`，故投影必须先选定环境——这是投影逻辑，不属于任何一行 endpoint。同理 `IrEndpoint.path` 是 path-only，host 在 `format → ir` 那一侧就被分派到模块格或端点 override。

### 3. `IrNode.key` 的双重身份

`IrModule::key` / `IrCollection::key` / `IrEndpoint::key` 都是 `#[serde(default)] pub key: String`：

- **导入时**＝源格式里的位置指针（`/item/2`、`/paths/~1users/get`），与 `Diagnostic::pointer` 同一套 JSON Pointer 转义纪律（`~` 先于 `/`）。
- **导出时**＝`nodes.id`。
- 前端勾选传回的就是这个 key；`commit` 拿它当选择集与去重诊断的指针。三种容器都带它，故 `TransferNode.key` 与它同源。
- `#[serde(default)]` 不是风格：它让**手写**或**旧**的 IR JSON（缺 `key`）仍能反序列化，代价是缺失时得到空串——空串 key 会让选择集匹配不上，于是「整份文件看起来勾上了却一个都没写」。所以任何构造 IR 的地方都必须显式给 key。

### 4. 去重语义

指纹取 `method + path + 启用的 query + 启用的 path_params + body`（body 要算模式），排除 name / description / tags / auth / headers / 位置。每段先写长度再写原文，防止 `a=1,b=2` 与 `b=2` 拼出同一字节流。算法是 FNV-1a 128 取前 16 位十六进制——不用 `DefaultHasher`（跨版本不稳定），不拖 `sha2` 依赖。

「同一份 IR 落两次」第二次全跳过：账本先灌入已落库的行，`decide` 在**克隆**上跑一遍。去重是有状态的（`seen` 记账），所以判定只能走一遍，写在第二遍再判会得到两个不同答案。

### 5. 导出确定性与唯一性

- `operationId` 由动词 + 路径生成，路径分隔符换成双下划线（`/api/user-info` 与 `/api/user/info` 不撞名）；映射不是单射，故另有编号回退，规范要的是文档内唯一而不是可逆。
- schema 名由规范化 JSON（键字典序）的短哈希决定，不依赖 `serde_json` 的 `preserve_order` 特性是否被依赖图打开。

## 陷阱

1. **别把 `key` 当 path 用。** path 是地址，在源文件里未必指得到一行；诊断指针用 key，渲染层原样显示给用户，指向不存在位置的指针等于没给。
2. **重新导入不是「更新」，是「追加 + 跳过」。** 没有 id 匹配就谈不上 upsert；改了 body 再导一次会多出一个**看得见**的副本（指纹含 body，这是刻意的取舍——宁可多一个副本，也不要静默丢请求）。
3. **`retain_selected` 与 commit 的裁剪是两份实现**，刻意不共用：落库那份还要问指纹并把「为什么少写了几个」记成诊断。想合并它们会逼导出路径背一个用不到的账本。
4. **空容器判据是 `writable` 不是 `selected`**。全勾上却全被去重时，按 `selected` 判会让树里落下一排点开什么都没有的集合。
5. **导入落点永远是模块**。`CommitTarget::ProjectRoot` 在生产代码里没有调用方（只有 `commit/*_test.rs` 与 `project_test/` 用），UI 与四个入口都只传 `module_id`；`Module` 目标下 IR 顶层模块一律**降级成集合**，因为模块不能挂在模块下。
6. **REST 侧必须显式抬 `DefaultBodyLimit`**。axum 默认 2 MB，四个 interchange 路由都 `.layer(body_limit())`，上限 `TRANSFER_BODY_LIMIT_BYTES = FETCH_MAX_KB * 1024 * 2`（16 MB）。少了这层，浏览器 / Docker 回 413 而桌面正常——两侧分叉且无一声报错。同一份理由适用于 `/api/spec/upsert`（另一个上限常量）。
7. **URL 模式下去重与地址计划是在预览里算的第二次**，不是缓存的；`resolved_content` 把原文回传给前端，commit 再解析一遍。别「优化」成缓存——那需要临时表 + TTL + 清理路径。
8. **`fixtures/` 有两个根。** 仓库根的 `fixtures/` 是 parity 样本（两侧读取器 `include_str!` / `readFileSync`），`crates/simple-post-interchange/fixtures/sample-exchange.json` 是导出校验树。写新样本前先确认放哪个。
9. **改规则的纪律**：一个 fixture + 两个实现 + 两侧读取器，同一个 commit。读取器要 `deny_unknown_fields`（TS 无 serde，等价物是手写键名白名单，见 `oauth2-config-parity.test.ts::assertKnownKeys`）并断言条目数或「恰好覆盖一次」——键名漂了会让整条测试静默空转，绿的。
10. **`spec.rs` 里 `PATH_METHODS` 是导入导出共用的唯一词表**，不要在两处各写一份：`x-` 扩展键靠它被自然跳过，导出侧靠它挡住非法动词。

## 相关笔记

- [01-overview.md](./01-overview.md)：仓库全貌与本篇在其中的位置。
- [02-architecture.md](./02-architecture.md)：workspace 分层与 dual-transport invariant——本篇「编排为什么不住在 Tauri command 里」的正式出处。
- [03-data-model.md](./03-data-model.md)：`nodes` / `endpoint_details` / `node_scripts` / 环境格子的表结构，以及 `commit_in_tx` 的事务边界与指纹对着哪一行算。
- [04-request-pipeline.md](./04-request-pipeline.md)：resolve / multipart / body / path-params / base-url / oauth2 规则的**发送侧**那一半；本篇只从「谁读哪份样本」的角度定位。
- [07-frontend.md](./07-frontend.md)：`TransferDialog` / `TransferTree` / 勾选状态与 `reloadWorkspace` 的树刷新时序。
- `crates/simple-post-interchange/README.md` 的「已核实的陷阱」「导出侧还丢了什么：逐条结论」「Redocly 抓出来的三处真缺陷」是本篇不重复展开的部分。

## 文档与实现不一致

1. **`src/lib/__tests__/placement-parity.test.ts` 的文件头把 Rust 读取器写成 `crates/simple-post-core/src/placement_parity_test.rs`**，实际是 `crates/simple-post-core/src/domain/placement_parity_test.rs`（`#[path]` 挂载在 `domain` 模块下）。按头注释去找会找不到文件。
2. **`AGENTS.md` 第 194 行把 `base-url-parity.json` 的 Rust 侧写成 `core::url`**。实际读取器在 `crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs`，实现是 mcp 的 `resolve_base_url` 加 core 的 `apply_rewrite` 两处；core 只有改写，没有解析。同一份 `AGENTS.md` 第 75 行的说法是准确的，两处自相矛盾。
3. **「读取器要 `deny_unknown_fields` 并断言条目数」目前只在一部分读取器上成立**。严格的：`base_url_parity_test.rs`（全部结构体都带，且断言 `cases` / `rewriteCases` 的精确条数）、`placement_parity_test.rs`（带，且断言九格恰好各一次）、`body_parity_test.rs`、`oauth2/config_parity_test.rs`、`db/nodes_read_test.rs`。**没带的**：`variable_fill/resolve_parity_test.rs`、`path_params_test.rs`、`oauth2/inject_parity_test.rs`、`dynamic/resolve_parity_test.rs`、`mcp/http_ops/multipart_parity_test.rs`——这些只断言 `!cases.is_empty()`，键名漂移会静默空转。这是纪律与现状的差距，不是设计分歧。
4. **`fixtures/oauth2-config-parity.json` 与 `fixtures/placement-parity.json` 没有 `$comment`**，其余八份都有。靠 `$comment` 找「两侧读取器是谁」的读者会在这两份上落空；实际信息只在两侧测试文件的头注释里。
5. **`CommitTarget::ProjectRoot` 的模块文档把它写成真实选项**（「各自新建一个顶层模块。这是资源树里模块唯一的合法位置」），但生产路径没有调用方——`import_commit` 恒传 `Module`，导出也只做 `module_to_ir`。是「保留的 API 形状」还是「忘了接的项目级入口」，`[未核实]`。
