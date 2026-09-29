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
| [D1](#d1-统一树结构的描述) | 统一树结构的描述 | 文档 + 代码 | P0 | 待办 |
| [D2](#d2-agentsmd-引用了已删除的文件路径) | AGENTS.md 引用已删除路径 | 文档 | P0 | 待办 |
| [D3](#d3-readme-自相矛盾已删除的-npm-包) | README 自相矛盾 | 文档 | P0 | 待办 |
| [D4](#d4-refactor-planmd-状态过期) | REFACTOR-PLAN 状态过期 | 文档 | P1 | 待办 |
| [D5](#d5-scriptse2e-完全未记载) | `scripts/e2e/` 未记载 | 文档 + 工程 | P2 | 待办 |
| [D6](#d6-readme-mcp-工具清单漏列-endpoint_list) | 工具清单漏列 | 文档 | P2 | 待办 |
| [R1](#r1-commandsrs1196-行--50-个-handler) | `commands.rs` 过大 | 重构 | P1 | 待办 |
| [R2](#r2-crud-slicets-1094-行) | `crud-slice.ts` 过大 | 重构 | P2 | 待办 |
| [R3](#r3-environmentsectiontsx-812-行) | `EnvironmentSection` 过大 | 重构 | P2 | 待办 |

---

# P0 — 文档与代码脱节

## D1 统一树结构的描述

**类型：文档 + 代码**

### 现状

所有文档都把数据模型描述成固定的四层：

> README「4 级层级：Project → Module → Collection → Endpoint」
> README 侧边树一节同样写「4 级：项目 → 模块 → 集合 → 接口」
> AGENTS.md 的架构描述沿用了同一说法

**但代码实现的是递归树。** `schema.sql` 的 `collections` 表有自引用外键：

```sql
parent_collection_id TEXT REFERENCES collections(id) ON DELETE CASCADE
```

全链路都已实现并生效：

| 层 | 实现位置 | 做法 |
|---|---|---|
| 持久层 | `crates/simple-post-core/src/db/collections.rs` | `list_children` 按 `parent_collection_id IS NULL` 区分顶层 / 子级 |
| 领域层 | `domain.rs` 的 `Collection` / `CollectionPatch` | `parent_collection_id` 与对应的 double_option |
| MCP 层 | `project_ops/collection.rs` | `parent_collection_id`、`new_parent_collection_id` |
| 状态层 | `crud-slice.ts` 的 `indexCollectionChildren` | 按父分桶 + BFS 收集子树 |
| 视图层 | `tree-utils.ts` 的 `buildTree` | `Map<string \| null, Collection[]>` + 递归 `buildCollectionBranch` |
| 组件层 | `rows/CollectionBranch.tsx` | 自递归渲染 |

`useTreeDnd.ts` 里处理「落到非顶层集合时回落到其父级」的那段逻辑，也只有在这条
自引用路径真实存在时才写得通。

### 成因

`crates/simple-post-core/src/db/fixtures.json` 里 **8 个 collection 的
`parentCollectionId` 全部是 `null`**，没有任何嵌套样例。文档作者（含此前的 AI）
是从种子数据推导出「四层」的，而代码早已支持更深。

> 2026-09-29 更新：`fixtures.json` 已降级为 **test-only**（`db::init` 不再播种，
> 全新安装为空库）。所以「四层」的误读如今只来自文档本身——新用户看到的空工作区
> 不再提供任何样例来支撑或反驳这个说法。改文档时不必再考虑演示数据的观感。

### 文档层子任务

- [ ] README「数据层」一节改为「Project → Module → Collection（可递归嵌套）→ Endpoint」
- [ ] README「侧边树」一节同步
- [ ] AGENTS.md 中描述层级的段落同步
- [ ] 在 schema 注释或 README 里点明：**集合层深度不设上限，UI 用递归组件渲染**
- [ ] 明确哪些 UI 入口会暴露嵌套能力（右键菜单的「新建子集合」、拖拽的 into 落点）

### 代码层子任务

#### C1 — 统一「顶层集合」的判定

同一条语义（`parentCollectionId` 为空即顶层）在 8 处以不同写法重复表达：

```
src/features/project-tree/hooks/useEditMenu.tsx     （3 处：顶层筛选、子级筛选、移动目标筛选）
src/features/project-tree/hooks/useInlineMenus.tsx （1 处：新建时选父级）
src/features/project-tree/hooks/useTreeDnd.ts       （1 处：拖拽落点的顶层判定）
src/stores/crud-slice.ts                            （1 处：移动时的同父判定）
crates/simple-post-core/src/db/collections.rs       （1 处：SQL 侧 parent_collection_id IS NULL）
```

**风险**：`=== null` / `!== null` / `!c.parentCollectionId` 三种写法在 filter 里语义不等价
—— 最后一种会把空字符串误判为顶层。

**目标**：在前端提出一个谓词（如 `isTopLevelCollection(c)`）并全部改用它。
db 层保持 SQL 表达，不强行统一。

#### C2 — 合并两处集合分桶实现

`tree-utils.ts` 的 `buildTree` 与 `crud-slice.ts` 的 `indexCollectionChildren`
是**同一套分桶 + 排序逻辑的两次独立实现**，唯一差别是前者先按 `projectId`
过滤模块归属、后者不过滤（它处理全量集合）。

**风险**：改排序键、加环检测、或调整空桶处理时，极容易只改一处——
而这两处喂给完全不同的消费者（视图渲染 vs 移动操作），行为不一致不会立刻暴露。

**目标**：抽出公共函数，项目过滤作为可选参数。

**护栏**：Zustand selector 纪律——抽出后的函数必须是**普通函数**，
在组件内用 `useMemo` 调用，不得包装成返回新数组的 selector。

#### C3 — fixtures 补一个嵌套集合样例

让递归路径在演示数据和测试里都真实存在。

**前置警告**：fixtures 同时是 **Rust 测试基线**。既有断言包括
`projects_test.rs` 的 3 项目 / `environments_test.rs` 的 3 环境 / `mod_test.rs`
的空库播种流程。新增顶层项目或环境会直接打破这些断言。

**建议做法**：在既有 `p_demo` 下新增一个子集合（不动顶层数量），
再补一条 Rust 测试断言递归查询路径，并同步更新 README 的「Mock 数据」计数。

### 验收

```bash
npm run typecheck
npm test
npm run lint
cargo test --workspace
```

树 UI 的以下行为**没有自动化覆盖**，需手测一次（HTML5 DnD 无法用脚本验证）：
嵌套集合的展开/折叠、搜索过滤时父级保留、拖拽的 above / into / below 三态、
跨模块移动子树时 `moduleId` 的级联更新。

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
