# 贡献指南

面向人类贡献者。`AGENTS.md` 是给 agent 的运行时契约（不变量、陷阱、文件地图），本文只讲人怎么提 issue、怎么改代码、怎么发版。凡是本文列为「红线」的条目，都能在 `AGENTS.md` 找到出处——本文不新增任何未经证实的约束。

## 本地门禁

一条命令覆盖全部本地检查，推 PR 前跑它：

```bash
npm run check
```

它依次执行 `format:check` → `lint` → `typecheck` → `npm test` → `lint:openapi` → `fmt:check` → `clippy`。CI 的 Web 与 Rust 两个 job 跑的检查都在这条命令里，此外 CI 还多跑三样本地不跑的：`vite build` 生产构建、`cargo test --workspace` Rust 测试、容器内的快速验证（`docker build` 与 `docker compose up` 可在本地覆盖后两样的主要部分）。

需要 Rust 侧完整测试时单独跑 `cargo test --workspace`。

## 分支与 PR

- **不要直接推 `main`。** 受保护分支会拒绝（`GH006`）。开分支、提 PR。
- 分支命名随意，`feat/…`、`fix/…`、`docs/…` 都可以。
- PR 描述里写清**改了什么**和**怎么验证的**。没跑的测试就写「未执行」，不要写成通过。
- 合并前 CI 必须全绿，且三个必需检查必须出现：
  - `Web — format, lint, typecheck, test, build`
  - `Rust — fmt, clippy, test`
  - `Docker — image build + container smoke test`

## 红线

这些不是风格偏好，改错了会静默损坏数据或让工具永久失效。**动到相关文件前先读 `AGENTS.md` 对应段落。**

### 1. 改 schema 前先想清楚要不要 bump `SCHEMA_VERSION`

`ensure_current_schema` 在版本号变化时会**删掉每个用户的整个 store.db**。新增字段一律走 `db::schema::ADDED_COLUMNS`（加法式），`db::migrate` 负责把新列补进既有文件——**纯增量改动不 bump 版本号**，正是这个原因。

反过来，**表达不成「加一个带默认值的可空列」的改动就是一次 bump**，而 bump 会删掉用户的项目、环境与资源树。删列等于重建表，等于 bump；删一张没人读的表不用 bump——那种表在打开时丢掉即可。

出处：`AGENTS.md` → *`SCHEMA_VERSION` is 4 and a version bump replaces the store*。

### 2. MCP 双闸 fail-closed

任何 AI 客户端要写东西，必须同时过两道闸：全局能力闸（用户在设置里选的 `read_only` / `safe_write` / `full_access`）与项目权限闸（`permissions` 表）。

- **新增工具要改两处注册表**：`tool_table()` 和 `REQUIRED_CAPABILITY`。只改前者，工具会绕过能力闸——要么被当成未分类而彻底不可用，要么（改动未生效时）绕过用户的 `read_only` 选择。`every_registered_tool_has_a_capability` 与它的反向测试会拦住你。
- **`check_permission(..).await.ok()` 会把拒绝吞掉。** 它编译通过、扫一眼也过得去，然后 `PermissionDenied` 就没了，闸门等于不存在。永远用 `?` 传播。
- **别写死项目 id。** 处理器必须解析真实归属项目再传进去（`db::nodes::root_id_of` / `db::environments::lookup_project_id`）。传字面量 `"global"` 会查不到行、回落到默认的 `ReadCall`，于是所有需要 `Full` 的写工具被永久拒绝——**而工具看起来仍然注册正常**。
- `Send`（出站 HTTP）只有 `full_access` 覆盖。「允许写」不等于「允许随便发请求」。

出处：`AGENTS.md` → *MCP permissions — two gates*、*Adding an MCP tool requires TWO registry edits*。

### 3. dual-transport 五处协同

新增一个后端命令要改**五个地方**，少改一处会让桌面端或 Web 端**静默**坏掉：

1. `src-tauri/src/commands/<resource>.rs` 里的 handler
2. `src-tauri/src/lib.rs` 里的注册
3. `src/lib/tauri-commands.ts` 的 `COMMANDS` 常量
4. `crates/simple-post-server/src/web_api/` 的路由（`.route()` 注册在 `web_api/router.rs::build_router`，handler 在 `web_api/routes/<resource>.rs`）
5. `WEB_ROUTES` 条目（如果浏览器侧要用）

两侧都需要的 DTO 放 `crates/simple-post-server/src/dto.rs`，**不要**放回 `commands/`——那会把服务端重新耦合到 `tauri::State`。

出处：`AGENTS.md` → *The Tauri split*、*Dual-transport invariant*。

### 4. 反代前缀只能由服务端处理

`serve-web --public-base-path /sp` 会把整个 router（含 `/api/*`、`/mcp`、健康探针）一起 nest 到 `/sp` 下，并按前缀改写 SPA 外壳的资源引用。

**不要**为了「修」前缀问题去给 `vite.config.ts` 加 `base`，也**不要**加 nginx rewrite，也**不要**加 `<base href>`（它对 root-absolute 的 `src` 无效，看起来修好了其实什么也没做）。前缀只有一处处理，那一处是服务端。

出处：`AGENTS.md` → *Reverse-proxy base path is a runtime fact*。

### 5. MCP 的 stdout 只有 JSON-RPC

`simple-post-mcp` 里 stdout 只承载 JSON-RPC 帧。日志、panic、`__mcp_metrics__{…}` 遥测一律走 stderr。stdout 上出现非 JSON 会直接让 AI 客户端崩掉。

出处：`AGENTS.md` → *MCP stdout discipline*。

### 6. 回归网测试不要删、不要放宽

`crates/simple-post-mcp/src/mcp/mod_test.rs`、`crates/simple-post-server/src/project_api_shells_test.rs` 与 `crates/simple-post-server/src/daemon_supervisor_test.rs` 里有几条**读源码文本**的测试（`include_str!`），它们守护的契约在功能测试里根本观察不到——被永久拒绝的工具照样返回格式正确的 `PermissionDenied`，功能测试看不出区别。改代码时如果这些测试红了，先怀疑自己，不要动判据。

出处：`AGENTS.md` → *MCP permissions — two gates*、*MCP wire-format field names*。

### 7. 其他容易踩的

- `commands/mod.rs` 必须用 **glob** `pub use`——写成条目列表会丢掉 `#[macro_export]` 的 `__cmd__<name>` 宏，`generate_handler!` 报 E0433。
- `src-tauri/src/lib.rs` 的 `http_request` 必须保持**非 `pub`**，标 `pub` 会和 `generate_handler!` 撞名（E0255）。
- `gui` feature（声明在 `crates/simple-post-server/Cargo.toml`）不要移进 `default`。它关着正是为了不让容器构建拖进 GTK/WebKit。
- Zustand selector 不能返回新建的数组或对象（`.map` / `.filter` / 展开），`Object.is` 会导致无限重渲染。用 `.find(...)`，或在组件里 `useMemo`。
- 写库走 `db::with_tx`，别自己拼事务。

## 注释怎么写

四条规则，详见 `AGENTS.md` → *Source comments*：

1. **只写当下成立的东西。** 删掉过去时的缺陷叙事（「以前这里坏过」「这个曾经上线过」），保留现行约束与不变量。
2. **中文、简练、一句一事。** 压成结论加必要理由，只留「不这么写会出错」的部分。不要复述代码。
3. **每个分句点名执行者。** 声明上方的注释可以省主语；函数体内的行内注释必须点名是哪个执行者做的。
4. **不用借代词。** 「兜底」写成「回退」，「面板面」写成「公开接口」，「心智模型」写成「理解方式」，「锚定」写成「保持一致」——一句话要指向具体的代码路径、值或规则，而不是一个画面。

标识符、关键字、API 名、列名保持原样——那不是可翻译的散文。

## Issue

用模板。仓库的高风险点都做成了模板字段：两个模板都列出「数据结构 / schema」与「MCP 工具注册或权限」两项，`feature_request.yml` 另外列出「前后端命令」（描述里点名五处协同）、「反向代理 / base path」、「回归网测试」与「CI / 门禁」，`bug_report.yml` 另外列出「容器 / 部署」。

## 发版

`release.yml` 建的是 **draft** release，`publish-image.yml` 在同一 tag 上推多架构镜像。点 publish 之前按这个顺序走：

1. **确认 tag 已在远端**，且 `release.yml` 与 `publish-image.yml` 两个 workflow 都跑完。
2. **验证 draft 的产物**：
   - 下载 macOS `.dmg` / `.app`、Ubuntu `.deb` 与 `.AppImage`、Windows NSIS 安装包
   - 桌面应用启动一次，`npm run check` 不需要在安装包里跑，但要确认应用能开、能建项目
3. **验证镜像**（需要 `docker login ghcr`）：
   ```bash
   docker pull ghcr.io/Nnyjk/simple-post:<tag>
   docker buildx imagetools inspect ghcr.io/Nnyjk/simple-post:<tag>
   ```
   确认 `linux/amd64` 与 `linux/arm64` 都在清单里。
4. **验证 compose 部署**（对着刚拉的镜像，不带 `build:`）：
   ```bash
   SP_WEB_PASSWORD=<口令> docker compose up -d
   curl -fsS http://localhost:4173/healthz
   ```
   看到 `"status":"ok"` 即可——注意断言的是**响应体**，状态码单独看会骗人（未注册路径会落到 SPA fallback 返回 `200 text/html`）。
5. **手动发布** draft release，并写上 release notes。
6. 打 `v*` 前确认 `main` 上的 CI 是绿的，且版本号已在 `package.json` 与根 `Cargo.toml` 同步更新（两者共用一个 workspace version）。

## 许可

MIT，见 [`LICENSE`](../LICENSE)。