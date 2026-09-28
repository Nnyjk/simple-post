# 重构规划：两个巨型文件拆解

> 制定于 2026-09-28 · 基于实测行数，非估算
> **本文件只是规划，本轮没有执行任何拆分。** 下面的行数来自实际测量。

## 为什么要拆

两个文件已经大到"改动风险 > 收益"的程度：review 时无法一眼看出影响面，
新人不敢动，agent 改代码容易踩到无关区域。

| 文件 | 行数 | 问题 |
|---|---|---|
| `src/features/project-tree/ProjectTree.tsx` | 1870 | 单个组件函数占 966 行 |
| `src-tauri/src/web_api.rs` | 1960 | 58 个 handler + router + OpenAPI 声明混在一起 |

两者都有好的消息：**内部已经有清晰的分节注释**，拆解是"顺着缝剪"，不是"重新设计"。

---

## A. `ProjectTree.tsx`（1870 行）

### 现状结构（实测）

```
  38- 68   类型：TargetKind / DeleteTarget / DragPayload
  70-110   纯函数：encodeDrag / decodeDrag
 114-1080  ProjectTree()  ← 966 行的单函数
      115-123   raw state（~19 个 store 引用）
      124-142   store actions
      143-165   click handlers
      166-170   new-item modal
      171-197   local UI state
      198-222   derived
      223-245   new-item flow
      246-261   click / selection
      262-349   inline "+" 菜单构造
      350-601   edit / context menu / delete   ← 252 行
      602-910   drag-and-drop helpers          ← 309 行
      911-1080  render                          ← 170 行
 1085-1721  7 个行组件（ProjectRow / ModuleRow / TreeChildren /
            CollectionBranch / CollectionRow / EndpointRow / DropHintOverlay）
 1722-1870  纯函数：buildTree / filterTree / count*
```

好消息：行组件**已经是独立函数**，纯 helper 也在底部。真正的问题集中在
`ProjectTree()` 那 966 行，尤其 edit/menu（252）和 DnD（309）两块。

### 目标结构

```
src/features/project-tree/
  ProjectTree.tsx            ~200   容器：state 接线 + 渲染组合
  tree-types.ts               ~40   TargetKind / DragPayload / DropZone / TreeBranch
  tree-utils.ts              ~120   encodeDrag / decodeDrag / buildTree / filterTree / count*
  hooks/useTreeDnd.ts        ~310   ← 从 602-910 搬出
  hooks/useEditMenu.ts       ~250   ← 从 350-601 搬出
  hooks/useInlineMenus.ts    ~90    ← 从 262-349 搬出
  hooks/useNewItemFlow.ts    ~60    ← 从 223-245 搬出
  hooks/useTreeSelection.ts  ~60    ← 从 143-165 + 246-261 搬出
  rows/ProjectRow.tsx        ~85    ← 1085-1166
  rows/ModuleRow.tsx        ~115    ← 1186-1299
  rows/TreeChildren.tsx      ~55    ← 1300-1354
  rows/CollectionBranch.tsx ~115    ← 1379-1491
  rows/CollectionRow.tsx     ~90    ← 1510-1599
  rows/EndpointRow.tsx       ~90    ← 1619-1708
  rows/DropHintOverlay.tsx   ~15    ← 1709-1721
```

同目录已有 `NewItemModal.tsx` / `TreeContextMenu.tsx` / `ConfirmDialog.tsx`，命名风格照抄即可。

### 执行顺序（每步独立可提交、保持绿）

| # | 动作 | 风险 | 说明 |
|---|---|---|---|
| 1 | 抽 `tree-types.ts` + `tree-utils.ts` | **极低** | 纯类型 + 纯函数，不碰 React，tsc 能全量验证 |
| 2 | 抽 `rows/*` 7 个组件 | **低** | 本来就是独立函数，只是换文件 |
| 3 | 抽 `useTreeDnd` | **中** | 最大收益；`DragContextValue`(1734) 要一起搬 |
| 4 | 抽 `useEditMenu` | **中** | 第二大块 |
| 5 | 抽 `useInlineMenus` | 低 | |
| 6 | 抽 `useNewItemFlow` + `useTreeSelection` | 低 | |
| 7 | `ProjectTree.tsx` 瘦身为组合层 | 低 | 此时文件已经很薄，改动接近纯移动 |

### 必须遵守的约束

- **`App.tsx:3` 是唯一外部 import**（`import { ProjectTree }`），保持具名导出即可，外部零改动。
- **`DragContextValue`(1734) 与 DnD 代码同进同出**，否则会留下悬空 context。
- **Zustand selector 纪律**（AGENTS.md 已标为无限重渲染风险）：搬进 hook 时
  **不得**新增返回新数组/新对象的 selector。现有代码已经是 `.find()` 形态，
  搬迁时不要"顺手优化"成 `.filter()`。
- **hooks 必须在 `if (!endpoint)` 之类早退之上**（`rules-of-hooks` 是 error 级）。
- 搬迁**只做移动**，不要同时改逻辑。逻辑改动单独开 PR。

### 每步验证

```
npm run typecheck
npm test                      # 161 例，其中 crud/bootstrap slice 覆盖 store
npm run lint
```
第 3 步（DnD）之后追加**手动**验证：拖拽重排 module/collection/endpoint、
跨层级 drop 的 above/into/below 三态、拖拽时的 drop hint 指示。
这块没有自动化测试覆盖，只能手测——**主会话统一做一次**。

---

## B. `web_api.rs`（1960 行）

### 现状结构（实测）

```
    1- 424  前置：WebState / HttpErrorResponse / map_app_error / DTO 结构体
  425-1206  handler 区，按资源分节（共 58 个 sync fn）
      569- 638  modules        ( 69 行,  5 handler)
      638- 705  collections    ( 67 行,  5 handler)
      705- 831  endpoints      (126 行,  8 handler)
      831- 916  environments   ( 85 行,  7 handler)
      916-1000  baseUrlDefs    ( 84 行,  5 handler)
     1000-1035  request-settings( 35 行,  2 handler)
     1035-1068  permissions    ( 33 行,  2 handler)
     1068-1207  mcp daemon     (139 行,  5 handler)
  1266-1385  build_router（链式 .route 调用）
  1934-1960  RouteSpec + public_route_specs()（OpenAPI 单一真相源）
```

### 目标结构

Rust 侧天然适合按资源拆——分节注释已经就位：

```
src-tauri/src/web_api/
  mod.rs                    ~140   WebState / build_router / auth 中间件 / re-export
  error.rs                   ~60   HttpErrorResponse / map_app_error
  dto.rs                    ~320   全部请求体 / 响应体结构体
  spec.rs                   ~330   RouteSpec + public_route_specs()
  routes/projects.rs        ~120
  routes/modules.rs         ~140
  routes/collections.rs     ~140
  routes/endpoints.rs       ~210
  routes/environments.rs    ~160
  routes/base_url_defs.rs   ~170
  routes/request_settings.rs ~100
  routes/permissions.rs      ~100
  routes/mcp.rs             ~220
```

`web_api.rs` → `web_api/mod.rs`：Rust 允许 `mod web_api;` 同时解析到
`web_api.rs` 或 `web_api/mod.rs`，所以 `lib.rs` / `main.rs` 的 `mod web_api;`
**不需要改**。

### 执行顺序

| # | 动作 | 风险 | 理由 |
|---|---|---|---|
| 1 | 抽 `spec.rs` | **极低** | 纯数据，无副作用；且天然有回归网（见下） |
| 2 | 抽 `error.rs` + `dto.rs` | 低 | 纯类型 |
| 3 | 按资源逐个抽 `routes/*` | 中 | 一次一个资源，每次带自己的 `.route()` 片段 |
| 4 | `mod.rs` 只留 `build_router` 组合片段 | 低 | |

**第 3 步是主体**。建议顺序按"独立性递增"：permissions → request_settings →
baseUrlDefs → modules → collections → environments → endpoints → mcp → projects。
`mcp` 放最后是因为它最长（139 行）且耦合最多。

### 关键风险与对策

| 风险 | 对策 |
|---|---|
| `WebState` 被全部 handler 引用 | 放 `mod.rs` 并 `pub(crate) re-export`，各 `routes/*` 用 `super::WebState` |
| 反代 base path 嵌套 | `build_router` 留在 `mod.rs`，auth 中间件同样留在 `mod.rs`，只搬 handler 与 route 片段 |
| axum 0.7 路径参数是 `:id` 不是 `{id}` | 搬迁时不要"修正"成 OpenAPI 风格 |
| 忘记某个 `.route()` | **`public_route_specs()` 就是回归网**——见下 |

### ⭐ 最强的验证手段

`docs/openapi.yaml` 由 `public_route_specs()` 生成，所以：

```bash
node scripts/dump-openapi.mjs > /tmp/before.yaml   # 拆之前
# ... 拆分 ...
node scripts/dump-openapi.mjs > /tmp/after.yaml    # 拆之后
diff /tmp/before.yaml /tmp/after.yaml             # 必须为空
```

**只要这个 diff 为空，就证明没有丢路由、没有改 method、没有改路径。**
这是本次重构最可靠的护栏，建议每一步都跑。

配套跑：

```bash
npm run check                  # 全门禁
npm run test:web:auth          # bearer 鉴权流程（需要已编译 binary）
npm run test:web:basepath      # 反代 base path（需要 binary）
npm run perf:bench             # 确认没引入性能回退
```

---

## 执行顺序建议

两个文件**互不相干**，可以完全并行；建议按此优先级：

1. **先做 `web_api.rs`**。理由：它有 openapi.yaml diff 这个确定性护栏，
   验证成本低、结论明确。`ProjectTree.tsx` 的 DnD 部分**没有自动化测试**，
   风险更高，放在后面。
2. 每个文件内部严格按上表顺序，**一步一个 commit**。
3. 全程不 push，等主会话统一 review。

## 明确不做的事

- 不改任何行为、不改数据库 schema、不改 REST 契约（契约变更走 openapi diff 暴露）
- 不在拆分 PR 里顺手重构逻辑
- 不动 `NewItemModal.tsx` / `TreeContextMenu.tsx`（已是独立文件）
- 不引入新依赖（虚拟滚动库、router 宏等）——**另开议题**。真要虚拟滚动，
  `ProjectTree` 拆完之后再评估，那时才知道渲染瓶颈到底在哪。


