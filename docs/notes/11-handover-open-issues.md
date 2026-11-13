> 适用范围：刚接手、要决定「先查什么、哪些文档不能信」的人；也用于收口时判断一条矛盾是否已被登记
> 权威来源：本表每行的「应以谁为准」列（源码路径:行 或 小节），以及各篇末尾的「文档与实现不一致」
> 最后核对：2026-11-13（v0.1.34）
> 前置阅读：README.md、01-overview.md

# 11 接手清单：已知矛盾、核对顺序、欠账

## 一句话结论

这 21 条都是**已经读过源码、能指出双方位置**的矛盾或欠账，不是猜测；其中只有 3 条会改变运行行为（#7 schema 拒收门的不对称、#13 权限写入的错误形态、#18 镜像版本漂移），其余是「文档/注释在说旧话」。接手时先按第 2 节的顺序跑一遍，再动第 3 节里的欠账。

## 结构：已核实的「文档与实现不一致」总表

核对方法：逐条打开「位置」列指到的文件读原文，再打开「应以谁为准」列的实现确认。结果标在最后一列：**确认** = 与描述一致；**修正** = 我读出的与描述有出入（已按实际写）。

| # | 位置 | 现状说法 | 应以谁为准 | 影响面 | 建议动作 |
| --- | --- | --- | --- | --- | --- |
| 1 | `AGENTS.md` §Important Files（`vite.config.ts` 条目） | 代理 `/api` + `/mcp`（ws disabled on `/mcp`） | `vite.config.ts:45-52` 只有 `/api`，注释明写没有 `/mcp` 代理 | 以为开发态能从 5173 连 MCP，实际连不上 | 改文档（删 `/mcp` 半句）· **确认** |
| 2 | `AGENTS.md` §Important Files（工具注册表一句） | 「the 47-tool registry」 | `crates/simple-post-mcp/src/mcp/mod_test.rs:90` `EXPECTED_TOOL_COUNT = 48`；`README.md` 四处均 48 | 数字对不上时浪费一次排查 | 改文档（47→48）· **确认** |
| 3 | `src-tauri/src/main.rs:33` 与 `:81` | 同一子命令分别指 `simple-post-mcp-proxy` 与 `simple-post-mcp.exe` | `crates/simple-post-mcp/Cargo.toml` 的 `[[bin]] = simple-post-mcp`；旧名已随迁移删除 | 照 `:33` 找一个不存在的二进制 | 改注释 · **确认** |
| 4 | `crates/simple-post-mcp/src/mcp/project_ops/mod.rs:51-54` | endpoints (8)、「含全部 24 条」 | `project_ops/endpoint/registry.rs` 推 9 条（多 `endpoint_history`），合计 25 | 数字注释与注册表不一致，排查工具数时误导 | 改注释（8→9、24→25）· **确认** |
| 5 | `crates/simple-post-mcp/src/mcp/resources.rs:153` | `body_mode` 枚举 `none/json/form/multipart/raw`（5 个） | 同文件 `:182` 的 `guide.rules` 与 `core::spec::input::BODY_MODE_VALUES` 都是 7 个（多 `xml`、`binary`） | 照模板写的 AI 以为不支持 `xml`/`binary`；`resources_test.rs` 只校验键存在、不校验枚举值 | 改代码（补两个值）· **确认** |
| 6 | `AGENTS.md:194`（Key Directories 的 fixtures 条目） | `base-url-parity.json` 的 Rust 侧是 `core::url` | 读取器在 `crates/simple-post-mcp/src/variable_fill/base_url_parity_test.rs`，实现是 mcp 的 `resolve_base_url` + core 的 `apply_rewrite`；同文件 `:75` 的说法正确 | 去 core 找 `resolve_base_url` 会空手（core 只有改写） | 改文档 · **确认** |
| 7 | 桌面 `endpoint_detail_update`（`src-tauri/src/commands/nodes.rs:176`）与 REST `PATCH /api/nodes/{id}/endpoint-detail`（`web_api/routes/nodes.rs:274`） | 04 篇写「只有 MCP/REST 写入路径有」schema 拒收门 | 门只存在于 `core/spec/upsert/write.rs:85`、`project_ops/endpoint/write.rs:192,339`、`variable_fill.rs:414`；**REST 的普通端点更新路径同样没有** | 同一份「Schema 当载荷」数据，AI 走 MCP 被拒、用户桌面/浏览器保存照过 | 改文档写清不对称；是否补门需决策 · **修正** |
| 8 | `crates/simple-post-core/src/dynamic/mod.rs:30` | 退化顺序只有「覆盖值 → 过期缓存（`stale_fallback`）→ 按缺失」 | `dynamic/cache.rs::decide` 的完整表还含「新鲜缓存」「成功取数」两档 | 读模块文档会以为缓存永远先于取数 | 改注释 · **确认** |
| 9 | `src/main.tsx:30` | `bootstrap()` 在 `loaded \|\| loading` 时幂等早退 | `src/stores/bootstrap-slice.ts:309` 只有 `if (get().loaded) return;` | 并发调用会重跑第 1 层；当前只有 `main.tsx` 调一次，故不触发 | 改注释或补守卫（二选一）· **确认** |
| 10 | `src/features/workspace/WorkspaceShell.tsx:8-9` | 「这里刻意不挂载 `ProjectTree`」 | 同文件 `:169` 是 `{activeProjectId !== '' && <ProjectTree />}`（条件挂载） | 读注释以为树在别处渲染 | 改注释（「没有项目时不挂载」）· **确认** |
| 11 | `crates/simple-post-core/src/db/preferences.rs:94` | `MCP_AUTO_START` 文档「默认 `false`」 | 同文件 `:140-143` 实现 `raw.unwrap_or(true)`，`:138` 注释解释为什么默认开 | 误判部署后 MCP 默认是否在跑 | 改注释 · **确认** |
| 12 | `crates/simple-post-core/src/db/permissions.rs:4` | 「刚导入的项目无需显式授权即可使用」 | 逐项目缺行确实是 `Permission::default() = ReadCall`，但全局 `mcp.policy` 默认 `read_only`，两道门取更严 | 措辞像「AI 默认能写」，实际不能 | 仅记录（可补一句点明两道门）· **确认** |
| 13 | `crates/simple-post-core/src/db/permissions.rs:34` `set` | 不校验项目存在，坏 id 以原始外键错误收场 | schema 有 FK + CASCADE，不会留下悬挂行（行为安全，只是错误不友好） | 调用方拿到的是 `Db(...)` 而不是 `NotFound`；是否有意 `[未核实]` | 需人工确认（改则先查项目）· **确认事实，意图待定** |
| 14 | `src/lib/__tests__/placement-parity.test.ts:5` | Rust 读取器写成 `crates/simple-post-core/src/placement_parity_test.rs` | 实际是 `crates/simple-post-core/src/domain/placement_parity_test.rs`（`#[path]` 挂在 `domain` 下） | 照注释找不到文件 | 改注释 · **确认** |
| 15 | 五个 parity 读取器：`variable_fill/resolve_parity_test.rs`、`core/path_params_test.rs`、`core/oauth2/inject_parity_test.rs`、`core/dynamic/resolve_parity_test.rs`、`mcp/http_ops/multipart_parity_test.rs` | 交接说明称「五个都只断言非空，且没有 `deny_unknown_fields`」 | 五个确实都没有 `deny_unknown_fields`；但 `resolve_parity_test.rs:137` **断言了精确条数 15**（注释还专门否掉了「非空」写法），其余四个才是只断言 `!cases.is_empty()` | 键名漂移时后四个会静默少跑；resolve 有数量兜底 | 改代码（补 `deny_unknown_fields` 或补条数）· **修正** |
| 16 | `fixtures/oauth2-config-parity.json`、`fixtures/placement-parity.json` | 缺 `$comment` | 逐文件核对：其余八份各有一条 `$comment`，这两份为 0 | 靠 `$comment` 找读取器的人在这两份落空 | 改 fixture（补 `$comment`）· **确认** |
| 17 | `crates/simple-post-interchange/src/commit/mod.rs:70-72` | `ProjectRoot` 文档写「各自新建一个顶层模块。这是资源树里模块唯一的合法位置」，读起来像现役入口 | 生产路径只有 `interchange_api.rs:133` 的 `CommitTarget::Module`；`ProjectRoot` 只出现在 `commit/*_test.rs` 与 `support.rs` | 让人以为存在「导入到项目根」的入口 | 需人工确认（保留就改文档，否则删）· **确认事实** |
| 18 | `docker/docker-portainer.yml:70` | 硬编码 `ghcr.io/nnyjk/simple-post:0.1.34` | 发布清单只有四份版本号；`docker/docker-compose.yml:26` 的同类写法只是注释 | 升级后 Portainer 栈仍拉旧镜像，且没人会收到提醒 | 改流程（纳入发布清单）· **确认** |
| 19 | `.github/workflows/release.yml:82` | `tagName: ${{ inputs.tag \|\| format('refs/tags/{0}', github.ref_name) }}` | 发布说明与 `publish-image.yml` 都按裸 `vX.Y.Z` 叙述 | 若 tauri-action 不接受 `refs/tags/` 前缀，draft release 的 tag 会异常 | 需人工确认（跑一次 release 或查上游文档）· **确认事实，兼容性未核实** |
| 20 | `crates/simple-post-core/src/db/mod.rs:7-8`、`crates/simple-post-server/src/main.rs:50-54`、`crates/simple-post-core/src/port_file.rs:53-55`、`crates/simple-post-server/src/daemon_supervisor.rs:62-65` | 数据目录/`store.db` 落点四处说法不一：`db/mod.rs` 写 Windows 是 `%APPDATA%\store.db`（与默认落点一致，但没提 `--data-dir`）；`server/main.rs` 的 `--data-dir` 文档写默认 `dirs::data_dir()/simple-post`；`port_file.rs` 注释让调用方 `.join("com.simple-post")`，实际代码 join 的是 `"simple-post"`（`:61`）；`host_data_dir()` 返回 `dirs::data_dir()/simple-post` | `db/pool.rs:24-26` + 三个 `db::init` 调用点（`src-tauri/src/lib.rs:138`、`web_api/serve.rs:35`、`server/src/lib.rs:121`）：库 = 传入目录 + `store.db`；`mcp.port` = 同一基准目录 + `simple-post/mcp.port`（`port_file.rs:60-63`）。Windows 默认即 `%APPDATA%\store.db` 与 `%APPDATA%\simple-post\mcp.port` | 按 `%APPDATA%\simple-post` 找库会落空、按它删库会删错位置；`serve-web --data-dir` 只搬主进程的库，daemon 仍读环境变量/`%APPDATA%` | 改注释与文档（不动代码）· **确认** |
| 21 | `AGENTS.md` §Key Directories（`:181` 起）、§Important Files（`:409` 起） | 文件地图缺条目：`docker-portainer.yml`（已随 Docker 文件迁入 `docker/`，并在 AGENTS 的 `docker/` 条目中列出）、`crates-guide.html`、`src/lib/error-text.ts`、`src/hooks/useLocalExpansion.ts`（§Key Directories 连 `src/hooks/` 整个目录都没提）、`.thinkrail/` | 仓库实际树：这五个路径都存在（`docker/docker-portainer.yml` 在 `docker/`，`crates-guide.html` 在根，另三个在 `src/`、`src/lib`、`src/hooks`） | 按地图找 Portainer 栈文件、错误文案或本地展开 hook 会空手；新文件不知道归到哪一类 | 改文档（补条目）· **确认** |

## 结构：接手后建议的核对顺序

按「最可能真出事」排序。每条都写清期望结果；不期望时先怀疑什么。

1. **两条权限闸哨兵**：`cargo test -p simple-post-mcp`。
   期望：`no_gate_is_passed_the_literal_global` 与 `no_permission_check_result_is_discarded` 通过。
   不期望时：说明有人新增了绕过 `nodes::root_id_of` 的 handler，或把 `check_permission(...)?` 写成了 `.ok()`——先读失败信息点名的文件，**不要**放宽判据。

2. **parity 那批**：`npm test`（自动收 `src/**/*.test.ts`）。
   期望：全绿，且 `http-build-parity` / `body-parity` / `base-url-parity` 等各跑满样本条数。
   不期望时：多半是只改了两侧之一。想验证第 15 条的空转风险，可临时把某个 fixture 的键名改错一个字母，再跑**读它的那个读取器**——五个里有四个是 Rust 的（`cargo test -p simple-post-core <测试名>` / `-p simple-post-mcp <测试名>`），本步的 `npm test` 只收 `src/**/*.test.ts`，证明不了 Rust 侧；绿了就说明该读取器没有兜底，改回后再决定是否补校验。

3. **两个数字**：工具数 = 48（`mcp/mod_test.rs` 的常量与 `tool_table().len()`），`COMMANDS` 与 `WEB_ROUTES` 各 59 条。
   期望：三处都与实测一致。
   不期望时：先看是不是刚加了工具/命令没同步三处；这两个数字是本仓库最容易被抄错的地方（见第 2、4 条）。

4. **OpenAPI 导出**：`npm run lint:openapi`。
   期望：Redocly 无 error（`no-unused-components` 已显式打开为 error）。
   不期望时：通常是 `components.schemas` 里出现没人引用的定义，或某个 operation 缺 `responses`——两者都是导出侧的真缺陷，不是 linter 太严。

5. **起一次真实服务**：设 `SIMPLE_POST_DATA_DIR` 到临时目录后 `npm run dev:split`，打开 `http://127.0.0.1:5173/`。
   期望：欢迎页正常、能建项目、`/api/projects` 返回 200、设置面板可达。
   不期望时：先确认 4173 是否真的起来（`/api` 由 Vite 代理过去），再确认数据目录是空的新库而不是用户那份（本构建的 `SCHEMA_VERSION` 更高时会删库重建，见 01 的陷阱）。

6. **Rust 全量**：`cargo test --workspace`。
   期望：通过；尤其 `openapi_model_test`（用 `openapiv3` 反序列化自家产物）不能红。
   不期望时：interchange 的导出形状变了，或 `db/schema.rs` 的增列逻辑弄坏了一个 fixture 库。

7. **桌面形态冒烟**：`npm run tauri:dev`，建一个节点、改一次 endpoint、开一次导入预览。
   期望：全部走通。
   不期望时：`invalid args … missing required key <x>` 就是第 7 条那类「参数没有逐字段摊平」；源码形状测试抓不到真实的 payload。

8. **（可选）容器**：改动碰到 `crates/simple-post-server` 或 `docker/Dockerfile` 时跑一次 `docker build -f docker/Dockerfile .`。
   期望：镜像里没有 `libgtk*`/`libwebkit*`，`/healthz` 与 SPA 正常。
   不期望时：`simple-post-server` 重新依赖了 Tauri，或运行阶段被加进了包。

## 结构：明显的「下一步」候选（欠账，不是 bug）

1. **给五个读取器补严格校验**（第 15 条）。
   现状：都缺 `deny_unknown_fields`，其中四个只断言样本非空。
   代价：fixture 键名漂移时测试静默通过，两侧实现分叉要到线上才发现。补法有两种：加 `deny_unknown_fields`，或照 `resolve_parity_test.rs:137` 那样写死条数。

2. **给两份 fixture 补 `$comment`**（第 16 条）。
   现状：`oauth2-config-parity.json` 与 `placement-parity.json` 没有 `$comment`，其余八份都有。
   代价：想知道「这两份谁在读」只能去翻测试文件头，新人和 agent 都会多跳一次。

3. **把 `docker/docker-portainer.yml` 的镜像版本纳入发布清单**（第 18 条）。
   现状：版本源被定义为四份，compose 簇里的 Portainer 栈写死了 `:0.1.34`。
   代价：每次发版漏改这一行，Portainer 用户的升级会静默失败——没有报错，只有旧镜像在跑。

4. **补齐 `endpointTemplate.body_mode`**（第 5 条）。
   现状：模板枚举 5 个值，`guide.rules` 与 `BODY_MODE_VALUES` 是 7 个。
   代价：AI 客户端按模板填 `xml`/`binary` 时会绕路或干脆不用这两个模式；`resources_test.rs` 目前只查键存在，不会替它报错。

5. **决策 `CommitTarget::ProjectRoot`**（第 17 条）。
   现状：文档当现役入口写，生产路径零调用，只有测试用。
   代价：读代码的人会以为存在「导入到项目根」的产品能力；要么补上入口，要么删掉变体并改文档——两条都比留着含糊好。

## 不变量

- 本表是矛盾的**唯一索引**：新发现先登记在这里，再在对应篇章末尾留一句上下文（README 维护规则第 4 条）。
- 每行的「应以谁为准」必须指到源码可核对的位置；指不到就写 `[未核实]`，不要写结论。
- 「确认」与「修正」是核对结果，不是严重度；严重度看「影响面」列。
- 修掉一条就把该行标成已修（或删掉），不要留着「历史记录」——本仓库的注释纪律同样适用于笔记。

## 陷阱

- **#7 最容易被误读**：它不是「桌面少了一道校验」这么简单——REST 的普通更新路径也没有，所以这条不对称横跨三种传输，补门要三处一起补，否则会出现「浏览器能存、桌面能存、AI 存不了」的第三种结论。
- **#15 的原始描述有一半不成立**：`resolve_parity_test.rs` 有精确条数断言。按错误描述去「补非空断言」会把它已有的兜底改弱。
- **#13 / #17 / #19 的「是否有意」我没有答案**：它们标的是未核实，不是缺陷；动之前先问维护者或看 git 历史。
- 别把本表当待办清单照单全改：其中 14 条是改文档/注释，改代码的只有 #5、#15（以及需决策的 #7）。

## 相关笔记

- [README.md](README.md) — 索引与维护规则
- [09-engineering-process.md](09-engineering-process.md) — 门禁、CI 与发布流程（第 18、19 条的背景）
- [08-interchange-and-parity.md](08-interchange-and-parity.md) — 第 14–17 条的机制背景
- [05-mcp.md](05-mcp.md) — 第 2、4、5、12 条的机制背景
- [04-request-pipeline.md](04-request-pipeline.md) — 第 7 条的机制背景
