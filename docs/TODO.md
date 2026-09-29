# 待办清单

> 本文档记录**已核实**的技术债与待办项。每一项都注明现状、目标、验收标准与护栏。
>
> 约定：
> - **不写行号**——行号在第一次改动后就失效，请按函数名 / 文件名定位。
> - 「现状」段落描述的是本文档创建时的代码事实；若与代码不符，以代码为准并顺手修正本文档。
>
> **2026-09-29 重要背景：`docs/` 已被删除，本文件是那里唯一幸存的内容。**
> `REFACTOR-PLAN.md`、`http-api-reference.md`、`openapi.yaml`、
> `reverse-proxy-{caddyfile,nginx.conf}`、`security/tauri-csp-null.md`
> 全部随之移除，Docker 打包（`Dockerfile.web` / `docker-compose.yml` /
> `docker/healthcheck.sh`）一并取消。因此：
> - **D2 / D4 / D7 的部分结论已成历史**——它们修复的正是那些已删文件里的失效引用。
>   条目保留是为了说明当时为什么那样改，不是待办。
> - 那些「yaml 表达不了的坑」已迁入 `AGENTS.md` 的 *OpenAPI* 一节。
> - **`docs/TODO.md` 自身是否也该删除，尚未决定**——它现在是一份没有归属目录的孤儿文档。
>
> **2026-09-29 附记：R1 / R2 / R3 三项结构性热点已全部完成**（拆分为
> `refactor/split-oversized-files` 分支上的三个提交）。三项记载的行数与
> handler 数当时均已失准，条目内已按实测订正并附上验收证据。
> **D5 是当前唯一剩余的未完成项。**
>
> **如何读本文档**：✅ 标记的条目**不是待办**，而是「当时为什么那样改」的
> 决策记录与证据留档。改动相关目录前值得一读，但不需要重做。
> 只有「待办」标记的条目需要动手。

## 快速索引

| # | 条目 | 类型 | 优先级 | 状态 |
|---|---|---|---|---|
| [D1](#d1-统一树结构的描述) | 统一树结构的描述 | 文档 + 代码 | P0 | ✅ 已完成（C1/C2/C3 + 文档层全部落地） |
| [D2](#d2-agentsmd-引用了已删除的文件路径--已完成) | AGENTS.md 引用已删除路径 | 文档 | P0 | ✅ 已完成 |
| [D3](#d3-readme-自相矛盾已删除的-npm-包--已完成) | README 自相矛盾 | 文档 | P0 | ✅ 已完成 |
| [D4](#d4-refactor-planmd-状态过期--已完成) | REFACTOR-PLAN 状态过期 | 文档 | P1 | ✅ 已完成 |
| [D5](#d5-scriptse2e-完全未记载) | `scripts/e2e/` 未记载 | 文档 + 工程 | P2 | 待办 |
| [D6](#d6-readme-mcp-工具清单漏列-endpoint_list--已完成) | 工具清单漏列 | 文档 | P2 | ✅ 已完成 |
| [D7](#d7-readme-宣称了根本不存在的-mcp-resources--已完成已删除该节) | README 宣称不存在的 MCP Resources | 文档 | P1 | ✅ 已完成（已删除该节） |
| [D8](#d8-agentsmd-自相矛盾且计数失准的-rust-测试布局--已完成) | AGENTS.md Rust 测试布局自相矛盾 | 文档 | P2 | ✅ 已完成 |
| [R1](#r1-commandsrs-过大已拆分) | `commands.rs` 过大 | 重构 | P1 | ✅ 已完成（拆为 `commands/` 模块树） |
| [R2](#r2-crud-slicets-过大已拆分) | `crud-slice.ts` 过大 | 重构 | P2 | ✅ 已完成（拆为 6 个子工厂） |
| [R3](#r3-environmentsectiontsx-过大已拆分) | `EnvironmentSection` 过大 | 重构 | P2 | ✅ 已完成（拆为 `rows/` + `hooks/`） |

---

# P0 — 文档与代码脱节

## D1 统一树结构的描述

**类型：文档 + 代码**

### 现状

> **2026-09-29 更新：本条的「现状」描述已被一次 schema 重构超越。**
> `modules` / `collections` / `endpoints` 三张表已合并为单张 `nodes` +
> `endpoint_details` 侧表，`parent_collection_id` 这个字段本身不存在了——
> 层级统一由 `nodes.parent_id` 表达，归属项目由 `nodes.root_id` 反规范化。
> 下面这张「四层 vs 递归」的实现位置表已全部失效，仅作为**成因**留档。
> 文档层子任务依然成立且更紧迫：现在文档描述的模型在代码里连字段名都对不上了。

所有文档都把数据模型描述成固定的四层：

> README「4 级层级：Project → Module → Collection → Endpoint」
> README 侧边树一节同样写「4 级：项目 → 模块 → 集合 → 接口」
> AGENTS.md 的架构描述沿用了同一说法

**但代码实现的是递归树。** 重构前的 `schema.sql` 的 `collections` 表有自引用外键：

```sql
parent_collection_id TEXT REFERENCES collections(id) ON DELETE CASCADE
```

全链路当时都已实现并生效（文件名已随重构删除）：

| 层 | 实现位置 | 做法 |
|---|---|---|
| 持久层 | `db/collections.rs` | `list_children` 按 `parent_collection_id IS NULL` 区分顶层 / 子级 |
| 领域层 | `domain.rs` 的 `Collection` / `CollectionPatch` | `parent_collection_id` 与对应的 double_option |
| MCP 层 | `project_ops/collection.rs` | `parent_collection_id`、`new_parent_collection_id` |
| 状态层 | `crud-slice.ts` 的 `indexCollectionChildren` | 按父分桶 + BFS 收集子树 |
| 视图层 | `tree-utils.ts` 的 `buildTree` | `Map<string \| null, Collection[]>` + 递归 `buildCollectionBranch` |
| 组件层 | `rows/CollectionBranch.tsx` | 自递归渲染 |

### 成因

`crates/simple-post-core/src/db/fixtures.json` 里 **8 个 collection 的
`parentCollectionId` 全部是 `null`**，没有任何嵌套样例。文档作者（含此前的 AI）
是从种子数据推导出「四层」的，而代码早已支持更深。

> 2026-09-29 更新：`fixtures.json` 已降级为 **test-only**（`db::init` 不再播种，
> 全新安装为空库）。所以「四层」的误读如今只来自文档本身——新用户看到的空工作区
> 不再提供任何样例来支撑或反驳这个说法。改文档时不必再考虑演示数据的观感。
> 该文件在 nodes 重构中保持了三段式结构（modules / collections / endpoints），
> 翻译到新表只发生在 `db::init::seed_fixtures` 一处，因此 C3 仍是待办。

### 文档层子任务 ✅ 已完成

> 2026-09-29：nodes 重构后这批子任务的**措辞需要重写**——不只是「四层改递归」，
> 文档里引用的表名、字段名、REST 路径、命令名全部已经不存在了。

- [x] README「数据层」一节改为 `nodes` + `endpoint_details` 两张表的描述，
      并说明「Project → Module → Collection（可递归嵌套）→ Endpoint」
- [x] README「侧边树」一节同步，并单列一段写明**暴露嵌套能力的入口**
      （右键「新建子集合」、拖拽的 into 落点、面包屑逐级展开）
- [x] AGENTS.md 中描述层级的段落同步：Architecture 一节新增「Resource tree」
      小节（单表模型 + `type` 矩阵是被强制的契约），`Key Directories`
      点名 `db/nodes.rs`，`Partial updates` 的示例类型从已删除的
      `EndpointPatch` 换成 `ResourceNodePatch` / `EndpointDetailPatch`
- [x] 在 schema 注释或 README 里点明：**集合层深度不设上限，UI 用递归组件渲染**，
      以及 `type` 字段的类型矩阵（module 只能顶层 / endpoint 只能挂 collection）
- [x] 明确哪些 UI 入口会暴露嵌套能力（右键菜单的「新建子集合」、拖拽的 into 落点）
- [x] 顺带清掉 `POST /api/endpoints/:id/send`（README）与
      `/api/endpoints/:id/duplicate`（http-api-reference）两处失效路径

### 代码层子任务

#### C1 — 统一「顶层集合」的判定 ✅ 已消解

~~同一条语义（`parentCollectionId` 为空即顶层）在 8 处以不同写法重复表达~~

**已随 nodes 重构结构性消解**：`parent_collection_id` 字段本身不存在了。
层级只有 `nodes.parent_id` 一个来源，「顶层」= `parentId === null`，
不存在第二种写法与之竞争。`module_id` 也退化为「向上走到第一个 module 节点」的
派生值，不再是集合行上的独立列。

类型矩阵另有一处强制：`ResourceNodeType::may_under` 定义了 module 只能顶层、
endpoint 只能挂 collection，SQLite trigger 与应用层双防线。
也就是说「集合能挂在任意深度」现在是**被校验的契约**，而不是靠各处自觉。

剩余的 `parentModuleId` / `parentCollectionId` 只存在于**新建弹窗的草稿状态**
（`newItem`）里——那是两个下拉框的 UI 选值，不是领域字段；
`submitNewItem` 的实际写入目标是单个 `parentId`。

#### C2 — 合并两处集合分桶实现 ✅ 已完成

`tree-utils.ts` 的 `buildTree` 与 `crud-slice.ts` 的 `indexByParent`
曾是**同一套分桶 + 排序逻辑的两次独立实现**，唯一差别是前者按 `rootId`
过滤出单个项目、后者处理全量节点。改排序键、加环检测、或调整空桶处理时，
极易只改一处——而这两处喂给完全不同的消费者（视图渲染 vs 移动操作），
行为不一致不会立刻暴露。

**已抽出 `src/lib/tree-index.ts::indexByParent(nodes, rootId?)`**，
项目过滤作为可选参数（省略 = 全工作区）。两处调用点都改为调用它，
`crud-slice` 的私有副本已删除。`collectSubtree` 留在 store——它只有 store
这一个消费者（渲染器是自顶向下递归，不是 BFS），搬进共享模块只会撑大公共面。

> 2026-09-29 补记（R2 拆分后）：`collectSubtree` 现位于
> `src/stores/slices/crud-helpers.ts`，`indexByParent` 仍从 `@/lib/tree-index`
> 导入。R2 刻意没有动这两者的归属，理由与上面一致。

> 实际动手时发现的是**三份**而不是两份：第三份在 `NewItemModal.tsx`
> 的 `collectionsUnderModule`（本次重构新写的）。合流方式是把类型过滤
> 放进遍历里，而不是给共享签名加一个 `type` 谓词：
> `for (const k of byParent.get(parentId) ?? []) { if (k.type !== 'collection') continue; … }`。
> 桶已经由共享函数排好序，所以原来的每层 `[...kids].sort()` 一并消失。
> 从 `moduleId` 向下遍历天然走不到别的项目/模块，因此不需要 `rootId` 过滤。

新增 `src/lib/__tests__/tree-index.test.ts`（14 个用例）锁定：按 `parentId`
分桶、`null` 独立成顶层桶、每桶按 `sortOrder` 升序且与输入顺序无关、
`rootId` 过滤的开/关、孤儿节点（`parentId` 指向不存在的父）仍进桶、
以及**入参数组与节点对象都不被改写**。

#### C3 — fixtures 补一个嵌套集合样例 ✅ 已完成

`fixtures.json` 新增一个子集合 `c_order_refund`（挂在 `c_order_crud` 之下，
`moduleId` 仍写 `m_order` 以便派生 `root_id`）与一个属于它的接口
`e_refund_create`，递归路径的两段边（collection→collection、
collection→endpoint）在测试数据里第一次真实存在。

**插入顺序是硬约束**：`nodes.parent_id` 有外键，而 `seed_fixtures` 按
modules → collections → endpoints 的文件顺序插入，所以子集合**必须排在
父集合之后**，否则插入直接失败。

`nodes_test.rs` 新增 5 个回归（复用 `db::test_support::init_seeded`）：
两个深度可区分（父集合的孩子含子集合，模块的孩子不含）、
子集合与其接口继承祖先的 `root_id`、扁平 `list_by_root` 仍返回该行但
`parent_id IS NULL` 查询不返回、`subtree_ids` 跨两种边走通 8 节点闭包、
删父集合级联带走子集合与其 `endpoint_details` 行（兄弟节点存活）。

**既有计数断言无需改动**：全仓 grep 确认，fixture 派生的计数只有
`projects_test.rs` 的 3 项目、`environments_test.rs` 的 3 环境、
`base_url_defs_test.rs` 的 1 个 def，都不受新增集合/接口影响。
fixtures 仍是 **test-only**（`db::init` 不播种），已复核确认。

### 验收

```bash
npm run typecheck
npm test
npm run lint
cargo test --workspace
```

树 UI 的以下行为**没有自动化覆盖**，需手测一次（HTML5 DnD 无法用脚本验证）：
嵌套集合的展开/折叠、搜索过滤时父级保留、拖拽的 above / into / below 三态、
跨模块移动子树时 `root_id` 的级联重派生。

> 2026-09-29 追加：`root_id` 的级联已有 Rust 侧回归覆盖
> （`project_ops/mod_test.rs::module_move_preserves_collections_and_endpoints`
> 断言整个子树的 `root_id` 被重新派生），但**浏览器里的拖放分发**仍无法自动验证。

---

## D2 AGENTS.md 引用了已删除的文件路径 ✅ 已完成

**类型：文档** ｜ **优先级：P0**

### 现状

`web_api.rs` 已在提交 `755c8e3` 拆分为 `web_api/mod.rs` 模块树，该文件**不再存在**。

> **本条原始记载有两处不准，以实测为准：**
> 1. 失效引用不是 4 处而是 **6 处**——原列表漏了「Development Commands」里
>    `dump-openapi` 那行的行尾注释，以及「Architecture」CRUD 数据流那行。
> 2. 原文建议「全部改为 `web_api/mod.rs`」，这对 `public_route_specs()` 是**错的**：
>    该函数定义在 **`web_api/spec.rs`**，由 `mod.rs` re-export。
>    照抄会把一条失效路径换成另一条。
>
> 实际修复：CRUD 数据流、`Key Directories`、`dump-openapi` 注释、
> Dual-transport、OpenAPI、Important Files 共 6 处；
> `docs/http-api-reference.md` 3 处。其中三处不是纯路径替换：
> - Dual-transport 改指 `web_api/` 并**补上注册与实现的分离**（`.route()`
>   在 `mod.rs::build_router`，handler 在 `web_api/routes/<resource>.rs`），
>   否则「五处协同编辑」的契约在新结构下不可执行。
> - `http-api-reference.md` 的标题从「拆 `web_api.rs` 时」改为
>   「**重构** `web_api/` 路由时」——目录已经拆过，说「拆目录」讲不通。
> - OpenAPI 段尾的「splitting `web_api.rs`」改为「refactoring the `web_api/`
>   route tree」——那个护栏本来就管的是任何路由树重构。

### 影响

AGENTS.md 是 agent 与新贡献者的行为契约。路径失效会让后续改动**找不到文件**，
或误以为 `web_api.rs` 仍然存在而重新创建它。

### 验收（原验收标准不可达，已修正；后又因 `docs/` 删除而彻底作废）

```bash
grep -rn "web_api\.rs" --include="*.md" .
```

**当时**这条命令不可能归零且不应强行归零：`docs/REFACTOR-PLAN.md` 按 D4 的要求
原样保留为历史存档，它按定义就要提到当年被拆的那个文件；其中还有一句技术上
**必须**保留——`mod web_api;` 同时可解析到 `web_api.rs` 或 `web_api/mod.rs`，
这正是拆分不需要改动 `lib.rs` / `main.rs` 的原因。改掉它就是把一段正确的
Rust 模块解析说明写错。

**现在**该顾虑已不存在：`docs/` 整个被删除，`REFACTOR-PLAN.md` 与
`http-api-reference.md` 都不在了，命令会真正归零。`AGENTS.md` 的 6 处失效
路径在删除前已全部修正，那部分工作并未白做——只是它保护的文档已经没了。

留在树里的 `web_api/` 引用一律准确：注册在 `web_api/mod.rs::build_router`，
handler 在 `web_api/routes/<resource>.rs`，`public_route_specs()` 在
`web_api/spec.rs`。

---

## D3 README 自相矛盾（已删除的 npm 包） ✅ 已完成

**类型：文档** ｜ **优先级：P0**

### 现状

提交 `fba0f08 refactor(distribution)!: drop the npm wrapper` 删除了整个
`packages/simple-post-mcp/`（含 package.json、postinstall、bin 启动脚本）以及
`.github/workflows/release-mcp.yml`。**已复核：两个路径均不存在。**

README 开头仍写：

> 四种分发形态均可运行：…… 以及独立的 `simple-post-mcp` **npm 包**。

而同一份 README 的路线图一节又写：

> 不发布到 npm，AI 客户端也可直连 daemon 的 HTTP 端点。

**同一文件内两处说法相反。**

### 修复

开头改为「独立的 `simple-post-mcp` **二进制（不发布到 npm 包）**」。

**没有**顺手删掉 README 里其余的 `npm install` / `npm run …`——那些是
Tauri 应用自身的 dev 工作流（`package.json` 脚本），完全正确。
错的只有「存在一个 npm **发布**的 `simple-post-mcp` 包」这一条断言；
把两者混为一谈会误删整条上手流程。

### 验收

```bash
grep -rn "packages/simple-post-mcp" --include="*.md" .   # 仅本文档提及
```

---

## D4 REFACTOR-PLAN.md 状态过期 ✅ 已完成

**类型：文档** ｜ **优先级：P1**

### 现状

`REFACTOR-PLAN.md` 开头标注：

> 状态：**规划中，尚未执行。**

但它规划的两项拆分**均已完成**（`git show --stat` 已核对）：

- `755c8e3 refactor(web): split web_api.rs into a web_api/ module tree`
- `1c387de refactor(tree): split ProjectTree.tsx into rows/ hooks/ and shared types`

目标结构里的 `tree-types.ts`、`tree-utils.ts`、`hooks/*`、`rows/*`、
`web_api/spec.rs`、`web_api/routes/*` 全部已落地。

### 修复

取**归档**方案：头部标注「已完成」并附两个提交号，同时写明「正文按原样保留——
它是**为什么这么拆**的决策记录，不是当前状态的描述；再次改动这些目录前先读它」。
正文**逐字未改**，diff 只触及开头数行。

### 备注（修复时已保留，随后随文件删除）

该文档的 A 节有一处**仍然有效**的约束，修复后逐字复核仍在位：
> 搬迁**只做移动**，不要同时改逻辑。逻辑改动单独开 PR。

> 2026-09-29 终局补记：**`REFACTOR-PLAN.md` 已随 `docs/` 整体删除**，
> 本条连同它的归档决定一起成为历史。上面的「备注」之所以值得抄进这里，
> 是因为这条约束与文件是否还在无关——它约束的是**将来**任何一次目录搬迁。
> 同理，D2 的验收标准曾因「必须保留历史指代」而无法归零，那个顾虑现在也
> 随文件一起消失了。

---

## D5 `scripts/e2e/` 完全未记载

**类型：文档 + 工程** ｜ **优先级：P2**

### 现状

存在 `package.json` 脚本 `e2e:preflight`，以及三个未在任何文档中出现的文件：

```
scripts/e2e/preflight.mjs           # 二进制产物探测 + 可用性预检
scripts/e2e/artifact-resolver.mjs   # 产物路径解析（debug / release）
scripts/e2e/process-lifecycle.mjs   # 进程生命周期管理
```

README、AGENTS.md 均未提及，CI 也未执行。

### 目标

- 在 README 的命令表中补 `npm run e2e:preflight`，说明它做什么、什么时候需要先跑
- 在 AGENTS.md 的测试表中补一行
- 决定它是否应该进 CI（注意：它探测二进制产物，而 CI 的 rust job 只跑 clippy/test，不产出可执行文件，
  所以**大概率不该直接进 CI**，需要确认设计意图）

---

## D6 README MCP 工具清单漏列 `endpoint_list` ✅ 已完成

**类型：文档** ｜ **优先级：P2**

### 现状

README 的权限矩阵清单中，Read 一行缺少 `endpoint_list`。
该工具确实已注册，`REQUIRED_CAPABILITY` 中有对应行且为 `Capability::Read`。

实测 `Capability::Read` 共 **16 行**；README 用 `api_resolve_*` 通配覆盖其中 3 个，
所以它的枚举方式与源码的行数不是一一对应——补 `endpoint_list` 时没有顺手把
通配展开，保持了原有简写风格。

总数 **44 是正确的**（由 `every_registered_tool_has_a_capability` 测试守护），
本次**未改动** 44，仅补枚举。

### 修复

Read 行在 `endpoint_get` 之前插入 `endpoint_list`（与源码顺序一致）。

### 验收

`REQUIRED_CAPABILITY` 的条目数应与 README 声称的 44 一致，且枚举无遗漏。
权威来源始终是 `crates/simple-post-mcp/src/mcp/mod.rs`，README 只是摘要。

---

## D7 README 宣称了根本不存在的 MCP Resources ✅ 已完成（已删除该节）

**类型：文档** ｜ **优先级：P1** ｜ **发现于 2026-09-29（D1 收尾时）**

### 现状

README 有一节列出 4 个 MCP Resource：

> Resources（AI 侧"读取"通道，HTTP MCP 模式下暴露）：
>
> - `simple-post://projects`
> - `simple-post://projects/{id}`
> - `simple-post://endpoints/{id}`
> - `simple-post://environments/{projectId}`

**这些 resource 从未实现。** 全仓 grep `simple-post://` 只命中 README 自身；
`crates/simple-post-mcp/` 里没有任何 `resources/list` / `resources/read` 处理、
没有 `uri_template`、也没有任何 scheme 字面量。MCP 侧只有 44 个 **tools**。

### 影响

比 D6（漏列一个已注册工具）更严重：D6 的方向是「有但没写全」，
这一条是「写了但根本没有」。读者会以为存在一条只读通道，
并据此设计 AI 集成——而接上去会发现 `resources/list` 无人应答。

### 修复

**已删除该节**（推荐方案）。tools 已经覆盖全部能力，Resources 是早期设计的残留叙述。
删除前复核了上下文：相邻段落引用的 `docs/openapi.yaml` 与
`docs/http-api-reference.md` 本身自洽，Resources 只是一个普通段落而非标题，
README 也没有目录，删除后没有留下悬空引用或标题层级断层。

### 知识留存

若将来真的实现 Resources，`simple-post://endpoints/{id}` 这个 URI 用的是
**nodes 重构之前**的命名（那时资源按 modules / collections / endpoints 分表）。
届时应统一改为 node 语义，否则又是一次文档与实现分叉。**本条已结项，
但这条知识不要随本节的删除一起丢掉。**

### 验收

```bash
grep -rn "simple-post://" --include="*.md" .   # 仅本文档提及
```

---

## D8 AGENTS.md 自相矛盾且计数失准的 Rust 测试布局 ✅ 已完成

**类型：文档** ｜ **优先级：P2** ｜ **发现于 2026-09-29（D1 收尾时）**

### 现状

同一份 AGENTS.md 对 Rust 测试布局给了两个互斥的说法：

| 位置 | 说法 |
|---|---|
| 「Naming」约定段 | 「Rust tests **inline** as `#[cfg(test)] mod tests`」 |
| 「Testing & QA」段 | 「There is **no** `__tests__` equivalent… 本仓对**全部 22 个**都用 sibling `#[path]` 形式」 |

实测（`src-tauri` + `crates` 全量 `.rs`）：

```
inline  `mod tests {`  : 3
sibling `mod tests;`   : 21
```

即少数派才是内联形式，而「Naming」段恰好把少数派写成了通例；
「22」这个数字也已失准（现为 21）。

### 影响

新 agent 读「Naming」段会以为该写内联 `#[cfg(test)] mod tests { … }`，
而本仓 21/24 走的是 `#[path]` 兄弟文件；两者在 `db/mod.rs` 里还有
**实质差别**——那里 `pub mod test_counter` 排在 `mod tests` **之后**，
按「内联=文件末尾」的直觉切分会把这个 test-only helper 编译进库。

### 修复

- 「Naming」段改为指向 `#[path]` 兄弟文件形式，并注明内联只是少数派残留、
  详见 Testing & QA——不再把少数派写成通例。
- 「Testing & QA」段把「全部 22 个」换成一个**可复核的具名清单**而非数字：
  「除少数仍内联在 `src-tauri/` 下的 `auth.rs` / `daemon_supervisor.rs` /
  `port_registry.rs` 外全部使用兄弟形式」。删掉任一测试文件都不会让这句腐烂。
- `db/mod.rs` 的 `test_counter` 警告经复核**仍然准确**，逐字保留。

### 验收

AGENTS.md 中不再出现 "tests inline"，且两段说法一致。

---

# P1 / P2 — 结构性热点

> 以下不是 bug，是改动风险已经超过收益的大文件。拆分方案另写文档，
> 做法与护栏沿用原先 `REFACTOR-PLAN.md` 的思路（该文档已随 `docs/` 一并删除）：
> **先让文档可信，再动代码**；搬迁只做移动，逻辑改动单独开 PR。
> 排在文档类之后——**先让文档可信，再动代码。**

## R1 `commands.rs` 过大（已拆分）✅

**类型：重构** ｜ **优先级：P1** ｜ **完成于 2026-09-29**

> 原文记载「1196 行 / 50 个 handler」，**两个数字都失准**：动手前实测为
> **1406 行 / 41 个 `#[tauri::command]`**。行数与 handler 数都被高估。

`web_api.rs` 拆分完成后遗留的最后一个同量级文件。它特殊的地方在于：它同时是
**双通道不变量五处编辑里的两处**（Rust handler + `lib.rs` 注册），
拆分时必须保证两者的对应关系不被破坏。

**做法**：`commands.rs` → `src-tauri/src/commands/`，14 个文件
（每个资源一个 + `dto.rs` / `send.rs` / 兄弟测试模块），结构与
`web_api/routes/` 对称。总计 1388 行，最大单文件 `nodes.rs` 188 行。

**核心是让 `lib.rs` 一行都不改**：`mod.rs` 再导出整个子树，使 41 条
`crate::commands::<name>` 路径在 `generate_handler!` 中照常解析，
`web_api/routes/nodes.rs` 对 `send_endpoint_by_id` / `EndpointSendArgs`
的跨模块引用也照常解析。漏注册是**静默**失败——能编译，只在桌面
IPC 路径的 `invoke()` 上炸。

### 护栏与实测

| 项 | 结果 |
|---|---|
| command 定义 ↔ `generate_handler!` 注册 | **41 ↔ 41**，双向差集皆空 |
| `git diff src-tauri/src/lib.rs` / `web_api/` | **空**（逐字节与 HEAD 相同） |
| `AppError` 变体 / `with_conn` / `with_tx` / `permissions::get`,`set` 调用直方图 | **完全一致** |
| `node scripts/dump-openapi.mjs` 输出 | **逐字节不变**（6836 B，sha256 `9b8da161d11fe8ea`） |
| `cargo check` / `clippy -D warnings` / `fmt --check` / `test` | 全绿（26 tests） |

### 三个后来人必须知道的坑

1. **`mod.rs` 必须用 glob 再导出，不能用 item list。**
   `#[tauri::command]` 除函数体外还生成 `__cmd__<name>` 与
   `__tauri_command_name_<name>` 两个宏，而 `generate_handler!` 会把
   `crate::commands::<name>` 改写成 `crate::commands::__cmd__<name>(...)`。
   item list 只再导出函数、不带上宏，编译直接失败 **82 × E0433**。
   这条已写进 `AGENTS.md` 的 dual-transport 不变量段。
2. **`http_request` / `endpoint_send` 必须留在 `lib.rs`。**
   上述宏仅对 `pub` 项 `#[macro_export]`，私有 command 不产生 `__cmd__` 宏，
   因而不会与 `generate_handler!` 产生的同名宏冲突（E0255）。
   把它们搬进 `commands/` 反而会让「非 `pub`」这条承重约束失效。
3. **内联测试模块要落成 `#[path]` 兄弟文件**，与本仓 21/24 的主流形式一致。
   切分时**匹配花括号**，别假设闭合括号是末行。

### 顺带修正

拆分使 `commands.rs` 这个路径失效，共 11 处引用需要跟着改
（`daemon_supervisor.rs`、`web_api/dto.rs`、`web_api/routes/{mcp,base_url_defs}.rs`、
`simple-post-core` 两处 doc 注释、`src/lib/` 两处文件头、`AGENTS.md` 四处）。
**这类引用修正必须与拆分同一个提交**——拆开提交会产生一个文档指向
不存在文件的中间状态。

---

## R2 `crud-slice.ts` 过大（已拆分）✅

**类型：重构** ｜ **优先级：P2** ｜ **完成于 2026-09-29**

> 原文记载「1094 行 / 8 个分节」，**两个数字都失准**：实测为
> **902 行 / 6 个分节**（分节为 helpers / project CRUD / resource tree /
> move / environment / baseUrl definitions）。

**做法**：902 行的单文件拆为 `src/stores/slices/` 下 6 个子工厂
+ 99 行 barrel，合计 1054 行，最大单文件 `crud-tree.ts` 253 行。
barrel 继续导出 `createCrudSlice` / `CrudSlice` / `CrudSliceState` /
`CrudSliceActions` / `NewNodeInput`，导出名不变，因此
**`app-store.ts` 与测试文件零改动**。

**为什么没拆成多个独立 Zustand slice**（这是本条最关键的设计判断）：
若拆成独立 slice，那些**有意跨 slice 写 key** 的 action 会被迫改写。
`deleteProject` 写 `openTabs` / `activeTabId` / `activeProjectId` /
`activeEndpointId`；`deleteNode` 写 `openTabs` / `activeTabId` /
`activeEndpointId`；`setEnvironmentActive` 写 `activeEnvironmentId`；
`deleteEnvironment` 写 `activeEnvironmentId` / `endpointDetails`；
`deleteBaseUrlDefinition` 写 `endpointDetails`。
把它们换成 `get().closeTab()` 之类的多次调用，会把**一次原子 `set()` 变成多次**，
改变 IPC 失败回滚路径的语义——那是行为改动，不是搬迁。

顺带把 `CrudSliceActions` 从扁平字面量改为 `extends` 组合：
解析后的成员集不变（20 个 action，无重复无多余），但同名 action 在两个
子工厂里签名不一致时，现在是 `tsc` 报错而非**静默的 last-spread-wins**。

`collectSubtree` **留在 store 侧**，不进 `@/lib/tree-index`：
它只有一个消费者，搬进去只会撑大共享模块的公共面。

### 护栏与实测

| 项 | 结果 |
|---|---|
| 20 个 action 体 | **逐字节一致**（含内联注释） |
| 5 个 helper | 仅差 `export` 关键字 |
| 跨 slice `set()` 调用 | 仍为单次原子调用 |
| `app-store.ts` / `store-types.ts` / `__tests__/**` / `src/lib/**` | **零改动** |
| import 拓扑 | 仍为星型，无环 |
| `npm test` | 188/188（9 个 crud-slice 测试仍经 barrel 入口） |
| `npx tsx scripts/_verify_store.mjs` | 29/29 |

> **Zustand selector 护栏在 R2 实际风险很低**：该文件不导出任何 selector
> 或自定义 hook，4 个 `useActive*` 全在 `app-store.ts`。原护栏是防御性的。

---

## R3 `EnvironmentSection.tsx` 过大（已拆分）✅

**类型：重构** ｜ **优先级：P2** ｜ **完成于 2026-09-29**

> 原文记载「812 行」，实测为 **884 行**（`Get-Content -Line` 口径 812，
> 差异来自 CRLF——该文件含 CRLF 而 `.gitattributes` 规定 LF，统计时需注意口径）。

全仓最大的单组件（第二名 `HttpTester.tsx` 695 行），拆为 205 行壳
+ `rows/` 下 7 个子组件 + `hooks/useEnvDialogs.ts`，照 `project-tree/`
的既有约定：父组件持有全部 `useState`，子组件纯 props，无 `index.ts` barrel。
合计 906 行。

该组件本来就是**完全 props-driven、零 `useAppStore` 调用**
（store 访问全在父组件 `SettingsPanel.tsx`），所以拆分不涉及 store selector；
文件内**无 CodeMirror 实例**，不存在「两个 editor 挂进同一棵树」的风险。
`SettingsPanel.tsx` **零改动**。

### 顺带收敛（已完成）

`InlineNameCell`（~81 行）与既有 `@/components/ui/InlineEdit` 功能重叠。
**审核结论：暂不收敛，原样搬走。** 三条理由：

1. `InlineEdit` **没有 `validate` 也不渲染内联错误**，而 `VariableRow`
   依赖这两者来阻止非法变量名（`VAR_KEY_REGEX`）提交——换过去会静默丢掉这个守卫
2. `InlineEdit` 是**双击**打开，`InlineNameCell` 是单击
3. `InlineEdit` 的裸 `<input>` 只有 `focus:outline-none`，而仓库的 `Input`
   带 `focus-visible:ring-2` 焦点环——换过去还会额外丢掉 a11y 焦点环

三条行为差异 + 零自动化覆盖，与「纯搬迁」的定位不符。
**若将来要收敛，应先给 `InlineEdit` 加 `validate` prop，再单开一条。**

### 护栏与实测

| 项 | 结果 |
|---|---|
| `className` 字面量 / `cn()` 参数 / CJK 字符串 | 64 / 22 / 26 条，**多集合逐条一致** |
| 11 个组件 | 各存在且仅存在一次 |
| `SettingsPanel.tsx` | **零改动** |
| 新增 `useAppStore` 调用 | **0**（全部 props-driven） |
| `rules-of-hooks` / `no-unused-vars` | 均配为 error，跑通 |

> ⚠️ **R3 未经运行时验证。** 本仓没有 React 组件测试（无 Vitest/RTL），
> `npm test` 的 188/188 **完全覆盖不到这个文件**。渲染一致性、行内编辑
> 的进入/取消/保存/空值行为、两个删除确认弹窗，仍需手测一次。
> 重点：提交非法变量名（如 `1abc`）确认红色报错出现并阻止提交。

---

# 附：本文档条目的核实方式

所有「现状」描述均经以下方式核实，不是推测：

- 文件存在性与行数：`git ls-files` + 逐文件统计
- 代码行为：直接阅读对应源文件
- 文档与代码的差异：`grep` 交叉比对 + `git log` 确认提交历史
- **未运行**任何构建或测试（`npm run check`、`cargo test --workspace` 等）；
  各项「验收」命令是待执行项，不是已通过的结果
