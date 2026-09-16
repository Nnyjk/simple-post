# Simple Post — UI Polish Plan

> 阶段目标：在切到真实业务（HTTP / SQLite / MCP）前，把 UI 做到功能完整。所有改动只在 `src/` 下，不动 Rust 端。

## 当前状态

✅ 已完成（基线）：
- 顶栏（项目 / 环境 / 搜索 / MCP 状态 / 设置）
- 侧边树（4 级可展开、过滤、method 彩色徽标）
- 请求测试器（Method / URL / Send / Save + 6 Tabs：Params / Headers / Body / Auth / Docs / Tests）
  - CodeMirror 6 JSON 编辑器、Markdown 编辑器
  - Auth 4 种类型（None / Bearer / Basic / API Key）
- 响应区（Pretty / Raw / Headers + 状态 / 耗时 / 大小）
- 设置抽屉（环境变量 / MCP 状态 / 主题占位 / 快捷键）
- Mock 数据 3 项目 / 12 接口 / 3 环境
- 暗色主题 / 完整 TypeScript / 0 console error

🟡 已知缺口（按用户感知价值排序）：

### P0 — 树完全不能 CRUD（最影响日常使用）
1. 「+ 新建」按钮是装饰，不响应
2. 项目 / 模块 / 集合 / 接口 不能重命名、删除、复制
3. 没有右键菜单（Postman/Insomnia 标配）
4. 没有拖拽排序 / 上下移动
5. 删除没有确认

### P1 — 缺少关键生产力工具
6. `Ctrl+K` 命令面板是空壳（无实现）
7. URL / Header / Body 里写 `{{var}}` 没有可视助手
8. 接口没有「复制为 cURL」按钮
9. 响应没有历史记录（每次刷新就丢）
10. 响应不能「另存为示例」

### P2 — 体验打磨
11. 没有欢迎页 / 空集合的引导
12. 主题切换（暗 / 亮 / 跟随系统）只是占位
13. 接口 tags 编辑体验差
14. 设置里只有环境变量，没有「请求超时 / 跟随重定向 / 代理」等
15. 接口为空时没有"新建接口"CTA

### P3 — 范围外（v0.2+ 再做）
- 导入 Postman / OpenAPI
- 集合批量运行
- WebSocket / SSE / GraphQL
- 插件系统
- 工作区 Git 同步

## 拆分与所有权

> 避免文件冲突：所有并行任务不能同时改 `src/stores/app-store.ts`。`store-extensions` 任务先把它扩到位，下游任务只读不改。

### Task 0 — `store-extensions`（owner: coder，必须先完成）
**目标**：扩 `src/stores/app-store.ts` 给所有下游用到的 actions / state。

新增数据字段：
```ts
responseHistory: Record<string, ApiResponse[]>;  // endpointId → 最多 10 条
responseExamples: Record<string, ApiResponse[]>; // endpointId → 用户保存的示例
theme: 'dark' | 'light' | 'system';
requestSettings: {
  timeoutMs: number;          // 默认 30000
  followRedirects: boolean;   // 默认 true
  maxResponseSizeKb: number;  // 默认 5120
};
```

新增 actions：
- 项目：addProject / updateProject / deleteProject
- 模块：addModule / updateModule / deleteModule
- 集合：addCollection / updateCollection / deleteCollection
- 接口：addEndpoint / updateEndpoint / deleteEndpoint / duplicateEndpoint
- 排序：reorderModules / reorderCollections / reorderEndpoints
- 响应：addResponseHistory / addResponseExample / deleteResponseExample
- 主题：setTheme（持久化到 localStorage）
- 请求设置：updateRequestSettings

**验收**：
- `npx tsc --noEmit` 0 错
- 不破坏现有 mock 数据的渲染
- 现有 selector（useActiveProject/Environment/Endpoint）继续返回稳定引用
- 不引入新数组 selector 死循环隐患

---

### Task 1 — `tree-crud`（并行，owner: coder）
**文件**：
- `src/features/project-tree/ProjectTree.tsx`（改）
- `src/features/project-tree/TreeContextMenu.tsx`（新）
- `src/features/project-tree/InlineEdit.tsx`（新）
- `src/features/project-tree/ConfirmDialog.tsx`（新）
- `src/components/ui/dialog.tsx`（新，shadcn 风格）

**功能**：
- 顶栏 + 按钮真实生效：弹出"新建项目/模块/集合/接口"小弹窗（name 输入框 + 父级选择）
- 右键树项：菜单（新建子项 / 重命名 / 复制 / 删除 / 复制 cURL）
- 双击树项：进入 inline 编辑模式
- 删除 / 复制：弹确认框
- 集合支持上下移动按钮（小图标）
- 全部走 store actions（不要本地 state）

**验收**：
- 鼠标操作完整跑通：新建 → 改名 → 复制 → 删除
- 删除有确认、复制后能立刻选中
- `tsc --noEmit` 0 错
- 浏览器 console 0 错

---

### Task 2 — `command-palette`（并行，owner: coder）
**文件**：
- `src/features/command-palette/CommandPalette.tsx`（新）
- `src/features/command-palette/command-registry.ts`（新）
- `src/components/ui/tag-input.tsx`（新）
- `src/features/http-tester/EnvVarPicker.tsx`（新，浮层 popover）
- `src/features/http-tester/HttpTester.tsx`（小改，接 EnvVarPicker 到 URL input）
- `src/features/top-bar/TopBar.tsx`（小改，全局 Ctrl+K → 打开面板、显示 kbd 提示）

**功能**：
- `Ctrl+K`（macOS `Cmd+K`）打开命令面板模态
- 模糊搜索跨项目 / 模块 / 集合 / 接口；命中即跳转
- 内置命令：切换环境、切换主题、打开设置、复制当前 URL、新建接口
- 面板键盘可达（↑↓ 选、Enter 触发、Esc 关）
- URL input 输入 `{{` 自动弹出 env 变量选择 popover
- Tag 输入：chips 样式 + 逗号 / 回车添加、Backspace 删

**验收**：
- Ctrl+K 全局可触发（在 input / textarea 里也能开）
- 命令执行后状态正确切换
- `tsc --noEmit` 0 错

---

### Task 3 — `response-actions`（并行，owner: coder）
**文件**：
- `src/features/response-viewer/ResponseHistory.tsx`（新）
- `src/features/response-viewer/ResponseViewer.tsx`（小改）
- `src/features/http-tester/HttpTester.tsx`（小改：加 Copy as cURL 按钮、加 Save as Example）
- `src/features/http-tester/curl.ts`（新，endpoint → cURL 字符串）
- `src/stores/app-store.ts`（只消费：addResponseHistory / addResponseExample 已在 Task 0）

**功能**：
- 响应区底部加一个"历史"折叠条，点开展示最近 10 条
  - 每条显示 method / url 截断 / 状态码 / 耗时
  - 点击重新填回 HttpTester
- HttpTester header 加：
  - 「Copy as cURL」按钮 → 复制 cURL 到剪贴板
  - 「Save as Example」按钮（响应区出现后才亮起）→ 写入 store.examples
- Docs Tab 在有 example 时展示"示例响应"区域（只读 JSON 高亮）

**验收**：
- 多次 Send 后历史正确累加并按时间倒序
- 复制 cURL 真实可粘贴到终端
- 至少 1 个 mock endpoint 触发 example 保存流程
- `tsc --noEmit` 0 错

---

### Task 4 — `welcome-theme`（并行，owner: coder）
**文件**：
- `src/features/welcome/WelcomeScreen.tsx`（新）
- `src/App.tsx`（小改：根据 activeEndpointId / projects 长度决定渲染 WelcomeScreen 或主界面）
- `src/features/settings/SettingsDrawer.tsx`（小改：接主题切换 / 请求设置）
- `src/components/ui/switch.tsx`（新）
- `src/components/ui/input.tsx`（如需支持 number type，可选）
- `src/styles/globals.css`（小改：完善 light theme 变量）

**功能**：
- 当 `activeProjectId` 下的接口数 = 0，显示 WelcomeScreen：
  - 一句 hero 文案 + 三个大按钮（新建项目 / 打开设置 / 看文档）
  - 不抢主布局，正常壳子
- 主题：dark / light / system 三选一，点击立即生效，写 localStorage
- 设置 → 「请求」 section：超时、跟随重定向、最大响应体积

**验收**：
- 切到 light 主题不出现白底白字
- WelcomeScreen 在 mock 数据下不会被触发（因为有 12 个接口）
- 主题切换 reload 后保留
- `tsc --noEmit` 0 错

---

### Task 5 — `integration-verify`（owner: verifier，verify-as-task）
**依赖**：Task 1、2、3、4 全部 accept

**跑**：
1. `npx tsc --noEmit` 必须 0 错
2. `npm run build` 必须成功（产物 < 1MB gzipped）
3. 起 dev server（已有，跑在 5173 端口），用 `browser` 工具跑下面流程：
   - 加载首页、截图
   - 检查 console：error / warning 必须是 0
   - 点击树里任意一个接口，确认 HttpTester 切换
   - 触发 Send（mock 响应），确认响应区出现
   - 打开设置抽屉
   - 触发 Ctrl+K
   - 触发右键菜单
   - 切换主题

**输出**：PASS / FAIL + 任何未跑通的步骤的修复建议

---

## 全局约束（每个 worker 都要遵守）

- **Zustand selector 硬约束**：selector 只能返回 state 里的原始引用（`s.x` 或 `s.x.find()`），所有 `.filter / .map / .sort` 必须在组件内 `useMemo`。参考 `app-store.ts` 顶部注释。
- **样式**：用现有 `src/components/ui/*` 的 shadcn 风格变量（`border / bg-card / text-muted-foreground` 等），不要写裸色。
- **图标**：用 `lucide-react`。
- **类型**：TypeScript strict，全过 `tsc --noEmit`。
- **改动文件**严格控制在自己 ownership 列表内。需要改其他文件先在 deliverable 里说。
- 完成后在 deliverable 里列出：改了什么 / 怎么验证的 / 任何未跑通的项。
