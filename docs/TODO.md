# 待办清单

> 本文档记录**已核实**的技术债与待办项。每一项都注明现状、目标、验收标准与护栏。
>
> 约定：
> - **不写行号**——行号在第一次改动后就失效，请按函数名 / 文件名定位。
> - 「现状」段落描述的是本文档创建时的代码事实；若与代码不符，以代码为准并顺手修正本文档。
> - 与 [`REFACTOR-PLAN.md`](REFACTOR-PLAN.md) 的关系：那篇是**拆分方案**（已完成或待执行），
>   本篇是**问题登记簿**。REFACTOR-PLAN 里的拆分执行完毕后，请把对应条目在这里划掉。

## 快速索引

| # | 条目 | 类型 | 优先级 | 状态 |
|---|---|---|---|---|
| [D1](#d1-统一树结构的描述) | 统一树结构的描述 | 文档 + 代码 | P0 | ✅ 已完成（C1/C2/C3 + 文档层全部落地） |
| [D2](#d2-agentsmd-引用了已删除的文件路径) | AGENTS.md 引用已删除路径 | 文档 | P0 | 待办 |
| [D3](#d3-readme-自相矛盾已删除的-npm-包) | README 自相矛盾 | 文档 | P0 | 待办 |
| [D4](#d4-refactor-planmd-状态过期) | REFACTOR-PLAN 状态过期 | 文档 | P1 | 待办 |
| [D5](#d5-scriptse2e-完全未记载) | `scripts/e2e/` 未记载 | 文档 + 工程 | P2 | 待办 |
| [D6](#d6-readme-mcp-工具清单漏列-endpoint_list) | 工具清单漏列 | 文档 | P2 | 待办 |
| [D7](#d7-readme-宣称了根本不存在的-mcp-resources) | README 宣称不存在的 MCP Resources | 文档 | P1 | 待办 |
| [D8](#d8-agentsmd-自相矛盾且计数失准的-rust-测试布局) | AGENTS.md Rust 测试布局自相矛盾 | 文档 | P2 | 待办 |
| [R1](#r1-commandsrs1196-行--50-个-handler) | `commands.rs` 过大 | 重构 | P1 | 待办 |
| [R2](#r2-crud-slicets-1094-行) | `crud-slice.ts` 过大 | 重构 | P2 | 待办 |
| [R3](#r3-environmentsectiontsx-812-行) | `EnvironmentSection` 过大 | 重构 | P2 | 待办 |

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

## D2 AGENTS.md 引用了已删除的文件路径

**类型：文档** ｜ **优先级：P0**

### 现状

`web_api.rs` 已在提交 `755c8e3` 拆分为 `web_api/mod.rs` 模块树，该文件**不再存在**。
AGENTS.md 仍有 4 处引用旧路径：

- 架构图里的 CRUD 数据流（`src-tauri/src/commands.rs` / `web_api.rs`）
- 「Key Directories」列表项 `src-tauri/src/web_api.rs`
- 「OpenAPI」一节：``public_route_specs()` in `src-tauri/src/web_api.rs``
- 「Dual-transport invariant」：改路径的指引 `a route in src-tauri/src/web_api.rs`

### 影响

AGENTS.md 是 agent 与新贡献者的行为契约。路径失效会让后续改动**找不到文件**，
或误以为 `web_api.rs` 仍然存在而重新创建它。

### 目标

全部改为 `src-tauri/src/web_api/mod.rs`（或按语境简写为 `web_api/`）。

### 验收

```bash
grep -rn "web_api\.rs" --include="*.md" .
```

应无结果。`docs/http-api-reference.md` 同样有 3 处引用，一并处理。

---

## D3 README 自相矛盾（已删除的 npm 包）

**类型：文档** ｜ **优先级：P0**

### 现状

提交 `fba0f08 refactor(distribution)!: drop the npm wrapper` 删除了整个
`packages/simple-post-mcp/`（含 package.json、postinstall、bin 启动脚本）以及
`.github/workflows/release-mcp.yml`。

README 开头仍写：

> 四种分发形态均可运行：…… 以及独立的 `simple-post-mcp` **npm 包**。

而同一份 README 的路线图一节又写：

> 不发布到 npm，AI 客户端也可直连 daemon 的 HTTP 端点。

**同一文件内两处说法相反。**

### 目标

开头改为「独立的 `simple-post-mcp` 二进制」，与后文及 AGENTS.md 一致。

### 验收

```bash
grep -rn "npm 包\|npm 包" README.md
grep -rn "packages/simple-post-mcp" .    # 应无结果
```

---

## D4 REFACTOR-PLAN.md 状态过期

**类型：文档** ｜ **优先级：P1**

### 现状

`REFACTOR-PLAN.md` 开头标注：

> 状态：**规划中，尚未执行。**

但它规划的两项拆分**均已完成**：

- `755c8e3 refactor(web): split web_api.rs into a web_api/ module tree`
- `1c387de refactor(tree): split ProjectTree.tsx into rows/ hooks/ and shared types`

目标结构里的 `tree-types.ts`、`tree-utils.ts`、`hooks/*`、`rows/*`、
`web_api/spec.rs`、`web_api/routes/*` 全部已落地。

### 目标

二选一：

1. **归档**：在开头标注「A、B 两节均已于上述提交完成」，保留原文作为决策记录
2. **重写**：删除已完成部分，只保留尚未执行的条目

倾向 1——该文档同时是「为什么这么拆」的历史记录。

### 备注

该文档的 A 节有一处**仍然有效**的约束，值得保留：
> 搬迁**只做移动**，不要同时改逻辑。逻辑改动单独开 PR。

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

## D6 README MCP 工具清单漏列 `endpoint_list`

**类型：文档** ｜ **优先级：P2**

### 现状

README 的权限矩阵清单中，Read 一行列出了 15 个工具，缺少 `endpoint_list`。
该工具确实已注册，`REQUIRED_CAPABILITY` 中有对应行且为 `Capability::Read`。

总数 **44 是正确的**（`REQUIRED_CAPABILITY` 表实为 44 行），仅枚举漏了一行。

### 目标

Read 列表补上 `endpoint_list`。

### 验收

`REQUIRED_CAPABILITY` 的条目数应与 README 声称的 44 一致，且枚举无遗漏。
权威来源始终是 `crates/simple-post-mcp/src/mcp/mod.rs`，README 只是摘要。

---

## D7 README 宣称了根本不存在的 MCP Resources

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

### 目标

二选一，并在 README 里说清：

- **删掉这一节**（推荐）。tools 已经覆盖全部能力，Resources 是早期设计的残留叙述。
- 或者实现它。届时还要同步本文档的「Tools vs Resources」边界。

注意：真要实现的话，`simple-post://endpoints/{id}` 这个名字也该跟着
nodes 重构一起改（见 D1），否则又是一次文档与实现分叉。

### 验收

`grep -rn "simple-post://" --include="*.md" .` 无结果（若选择删除）。

---

## D8 AGENTS.md 自相矛盾且计数失准的 Rust 测试布局

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

### 目标

「Naming」段改为指向 `#[path]` 形式并删掉「inline」表述；
「Testing & QA」段的计数改为不写死数字（或写成「绝大多数」），
避免每次删测试文件都要回来改文档。

### 验收

AGENTS.md 中不再出现 "tests inline"，且两段说法一致。

---

# P1 / P2 — 结构性热点

> 以下不是 bug，是改动风险已经超过收益的大文件。拆分方案另写文档，
> 做法与护栏参考 [`REFACTOR-PLAN.md`](REFACTOR-PLAN.md)。
> 排在文档类之后——**先让文档可信，再动代码。**

## R1 `commands.rs` 1196 行 / 50 个 handler

**类型：重构** ｜ **优先级：P1**

`web_api.rs` 已拆分完成后，这是最后一个同量级的遗留文件。

它特殊的地方在于：它同时是**双通道不变量五处编辑里的两处**（Rust handler +
`lib.rs` 注册）。拆分时必须保证两者的对应关系不被破坏。

已有可利用的分节注释，且 handler 天然按资源分组（projects / modules /
collections / endpoints / environments / base_url_defs / request_settings /
permissions / mcp），拆分风险低于行数给人的直觉。

建议目标结构与 `web_api/routes/` 对称。

**护栏**：与 web_api 拆分一样，跑一次 REST 冒烟测试
（`npm run test:web:auth`、`npm run test:web:basepath`）确认没有漏注册。

## R2 `crud-slice.ts` 1094 行

**类型：重构** ｜ **优先级：P2**

文件内部已有 8 个 `// ---------- xxx ----------` 分节注释（CRUD / move /
environment / baseUrl / helpers），拆分是「顺着缝剪」。

与 [D1-C2](#c2--合并两处集合分桶实现) 有交集——树相关 helper 应当在拆分时
一并收敛，避免重复实现随文件搬家继续存活。

**护栏**：Zustand selector 纪律（见 AGENTS.md）；搬进 hook 时不得新增
返回新数组 / 新对象的 selector。`npx tsx scripts/_verify_store.mjs` 可验证。

## R3 `EnvironmentSection.tsx` 812 行

**类型：重构** ｜ **优先级：P2**

当前最大的单组件。位于设置面板的环境变量编辑区。

---

# 附：本文档条目的核实方式

所有「现状」描述均经以下方式核实，不是推测：

- 文件存在性与行数：`git ls-files` + 逐文件统计
- 代码行为：直接阅读对应源文件
- 文档与代码的差异：`grep` 交叉比对 + `git log` 确认提交历史
- **未运行**任何构建或测试（`npm run check`、`cargo test --workspace` 等）；
  各项「验收」命令是待执行项，不是已通过的结果
