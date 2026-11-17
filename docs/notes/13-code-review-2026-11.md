# 13 · 2026-11 全量代码评审结论

> 适用范围：想知道「这轮评审到底改了什么、还欠什么」的人，以及下一轮评审的起点
> 权威来源：本轮六份报告（`.scratch/review/*.md`，gitignored）+ 代码本身；**「已修」一律以代码为准**，报告只作线索
> 最后核对：2026-11-17
> 前置阅读：[11-handover-open-issues](11-handover-open-issues.md)（文档/注释矛盾总表与修复记录，本文不重复那张表）

## 结论摘要

| 维度 | 一句话结论 |
| --- | --- |
| 注释与文档 | 47→48 工具数、45 200 预算、删库规则、依赖链、`COMMANDS` 位置均已对齐；`com.simple-post` 目录名与 `body_mode` 取值表已改真 |
| 架构与契约 | 能力过滤改用后端同一张表；`api_run_request` 补上项目门；建环境只剩一处实现 |
| 代码质量 | 5 个 patch 类型补 `deny_unknown_fields`；脚本快照/回滚不再静默失败 |
| 依赖 | `sha2`、`tower-http` 归并到已有版本，`r2d2` 降为 dev-dependency，reqwest 注释改成事实 |
| 视觉一致性 | Send 按钮回主色、药丸按底色选墨色、状态码文字走语义 token、标签条可键盘、hover-only 补 focus 通道 |
| 守卫 | 新增 9 条样式哨兵（`src/lib/__tests__/style-guards.test.ts`），字号下限与虚线语汇从此可回归；`scripts-manifest.test.ts` 守 `scripts/` 的静态可达性，`registry_parity_test.rs` / `route_specs_test.rs` 守四处注册表的差集 |

本轮落地 **28 条**（六份报告的 21 条 + 上一版残留清单里收口的 7 条），仍未修 **3 条**（见「未修残留」）。没有遗留 blocker：残留是「门禁成本」与「契约待决」两类，不是会算错数据的缺陷。

## 已修清单

### 能力过滤与权限门

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| 本地 `--capability` 用后缀启发式，与后端 `REQUIRED_CAPABILITY` 分叉（read_only 反而看见 `spec_upsert`） | `crates/simple-post-mcp/src/capability.rs:66` 改为查 `crate::mcp::required_capability`，再用 `CapabilityPolicy::satisfies`（唯一档位比较表），删掉后缀规则 | `crates/simple-post-mcp/src/capability_test.rs:12` `local_filter_agrees_with_the_capability_table` 两张表双向一致；`unknown_tool_names_are_hidden_in_every_tier` 断言 fail-closed |
| `api_run_request` 只看环境所属项目，没有按调用者权限裁决 | `crates/simple-post-mcp/src/mcp/http_ops/run_request.rs:61` 加 `check_permission(&state.pool, pid, "api_run_request").await?` | `crates/simple-post-mcp/src/mcp/http_ops/run_request_test.rs:184` `a_read_tier_project_cannot_send_through_its_environment` |

### 建环境单实现

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| 桌面字面量与 REST serde 缺省值各拼一份 `Environment`，新增字段时一侧报错、一侧静默 | 新增 `crates/simple-post-server/src/environment_api.rs:28` `create_environment`（id 铸法、`variables`、空集合都只在这里拼）；`src-tauri/src/commands/environments.rs` 与 `web_api/routes/environments.rs` 只做线格式 | `crates/simple-post-server/src/project_api_shells_test.rs:78` 的 `ENV_SHELLS` 断言两侧都调 `create_environment(` 且不再出现 `Environment {`；`crates/simple-post-server/src/environment_api_test.rs:50` 断言连续建两个后第一个仍是默认 |

### patch 类型与脚本回滚

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| 5 个 patch 类型静默丢弃未知键（`timeoutMS` 拼错也返回 200） | `crates/simple-post-core/src/domain/project.rs:43`（`ProjectPatch`）、`:141`（`EnvironmentPatch`）、`crates/simple-post-core/src/domain/dynamic.rs:101`、`crates/simple-post-core/src/domain/script.rs:30` 加 `deny_unknown_fields`；`crates/simple-post-server/src/dto.rs:118` 的 `RequestSettingsPatch` 同样补上 | `crates/simple-post-core/src/domain/patch_test.rs:396` `every_patch_rejects_a_misspelled_field`；`crates/simple-post-server/src/dto_test.rs:12` `a_misspelled_setting_is_rejected` + `a_settings_read_back_can_be_written_back_as_a_patch` |
| `crates/simple-post-core/src/script/engine.rs` 用三处 `let _ =` 丢弃快照/回滚失败：回滚不了仍照跑，链上下一段会读到残留值 | `crates/simple-post-core/src/script/engine.rs:164` 的 `run`：快照失败时**一次都不执行**；回滚失败经 `rollback_log`（`crates/simple-post-core/src/script/engine.rs:375`）写进 `effects.logs`（请求/环境各一条），原失败照旧返回 | `crates/simple-post-core/src/script/run_test.rs:280` `a_script_whose_state_cannot_be_snapshotted_is_not_run_at_all`；`:328` `a_failed_rollback_is_reported_in_the_logs_and_keeps_the_original_failure` 断言两条日志 |

### 依赖

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| `sha2 0.11` 与 `oauth2` 的 0.10 并存；`tower-http` 0.7 + 无读者的 `trace` feature；reqwest 注释声称「Windows 上从不编译」 | `Cargo.toml:99` 改 `sha2 = "0.10"`；`:132` 改 `tower-http = { version = "0.6", features = ["fs"] }`；`:38-56` reqwest 注释改成真话（`tauri-plugin-updater` 无条件依赖 0.13） | `cargo tree -d` 里 sha2/tower-http 各只剩一份；`npm run clippy` |
| `r2d2` / `r2d2_sqlite` 只在 `daemon_http_test.rs` 用，却被所有链接方继承 | `crates/simple-post-mcp/Cargo.toml:41` 移入 `[dev-dependencies]`（注释说明生产路径用 `DbPool`） | `cargo build -p simple-post-mcp` 与 `cargo test -p simple-post-mcp` 各跑一遍 |

### 视觉一致性

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| Send 按钮用固定 method 色配 `text-primary-foreground`，浅色档 POST 仅 2.1:1 | `src/features/http-tester/RequestUrlBar.tsx:104` 回 `Button` 默认的 `bg-primary text-primary-foreground`；动词只由左侧药丸表达 | `npm run lint`；深/浅两档目视 |
| 药丸实心档硬编码 `white` | `src/lib/utils.ts:97` `readableInk` 按 HSL 亮度选近黑/白，`src/components/ui/method-pill.tsx:58` 亮色背景用墨色、深色背景用白 | `src/lib/__tests__/style-guards.test.ts` 的「不写死黑与白」 |
| 状态码文字复用块色 `statusColorVar`（固定 HSL，浅底绿字 2.3:1） | `src/lib/utils.ts:84` `statusTextClass` 改语义 token（`text-success-foreground` / `text-warning-foreground` / `text-destructive` / `text-muted-foreground`），`src/features/response-viewer/ResponseViewer.tsx:191` 与 `src/features/response-viewer/ResponseHistory.tsx:232` 改用 | 同上；对比度按 token 走主题 |
| 响应侧手写两颗药丸，不跟 `MethodPill` 的墨色规则 | `src/features/response-viewer/RequestSummary.tsx:86`、`src/features/response-viewer/ResponseHistory.tsx:212` 改 `<MethodPill variant="solid">` | `npm test` |
| 标签条 `role="tab"` 无 `tabIndex`，键盘进不去 | `src/features/workspace/WorkspaceTabs.tsx:430` 补 `tabIndex={0}` 与方向键处理（`:387`），落点抽成 `src/features/workspace/tab-keyboard.ts:9` `nextTabFocusIndex`（Home/End/左右环绕） | `src/lib/__tests__/style-guards.test.ts` 第 9 条只管 hover，键盘行为靠 `WorkspaceTabs` 手测 |
| 9 处 hover-only 控件键盘不可见 | 同一 className 串补 `focus-visible:opacity-100` / `group-focus-within:opacity-100` | `src/lib/__tests__/style-guards.test.ts` 第 9 条；9 处 `group-hover:opacity-100`（分布在 7 个文件）各有对应 focus 通道 |
| `bg-accent/N` 22 处、`text-muted-foreground/N` 48 处手写透明度 | `bg-accent/N` 全部收口（唯一残留是 `src/features/workspace/WorkspaceTabs.tsx:474` 的注释文字）；`text-muted-foreground/N` 只留 12 处**不带字号**的装饰图标（哨兵第 5 条允许） | `src/lib/__tests__/style-guards.test.ts` 第 4、5 条 |
| 字号五档散落、10px 以下也有 | `tailwind.config.js:147-148` 补 `micro`（11px）/`nano`（10px）两档 | `src/lib/__tests__/style-guards.test.ts` 第 6 条（下限哨兵） |

### 文档与守卫

| 问题 | 改动位置 | 关键测试/命令 |
| --- | --- | --- |
| `AGENTS.md` 47-tool、旧预算、删库规则写宽、依赖链缺一环、`COMMANDS` 位置指错 | `AGENTS.md:432`（48）、`:374`（45 200 / 45 116）、`:126`（只在 `found <` 时删库）、`:30`（`server → mcp → core`）、`:277`（`COMMANDS` 在 `src/lib/tauri-commands/commands.ts`） | 与 `mcp/mod_test.rs`、`db/schema.rs` 对照 |
| 数据目录注释写 `com.simple-post`，代码 `simple-post` | `crates/simple-post-core/src/port_file.rs:53-62` 注释改成「不带任何应用段，`port_file_path_at` 自己 join `simple-post`」 | 读代码即可核对 |
| `mcp/resources.rs` 的 `body_mode` 只列 5 个取值 | `crates/simple-post-mcp/src/mcp/resources.rs:153` 补齐 7 个（`none/json/form/multipart/xml/raw/binary`） | `mcp/mod_test.rs` 预算不受影响 |
| `docs/notes/05-mcp.md` 与 `docs/notes/11-handover-open-issues.md:73` 的数字落后 | 两处改为 48 工具、62 条 `COMMANDS`/`WEB_ROUTES`、45 200/45 116 | `npm test`（预算测试会先红） |
| 样式约定无回归网 | 新增 `src/lib/__tests__/style-guards.test.ts`（213 行、9 条）：层级不用任意值/不内联 `zIndex`、不用原生调色板、不写死黑白、hover 不手写 accent 透明度、次要文字不叠 alpha、字号 ≥10px、圆角 ≤lg、虚线边框只属于 EmptyHint（5 个白名单）、hover-only 必须可聚焦 | 9 条 test 名与判据见该文件；`npm test` |

### 收口：上一节 7 条残留的落地

| 原 # | 改动 | 关键测试/命令 |
| --- | --- | --- |
| 1（任意字号） | 全部迁到 `text-micro`（11px）/`text-nano`（10px）：`grep -rn 'text-nano\|text-micro' src` 现在 221 处、62 个文件。`src/lib/utils.ts` 的 `cn()` 用 `extendTailwindMerge` 登记了这两档，否则 `cn('text-xs','text-nano')` 会留下两个类、由样式表顺序决定胜者。`src/styles/globals.css` 里 7 处 `@apply text-[11px]/[13px]` 是类定义本身，按已知例外留着 | `npm test`（`src/lib/__tests__/style-guards.test.ts` 第 6 条下限哨兵） |
| 3（4 个 parity 读取器） | 四处都补了 `deny_unknown_fields` 与精确条数：`crates/simple-post-core/src/path_params_test.rs:50` 32、`crates/simple-post-core/src/dynamic/resolve_parity_test.rs:87/122/139` 5/15/11、`crates/simple-post-core/src/oauth2/inject_parity_test.rs:93` 13、`crates/simple-post-mcp/src/mcp/http_ops/multipart_parity_test.rs:126` 2；TS 侧七处同样钉住条数（`base-url-parity` 11/13、`body-parity` 21、`dynamic-parity` 5/15/11、`http-build-parity` 15、`http-build-multipart-parity` 2、`oauth2-parity` 13、`oauth2-config-parity` 6） | `cargo test --workspace`；`npm test` |
| 4（MCP patch 松散入参） | `project_update` 收 `ProjectUpdateArgs`（`crates/simple-post-mcp/src/mcp/project_ops/project.rs:27-34`）、`env_update` 收 `UpdateEnvArgs`（`crates/simple-post-mcp/src/mcp/env_ops.rs:38-45`）；`patch: Option<ProjectPatch>` / `Option<EnvironmentPatch>` 直接反序列化，字段名拼错在这里报 `unknown field` | `cargo test -p simple-post-mcp --lib` |
| 7（dump-openapi 无门禁） | `package.json:19` 的 `lint:openapi` 链上 `node scripts/dump-openapi.mjs --out target/openapi.yaml`，`check` 因此覆盖它 | `npm run lint:openapi` |
| 8（两处手维护清单） | `crates/simple-post-core/src/script/stdout_test.rs:47` 的 `every_module_source_file_is_guarded` 与 `crates/simple-post-server/src/win_spawn_test.rs:92` 的 `every_windows_spawn_site_is_guarded` 改为目录扫描，新增源文件自动进网 | `cargo test --workspace` |
| 9（Button hover） | `bg-accent` 只留「已选中」（`src/components/ui/method-picker.tsx:160`、`src/features/dynamic-variables/EndpointTreePicker.tsx:213,275`、`src/features/http-tester/BaseUrlPicker.tsx:233`、`src/features/top-bar/TopBar.tsx:158,254`），hover 统一走 `bg-overlay-hover`（66 处）：`grep -rn 'hover:bg-accent\|focus:bg-accent' src` 现为 0 | `npm test`（style-guards 第 4 条） |
| 10（dual-transport 交叉守卫） | `crates/simple-post-server/src/registry_parity_test.rs`（5 条）与 `crates/simple-post-server/src/web_api/route_specs_test.rs`（3 条）把四处注册表的差集钉住 | `cargo test --workspace` |
| 6（scripts 盲区，部分） | 新增 `src/lib/__tests__/scripts-manifest.test.ts`：`package.json` 里每个 `node scripts/…` 目标必须存在、每个脚本能从 npm script 顺着 import 走到（或登记进 `STANDALONE` 并写理由）、脚本里的 `/api/…` 字面量必须是 `web-routes.ts` 的前缀；不 spawn 进程，故留在 `npm test` 里 | `npm test` |

## 未修残留（如实记录，3 条）

| # | 残留 | 为什么这轮没做 | 下次入口 |
| --- | --- | --- | --- |
| 1 | `crates/simple-post-core/src/permissions.rs:191` 的 `resolve_capability` 里，`Some(pid)` 逐项目合并路径（`:209-224`）只有测试在调：生产两处（`crates/simple-post-mcp/src/mcp/dispatch.rs:36`、`crates/simple-post-mcp/src/mcp/registry.rs:133`）都传 `None`，于是 `permission_to_capability()` 与 `stricter()` 也只有测试能走到 | 删掉它要先决定「项目级覆盖」到底要不要，属契约问题；留着则两套语义并存，读代码的人会以为项目级覆盖已经生效 | `permissions.rs:191-224` + `crates/simple-post-core/src/permissions_test.rs` 的三处 `Some(pid)` 用例 |
| 2 | ~~`check` 不含 `cargo test --workspace`~~ → **已用两档解决**：`check` 保持秒级快检，新增 `test:rust`（`cargo test --workspace`）与 `check:all`（`check` + `test:rust`），推之前跑后者 | 直接并进 `check` 会让本地门禁从分钟级拖到十分钟级；两档各留其用 | `package.json` 的 `check` / `test:rust` / `check:all`；跑 `npm run check:all` |
| 3 | 需要真进程/真端口的脚本仍未全进 CI：`test-web-auth.mjs` 与 `test-web-basepath.mjs` **已进 rust job 末尾**（`npm ci` + `npm run build` + `cargo build -p simple-post` 之后），剩下的 `mcp-integration.test.mjs`、`mcp-repl.mjs`、`_verify_store.mjs`、`perf-bench.mjs`、`e2e/*` 仍靠人跑（静态可达性由 `scripts-manifest.test.ts` 守住） | 后者需要 MCP daemon 端口与交互式 REPL，夹具与端口分配还没定 | `.github/workflows/ci.yml` 的 rust job；`scripts/` |

## 一条方法论教训：结论要回读到「落库/兜底那一层」

初判「web 建环境会让新环境抢走默认位」是错的：`crates/simple-post-core/src/db/environments.rs::create` 早就在 `created_at == 0` 时补 `now_ms()`，「最早创建 = 默认」这条规则只有一个出处。真实缺陷是**结构体缺省值有两个出处**（桌面字面量 vs REST `serde` 默认），表现只是「新增字段时一侧编译失败、另一侧静默」。

可复用的检查习惯：

1. 报告说「A 路径会写错值」时，先跟到真正落库/兜底的函数，确认它有没有已经在那一层做了归一化。
2. 同一结构体在两条 transport 上被构造时，把「谁补缺省值」当成一个可搜索的问题（`grep` 该字段名），而不是看两份构造代码是否长得像。
3. 缺省值的作者只能有一个：要么在构造函数（`environment_api::create_environment`），要么在存储层（`db::environments::create`），两边都写就是两个答案。
4. 定性时区分「算错」与「两处维护」：前者要改行为，后者只要收口实现，改动量与测试完全不同。

## 下次评审的起点

1. **219 处任意字号**（残留 1）——先建 9/10/11px 到 `nano`/`micro`/`text-xs` 的映射表，再分批迁。
2. **4 个 parity 读取器的字段锁与条数断言**（残留 3）——这是「新增字段静默通过」的唯一剩余入口。
3. **MCP 的 `project_update` / `env_update` 松 JSON**（残留 4）——AI 侧最容易写出 `timeoutMS` 这类键名的地方。
4. **本地门禁缺口**（残留 5–7）——`check` 不含 `cargo test`（已用 `check:all` 两档解决）、openapi 导出已入门禁、剩余 `scripts/*.mjs` 里只有需要 daemon 端口的那几个还没进 CI。
5. **两个手维护文件清单**（残留 8）——`stdout_test.rs` 与 `win_spawn_test.rs`，新增文件漏扫是静默的。

第 2、3 条优先于第 1 条：字号错了看得见，字段锁漏了要等线上数据形状变了才发现。
