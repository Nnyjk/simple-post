# 07 · 前端（`src/`）

- 适用范围：`src/` 全部渲染层代码（React 19 + Zustand 5 + Tailwind 4 + CodeMirror 6），含分包红线与前端测试约定。
- 权威来源：`src/App.tsx`、`src/main.tsx`、`src/stores/app-store.ts`、`vite.config.ts`、`src/lib/tauri-commands.ts`、各 `features/*/` 模块头注释。
- 最后核对：2026-11-13（v0.1.34）
- 前置阅读：`01-overview.md`、`02-architecture.md`

## 一句话结论

一个模块级 Zustand store 服务三个 lazy surface（欢迎页 / 工作区 / 无项目时的设置宿主），所有后端访问收敛到 `invokeCmd()` 一处双形态分派；前端只消费「原始引用」形式的 selector，派生一律在组件里 `useMemo`，分包红线则是「首屏静态图里不得出现编辑器、`marked`、GSAP」。

## 结构

### 1. 三个 surface 与分包红线

`src/App.tsx` 只有一份路由判断，不是路由库：

| 条件 | 渲染 | 代码 |
|---|---|---|
| `activeProjectId === ''` 且 `activeTabId !== SETTINGS_TAB_ID` | `WelcomeScreen` | 静态 import，首屏块 |
| 同上但 `activeTabId === SETTINGS_TAB_ID` | `SettingsSurface`（`lazy()`） | 无项目时设置的第三宿主 |
| `activeProjectId !== ''` | `WorkspaceShell`（`lazy()`） | 树 + 顶栏 + 标签 + tester |

- 判断放在 `App.tsx` 而非 `WelcomeScreen` 内，是为了两个宿主共用同一份 `SettingsPanel`（`features/settings/SettingsPanel.tsx` 的注释明说「挂载两处，一份实现」）。
- `lazy()` 的收益：workspace chunk 里滚进树、顶栏、tester、设置与 CodeMirror；冷启动只拉 store 与项目选择器。
- 浮层按「使其可见」的状态挂载（`newItem.open && newItem.type === 'project'` 才挂 `CreateProjectDialog`）。**`lazy()` 在 render 瞬间就发 import**：无条件写 `<Suspense><CreateProjectDialog/></Suspense>` 仍会在首屏下载 chunk，只是不阻塞；挂在 `open` 上才能把 GSAP（经集合对话框的 `Select`）推到点击之后。
- 全局常驻：`BootSplashGate`（拆 HTML 启动闪屏）、`BootstrapErrorBanner`（错误才挂）、主题副作用、`Ctrl+Enter`/`Ctrl+S` 监听（跨 surface 存活，靠 `document.querySelector('[data-testid="send-btn"]')` 转发点击，并跳过 `e.isComposing`）、跨 surface 的项目创建对话框。
- `vite.config.ts` 的 `manualChunks` **必须是函数**：Rolldown 直接拒绝对象形式（`Expected Function but received Object`）。只固定 `react` 与 `state` 两个共享 vendor 块；**CodeMirror 与 `marked` 不得列入**——`manualChunks` 会把模块连同依赖闭包提升成块，Rollup 为该块发静态 import，编辑器就回到首屏关键路径。验证方式是读 `dist/assets` 的块图，不是看块名列表。

### 2. store 结构（`src/stores/`）

- 桶文件 `app-store.ts` 组合 **6 个 slice**：`bootstrap-slice` / `crud-slice` / `ui-slice` / `mcp-slice` / `dirty-tabs-slice` / `dynamic-slice`，`AppState` 是六者交集。
- `ui-slice.ts`（37 行）与 `crud-slice.ts`（67 行）自身几乎是空的：实现拆在 `stores/slices/`（`ui-tabs`、`ui-response`、`ui-prefs`、`ui-new-item`、`ui-draft-endpoints`、`crud-project/tree/move/environment/helpers`、`history-keep`），在 slice 文件里组合。拓扑是星形：子工厂只 import `store-types` 与 `AppState` 类型，彼此不互相 import，跨 slice 读写只走 `get()`/`set()`。
- `store-types.ts`：跨 slice 共用的形状与哨兵常量（`SETTINGS_TAB_ID = '__settings__'`、`DRAFT_ID_PREFIX = 'draft:'`、`DYNAMIC_TAB_ID_PREFIX`、`TabKind`、`OpenTab`、`EndpointDraft`）。
- **两层 `bootstrap()`**（`bootstrap-slice.ts`，由 `main.tsx` 在 `createRoot` 前调用，让 IPC 与挂载重叠）：
  - 第 1 层 `await` `projectsList` + 偏好（`requestSettings`/`responseHistoryKeep`，串行而非并发），恢复 `lastProjectId`（项目已删则清记录、留欢迎页），最后 `set({ loaded: true })` 拆闪屏。**刻意不选默认项目**。
  - 第 2 层 `void get().loadWorkspace()`，**不 await**；失败落 `nodesError`，永不 reject。
  - `nodesLoaded` 三态含义：`false + nodesLoading=false` 尚未开始、`false + nodesLoading=true` 加载中、`true` 就绪。消费方必须区分——`nodesLoaded === false` 时的 `nodes: []` 是歧义的，要渲染加载态而非「该项目没有模块」空态（`ProjectTree.tsx:357`、`ui-tabs.ts:260`）。
  - `loadWorkspace()` 是幂等包装（`nodesLoaded` 早退 + 共享 `inFlight` promise，返回**未决** promise 而非立即 resolve），本体在 `reloadWorkspace()`；`inFlight` 存在 slice 闭包而非 state（promise 进 state 会让每次写入都触发订阅）。
- 持久化由 **store 订阅**负责，不在动作里写：`activeProjectId` 变化 → `saveLastProjectId`（连 `deleteProject` 失败回滚 `set(snapshot)` 也覆盖）；`activeEnvironmentId` 变化且两值都非空 → `saveRememberedEnvironmentId(projectId, envId)`（空串是「还没定」哨兵，写了会抹掉记忆）。
- 偏好键单一来源：`simple-post:prefs`（`theme`、`lastProjectId`、`currentEnvByProject`、待迁移的 `requestSettings`）。主题与解析规则只在 `src/lib/theme-boot/index.ts`，`vite.config.ts` 的 `themeBootPlugin` 在构建期把该文件源码编译成 `index.html` 的 `<head>` 内联脚本（`<!--theme-boot-->`），故**该文件运行时不得有 import、只能有一个默认导出**。
- selector 稳定性【硬约束】：Zustand v5 用 `Object.is`。允许返回 state 原始引用或单个 `.find()` 结果；**禁止** `.map/.filter/.sort/展开`。派生数组/对象在消费组件里 `useMemo`。`useActiveEndpoint` 刻意只返回节点，不 join 出 `{node, detail}`。

### 3. features 地图

| 目录 | 职责 | 关键文件 |
|---|---|---|
| `project-tree/` | 左栏资源树：构建/过滤、DnD、行内改名、右键菜单、新建流程 | `ProjectTree.tsx`、`tree-utils.ts`、`rows/TreeRow.tsx`、`hooks/useTreeDnd.ts` |
| `workspace/` | 工作区外壳、标签条、按标签分发内容、未保存关闭对话框 | `WorkspaceShell.tsx`、`WorkspaceTabs.tsx`、`tab-scroll.ts`、`DirtyCloseDialog.tsx` |
| `http-tester/` | 请求编辑器：method/URL、params/headers/body/auth/TLS/脚本、变量提示 | `HttpTester.tsx`、`model/endpointParts.ts`、`useRequestActions.ts`、`*-lazy.tsx` |
| `response-viewer/` | 响应：pretty/headers/binary/script 视图、历史、示例 | `ResponseViewer.tsx`、`responseKind.ts`、`ResponseHistory.tsx` |
| `interchange/` | 导入/导出预览与提交（先预览后落定） | `TransferDialog.tsx`、`useTransfer.ts`、`selection.ts` |
| `settings/` | 单例设置面板四分区 + 无项目宿主 | `SettingsPanel.tsx`、`SettingsSurface.tsx`、`sections/*` |
| `command-palette/` | Ctrl+K 全屏命令面板 | `CommandPalette.tsx`、`command-registry.ts`、`CommandPalette-lazy.tsx` |
| `top-bar/` | 顶栏：项目/环境切换、MCP 状态、窗口控件、启动错误横幅 | `TopBar.tsx`、`BootstrapErrorBanner.tsx` |
| `welcome/` | 欢迎页 + HTML 闪屏拆除 | `WelcomeScreen.tsx`、`BootSplashGate.tsx` |
| `scripts/` | collection 两段脚本对话框（与端点共用 `ScriptPanel`） | `CollectionScriptDialog.tsx`、`scripts-patch.ts` |
| `batch-run/` | 集合/模块批量发送与汇总 | `BatchRunDialog.tsx`、`model/runner.ts`、`model/collect.ts` |
| `create/` | 新建项目/模块/集合对话框（共用外壳；endpoint 无对话框） | `CreateDialog.tsx`、`CreateProjectDialog.tsx` |
| `dynamic-variables/` | 项目内动态变量声明与当前值 | `DynamicVariablesPanel.tsx`、`node-search.ts` |

### 4. 双传输调用

- 门面 `src/lib/tauri-commands.ts`，三块实现：`tauri-commands/commands.ts`（`COMMANDS` 名表，值必须等于 Rust 函数名）、`tauri-commands/web-routes.ts`（`WEB_ROUTES` REST 映射）、`tauri-commands/bearer.ts`（web 形态 token）。
- `invokeCmd()` 按模块级 `IS_TAURI_SHELL`（`window.__TAURI_INTERNALS__ || window.__TAURI__`）分派：Tauri 走 `@tauri-apps/api/core` 的 `invoke`（**丢弃 AbortSignal**），否则查 `WEB_ROUTES` 走 `fetch(readApiBase() + path)`；表里没有的命令直接抛「未在 web 形态暴露」。
- `readApiBase()` 读运行时注入的 `window.__SIMPLE_POST_BASE__`（serve-web 的 `--public-base-path`），**不能用 `import.meta.env.BASE_URL`**（构建期烤死，镜像不传 `--base`）。与 `crates/simple-post-server/.../static_files.rs` 的 `BASE_GLOBAL` 对齐。Vite dev 只代理 `/api` 到 `127.0.0.1:4173`。
- 新增命令要动的 5 处（发现/命名/注册/路由/测试）见 `02-architecture.md`，本笔记不重复。

### 5. UI 约定

- **唯一 modal 实现**：`src/components/ui/modal-surface.tsx`。要点：打开中的层进 `openSurfaces` 栈，Esc/Tab 只交给栈顶（`stopPropagation` 在**捕获阶段**，React `onKeyDown` 从此收不到）；背景 `inert` 用**计数**（`backgroundUsers`），归零才还原；`previousFocusRef` **在 render 期间**记下 `document.activeElement`（进 effect 时已被 `autoFocus` 挪走，焦点还不回去）。`Dialog`、`CommandPalette` 都走它。
- z-index 只从 `src/components/ui/layers.ts` 取：`popover 50` / `contextMenu 60` / `tooltip 70` / `modal 100` / `tooltipInModal 110`。手写 `z-[200]` 等于另立一层。
- 语义色：`--*-foreground` 是**压在饱和填充上的文字色**，不是「同色的浅版」。`--destructive` 深浅两档互换极性（light `5 59% 44%` + 白字；dark `354 100% 80%` + 近黑字），`--primary` 同理。hover/选中用 `bg-overlay-hover`/`bg-surface-hover` 而非 `bg-accent/40`。概念色走 `--hue-*` + `src/lib/hue.ts`（八色，角度固定）。
- 树行三块（`rows/TreeRow.tsx`）：箭头（`w-[16px]` 前导槽，`ChevronRight`）/ 名称方块（`h-[26px]`，悬停淡入边框覆盖层）/ 行尾「+」。**箭头与名称是两个独立按钮**：箭头 → `toggleNodeExpanded`，名称 → `openTab`。回归网 `project-tree/__tests__/tree-toggle-split.test.ts`（读源码断言，见测试节）。
- **标签条 GSAP 单通道**（`workspace/WorkspaceTabs.tsx`）：宽度、位置、激活指示条、横向滚动全部交给 GSAP，混入 CSS 过渡会互相打架（标签宽度刻意没有 `transition-width`）。
  - Flip 记**视口坐标**，故滚动停稳后必须重记（`captureFlipState`，滚轮 120ms 去抖），否则下次增删标签整条先跳一截。
  - 依赖必须带 `isCompact`：normal 160px / compact 92px 也是补间的一部分。
  - 激活指示条挂在条级、逐帧读激活标签实时 `rect`（`x = rect.left - stripRect.left + scrollLeft`），指数逼近 `BAR_APPROACH=0.24`，窗口 `BAR_SETTLE_MS=480`；补间中被打断的那一轮不滚。
  - 滚动目标由纯函数 `tab-scroll.ts::scrollTargetForActiveTab` 算出（三条规则：放得下不滚、已在可视区不滚、夹在 `[0, maxScroll]`、差 <0.5px 不滚），GSAP `to({scrollLeft})` 执行。
  - 批量关闭：`dirty-tabs-slice.ts` 的 `requestClose*` 先过脏守卫。右键菜单的「关闭其他/左/右」以**被右键的标签**为基准，标签条尾部按钮不传参时以**活动标签**为基准。「直接关闭」丢弃当前标签**并中止队列**；DirtyCloseDialog 的 ×/Esc/取消都是「一个都不关」；`closeSavedTabs()` 返回 0 时弹只讲事实的提示 Dialog（同时所有卡片都脏时也算取消）。

### 6. 测试

- 前端测试：`tsx --test` + `node:assert/strict` + `node:test`，**无 Vitest、无 RTL、无 jsdom**。`scripts/run-tests.mjs` 自动收集 `src/**/*.test.ts`（跳过 `node_modules/dist/target/build`），`npm test` 一次跑完；新增测试文件无需登记。
- 测试与源码同目录放 `__tests__/`（如 `src/stores/__tests__/`、`src/features/workspace/__tests__/`），全仓约 102 个 `*.test.ts`。
- 没有 DOM 的应对：把竞态/几何/解析逻辑抽成纯模块再测——`tab-scroll.ts`、`project-tree/drop-resolve.ts`、`interchange/selection.ts`、`scripts/scripts-patch.ts`、`stores/slices/history-keep.ts`。
- 交互接线的兜底是**读源码断言**（`tree-toggle-split.test.ts`、`tree-rename-selection.test.ts`、`tab-strip-animation.test.ts`）：`readFileSync` + 剥注释（否则注释里的字面量会让断言必然通过）+ 取 JSX 片段。这是既定取舍，不是遗漏。

## 不变量

1. `activeProjectId === ''` 是唯一「在欢迎页」的判据；没有第二个 route 状态。
2. `AppState` 只有一个 store 实例，所有 slice 共享 `(set, get, store)`；跨 slice 不 import 对方模块。
3. selector 返回新引用 = 无限重渲染；派生逻辑一律 `useMemo`。
4. `loaded` 与 `nodesLoaded` 不能合并：闪屏只等前者。
5. 所有后端调用经 `invokeCmd()`；命令名只从 `COMMANDS` 取，不写裸字符串。
6. 所有模态走 `ModalSurface`，所有层级走 `LAYER`。
7. 主题解析只在 `theme-boot/index.ts` 一处，且该文件保持「零 import、单默认导出、可编译成 classic script」。
8. 编辑器/`marked`/GSAP 不进首屏静态图；`manualChunks` 永远保持函数形式。

## 陷阱

- `lazy()` 组件一被 render 就开始下载：条件挂在 `open` 上，别写在 `<Suspense>` 里无条件渲染。
- Tauri 2 的 `invoke` 不支持 `AbortSignal`：需要中止的调用点自己包 promise。
- `nodesLoaded === false` 的 `nodes: []` 不能渲染成空态。
- `backgroundUsers` 计数、`openSurfaces` 栈都存在模块作用域：先关的那层不能解锁背景，两层都开时一次 Esc 只关一层。
- 树整份替换会抹掉 `expanded`（服务端不存），必须 `mergeExpanded`。
- 换项目顺序固定：`loadWorkspace()` → `switchProjectTabs(id)` → `setActiveProject(id)` → 清 `activeEndpointId`；顺序反了会丢掉旧项目的标签归属。
- `bootstrap()` 恢复记录发生在 `loaded` 翻真之前，否则欢迎页会闪一帧再变工作区。
- 标签条 Flip 补间中量到的位置是动画中途值：此时既不重记布局，也不算滚动目标。
- 欢迎页 `App.tsx` 里 `Ctrl+S`/`Ctrl+Enter` 只是 click 转发，找不到按钮就静默无事——不要以为是快捷键本身在发请求。

## 相关笔记

- [01-overview.md](./01-overview.md)：产品定位与三 surface 的由来。
- [02-architecture.md](./02-architecture.md)：新增命令的 5 处、Rust/前端边界。
- [03-data-model.md](./03-data-model.md)：store 的形状来自后端镜像表，补丁语义在那边。
- [04-request-pipeline.md](./04-request-pipeline.md)：`prepareRequest` 与变量替换，`http-tester` 只负责编辑。
- [05-mcp.md](./05-mcp.md)：`mcp-slice` 轮询的状态来源。
- [06-dynamic-and-scripts.md](./06-dynamic-and-scripts.md)：`dynamic-slice` 与脚本沙箱的规则。
- [08-interchange-and-parity.md](./08-interchange-and-parity.md)：`interchange/` 前端部分与双形态一致性。
- [09-engineering-process.md](./09-engineering-process.md)：`tsx --test` 管线、CI 与提交约定。
- [10-glossary.md](./10-glossary.md)：surface/slice/标签条等术语。
- [README.md](./README.md)：笔记索引与阅读顺序。

## 文档与实现不一致

1. `src/main.tsx` 注释称 `bootstrap()` 在 `loaded || loading` 时幂等早退；`bootstrap-slice.ts` 实际只判断 `if (get().loaded) return;`，**没有 `loading` 守卫**——并发调用会重复跑第 1 层（`main.tsx` 只调一次，故当前不触发）。以代码为准。
2. `WorkspaceShell.tsx` 头部写「这里刻意不挂载 `ProjectTree`」，正文第 169 行实际是条件挂载 `{activeProjectId !== '' && <ProjectTree />}`。语义应为「没有项目时不挂载」，注释措辞与代码字面冲突。
