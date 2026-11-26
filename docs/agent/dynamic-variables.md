# 动态变量

> 从 `AGENTS.md` 拆出的按需文档。动本域时读它。

## 归属与解析时机

- 定义属于**一个项目**：`dynamic_variables` 表有 `project_id`，名字项目内唯一——两个 partial unique index，有项目的行走 `(project_id, name)`，未归属的行走 `name where project_id IS NULL`（SQLite 多列唯一索引把 NULL 当互不相等）。故两个项目可以有同名变量各取各的。
- **解析在替换之前**，两侧各做一次。定义是声明（名字 → 数据源 → 抽取规则 → 转换脚本 → 缓存时长），正文里的 `{{sns}}` 在发送时换成取到的值。
- **不跨项目回落。** 项目 A 的 `{{name}}` 只对 A 的定义解析；A 没有就是缺失，绝不取 B 的同名定义。跨项目查询是**调用接口**的事（`dynamic_var_resolve` 与 `POST /api/dynamic-variables/resolve`，可选的 `project_id`）：不点名时多处同名报**歧义**（`DynamicErrorKind::Ambiguous`），不猜。

## 取数上下文

`core::dynamic::source_context` 由定义、调用方当前环境与数据库算出。模块没有第二选择：地址只存在 (环境 × 模块) 格，故取数用**源接口所在模块**。

**环境点名**（`source_environment_id` 有值）：用它，且必须属于源接口的项目。判定在 `check_source_bindings`（`db/dynamic_variables/validation.rs` 的模块私有函数），否则 `AppError::Validation`（「环境 … 不在数据源接口所属的项目里」）；行不存在也是 `Validation`（「环境 … 不存在」），不是「没有环境」。

- **只有 `NotFound` 变那条校验错。** `db::environments::lookup_project_id` 对不存在的环境报 `NotFound`、真实数据库错误放过去，`check_source_environment` 只映射 `NotFound`——故一次抖动不会因为 patch 没点名环境就把合法的 `source_environment_id` 置空。`write.rs` 的「未点名就保留旧值」匹配同一个 `Validation` 分支，两种陈旧（行已删、属于别的项目）都降级 NULL，其余字段照存。
- **读路径同样校验。** `source_context` 在取数时返回同一个 `Validation`：preview 整体失败，resolve 记 `source_error` 当缺失；两者都不拿默认环境顶替（顶替会让「这值来自哪个环境」变成假话）。
- **坏行逐行降级。** `crates/simple-post-mcp/src/dynamic/ops.rs` 的 `list` / `get` 捕获 `source_context` 错误，返回 `source: missing`、无值无缓存时间、消息挂在该行 `error`（kind `source_error`）——一行陈旧（环境已属别的项目，如 `move_to_root` 搬走了源接口；或环境行已删）不该让整个面板 400。`set_override` 仍报错（显式动作，用户要看到原因）；`resolve` 继续记 `source_error` 当缺失，一次发送不因一条定义配错而失败。

**环境未点名**（`NULL`，默认）：用**调用方**环境（请求发出时用户选中的，即 MCP 工具或 REST body 的 `environment_id`）；调用方没有、或其环境属于别的项目，回落到源接口所属项目的**默认环境**（`db::environments::get_default`：`created_at` 最早）；再没有就是没有环境。「按名字取第一个」已删，没有读者。

`detail.base_url_override` 优先于两者。表单把「继承」存成 `null` 而不冻结当时的值，故切环境对所有继承定义一起生效；缓存键用**生效**环境，切环境读不到上一个环境的值。

## 表结构与迁移

- **旧预览表在打开时 drop，不迁移。** `db::schema::migrate` 先跑 `drop_obsolete_dynamic_variable_tables`：有 `project_id` 但**没有** `source_environment_id` 时 drop `dynamic_variable_cache` 与 `dynamic_variables`。判据不能是「有 `project_id`」（当前形状也有），缺后面那列才区分得开；`CREATE TABLE IF NOT EXISTS` 对旧文件是 no-op，没有这次 drop 旧行就留在形状不同的表里。
- **迁移次序承重。** `add_missing_columns` 在 `SCHEMA_SQL` **之前**：新唯一索引建在 `project_id` 上，SQLite 拒绝在缺列的表上建索引，整趟迁移会失败。故它要问「表在不在」（`table_exists`）而不只是「有没有这列」——`PRAGMA table_info` 对不存在的表返回空行集。随后 `SCHEMA_SQL` 跑、旧的全库唯一索引 `idx_dynamic_vars_name` 被 drop（`drop_obsolete_dynamic_variable_name_index`，留着第二个项目就永远存不下同名变量），**最后** `backfill_dynamic_variable_projects` 给既有行找归属：源接口项目 → 工作区第一个项目（`created_at`，再 `id`）→ 不给（未归属，等 `DynamicVariablePatch::project_id` 认领）。backfill 跑在新索引之后，故新规则已护着它。
- 死**表** drop、死**列**留：见 `AGENTS.md` 的「数据层」「`SCHEMA_VERSION`」。`ADDED_COLUMNS` 加列不涨版本号。

## 值如何进请求

- **值进变量池，不改写已拼好的请求。** 合并规则：动态值胜过同名环境变量；**被禁用**的环境变量不影响动态变量。两者都只因「动态值在禁用过滤**之后**合并」而成立。
- 替换两处：`prepareRequest`（`src/lib/http-build.ts`）与 `resolve_endpoint_with`（`crates/simple-post-mcp/src/variable_fill/resolve.rs`）。解析步骤住在各自**调用方**，不在 `script::send_with_scripts`——那个函数只做第 7 步（脚本链 → 发送 → 写回），它跑时替换早已完成。
- `core::dynamic::merge_pool` 与 `mergePool`（`src/lib/dynamic-vars.ts`）是同一条规则的两份实现，由 `fixtures/dynamic-parity.json` 钉住（`crates/simple-post-core/src/dynamic/resolve_parity_test.rs` / `src/lib/__tests__/dynamic-parity.test.ts`）。改规则就改两处。
- **引擎在 core，取数器在 mcp crate。** `core::dynamic` 拥有合并规则、值序列化、缓存判定、错误分类与单飞注册表，定义 `SourceFetcher` 抽象；实现住 `crates/simple-post-mcp/src/dynamic/source.rs`（「引用端点」要用 `variable_fill::resolve_endpoint` 拼请求，core 不能依赖它）。三个消费方（MCP 工具、REST 路由、Tauri 命令）共用的操作在 `crates/simple-post-mcp/src/dynamic/ops.rs`。

## 取数失败与缓存

- **取数失败不失败发送。** 变量当缺失（`{{sns}}` 原样留在正文）、记一条错误。降级顺序：覆盖值 → 过期缓存值（只在 `stale_fallback` 开时）→ 缺失。缓存键 (定义 × **生效**环境)，住 SQLite——桌面、`serve-web`、MCP 守护进程是三个进程。
- 缓存行由定义派生：`db::dynamic_variables::update` 在 patch 真改了 `extract_path`、`transform_script`、`source_endpoint_id` 或 `source_inline_json` 时，于同一条 `UPDATE` 的事务里删该定义 `is_override = 0` 的行。覆盖行留下（用户钉住的值不派生自定义）。`ttl_seconds` 不触发失效（派生没变，`expires_at` 自己过期），`source_environment_id` / `value_mode` 也不失效（缓存键与读时序列化各自覆盖）。`value_mode` 的 `text` 与 `auto` 输出**相同**——`text` 只是「值要放进字符串位置、转义自己负责」的声明；只有 `json` 给标量加引号。
- **没有数据源就不读自动缓存行。** 源接口被删时外键把 `source_endpoint_id` 置 NULL，`core::dynamic::cache::decide` 忽略非覆盖行（连 `stale_fallback` 候选），解析报缺失——旧值不许比产出它的方式活得久。覆盖行仍生效；该定义照样列出可编辑（`has_source()` 只决定解析读什么，不决定面板能存什么）。
- **覆盖值比来源活得久。** 删掉源接口后定义点不了环境、缓存键退化成空串，而覆盖值躺在当时生效的环境键下。解析因此横跨该定义所有环境键找仍有效的覆盖（`db::dynamic_variable_cache::live_overrides_for_definition`：只覆盖行、有值、未过期），取最近保存的，同刻按 `environment_id` 升序。自动行不参与（没来源的值不许复活），过期或无值的覆盖不算。`ops::list` / `ops::get` 同口径，面板与发送不会各说一套。
- **取数中途定义被改，值不写回。** 期间 `source_endpoint_id`、`source_inline_json`、`source_environment_id`、`extract_path`、`transform_script` 任一变了就跳过写入（比较与 `upsert` 同事务）；调用方仍拿到这次的值，只是没有缓存、没有 `expiresAt`。定义被删同一情形，不是错误。
- **取数中途写入的覆盖也赢。** `resolve_one` 记入口时覆盖的**身份**（`OverrideRef`：`value_json`、`fetched_at`、`override_expires_at`）。写回事务比对：身份未变的活覆盖正是这次要替换的（只有 `force` 允许在活覆盖上开取，故是刷新在本职）；身份**不同**的是期间存的，跳过写；非覆盖行照常替换。入口没有覆盖时任何活覆盖都受保护。调用方仍拿到值，缓存行留住手写值。`OverrideRef` 比的是值、取数时间与截止时间而非版本号（同值同截止同一毫秒两次保存看起来一样）；版本号列**不**被「`SCHEMA_VERSION` 换库」挡住（`db/schema.rs` 的 `ADDED_COLUMNS` 加列不涨版本），没做只因窗口要三样同时命中。
- **端点模块是取数身份的一部分。** 地址在 (环境 × 模块) 格，搬端点会静默换地址，写回事务重读 `db::nodes::module_id_of` 与取数开始时捕获的比：不同就跳过写入（值仍返回，不缓存）。
- **时钟在读完缓存行之后采样。** 入口读与锁后读都先 `get` 再 `now_ms()`，两次之间过期的覆盖不会被判成活。
- **拿到单飞锁之后重跑判定。** `resolve_one` 先调一次 `cache::decide` 判断取数是否可能，但 `fetch_one` 要在 per-(定义 × 环境) 锁上等，期间别的连接可能填了缓存或存了覆盖。故 `fetch_one` 用**等待之后**的时钟重读、再调同一 `decide`：`UseOverride` / `UseCache` 直接返回，不发请求、不进写回事务；只有 `Fetch` 才取数，并用该判定的 `stale_fallback`。写回守卫的 `seen_override` 仍来自**锁前**那次读。
- **`Stale` 就是过期，在失败那一刻判定。** `failure_fallback` 重读缓存行、读后采样时钟，再问 `cache::fallback_value`（定义允许回退、自动值只在其来源仍在时、覆盖行允许、且该行必须**此刻**过期）：取数可能跨过过期点（开始新、失败时过期，正是它该顶上时）；曾过期但期间被写成新的则不许当过期值给出。故 `Decision::Fetch` 不带预先算好的回退值——它只回答路由。

## 恰好一个数据源

`db::dynamic_variables::create` 拒绝既无端点也无内联请求的定义，也拒绝两者都填。`source_endpoint_id` 由外键 `ON DELETE SET NULL` 在端点被删时清空——故两列都空就是「数据源接口已删」，面板显示 `数据源已删除`。放开创建侧这道门这个区分就没了：面板会把「表单没填」报成「端点被删」。

**更新是另一半：** patch 不许把有来源的定义改成没来源；但**本来就没来源**的定义可以保存（那行只因子端点被删而存在，改名或修抽取规则什么都不丢）。故表单只发真改过的来源字段：对没动的 `source_endpoint_id` 发 `null` 等于声明「没有来源」，正是「改个名都存不下来」那个错。来源环境属于别的项目（陈旧行）时，未点名的 patch 把那个环境丢掉，而不是让保存失败。

## 转换脚本

顺序 fetch → extract（JSONPath）→ transform（脚本）→ cache。脚本看到 `pm.data`（抽到的值）与 `pm.response`（原始响应：`text()` / `json()` / `truncated`），`return` 即最终值；没有脚本时原样通过。

同一个 `core::script` 沙箱，同样三条规矩：不 panic、不 stdout、跑在阻塞线程（`script::run_transform` 用 `spawn_blocking`）。抛错、超时、什么都不返回都是 `DynamicErrorKind::ScriptError`，名字当缺失走降级顺序。脚本对环境的写入**不落库**（纯计算，产物只有一个值）。`pm.response.truncated` 为 `true` 表示传输层按 `max_response_size_kb` 截断了正文。

## 引用关系不存在

刻意不做：常用变量几乎被每个端点引用，在变量侧维护反向索引只给出一份用户无法处理的清单。`db/dynamic_variables/references.rs` 已删，`dynamic_var_list` 没有 `include_references`，面板没有「被引用」一行。

## 面板

- **项目内标签，不是全局单例。** 标签 id 由项目铸（`stores/store-types.ts::dynamicTabId(projectId)`，前缀 `__dynamic_variables__:`），故 `tabOwningProject` 不用再查、`switchProjectTabs` 随该项目标签条暂存、项目没了由 `pruneStaleTabs` 退场。只列**当前项目**的定义；欢迎页没有入口，只写一句「动态变量在项目内维护，打开项目后从顶栏进入」（设置那个单例有自己的 `SettingsSurface` 宿主，动态变量没有）。
- **卡片 key 是 `${variable.id}:${activeEnvironmentId}`**：覆盖编辑器的文本存在卡片 state 里，而它编的是 (定义 × 取数环境) 的一个格子，换环境必须重挂，否则为 A 敲的那行存进 B 的格子。

### 草稿在 store 里，按项目分桶

`stores/dynamic-slice.ts` 拥有 `dynamicDrafts: Record<projectId, DynamicVariableDraft>`：`creating` / `editingId`，加上表单初始化的每个字段（`name`、`description`、`sourceKind`、`sourceEndpointId`、`sourceProjectId`、`sourceEndpointName`、`sourceInlineJson`、`environmentId`、`extractPath`、`transformScript`、`valueMode`、`ttlSeconds`（留原始输入串，好让「空」活下来）、`staleFallback`、`enabled`）与 `openedWith`（打开那一刻的定义）。

- 必须在 store：`WorkspaceShell` **只挂活动标签**，切走就整棵卸载，`useState` 跟着死。别改成「所有标签都挂着」——那会让每个标签的 CodeMirror 都启动。
- 读 `useAppStore((s) => s.dynamicDrafts[projectId])`，写走 `patchDynamicVariableDraft(projectId, patch)`（浅合并，一项目一对象）；别用 `(s) => ({ … })` 选择器。
- 只在表单无处可去时才清：保存成功（`finishDynamicVariableSave`，且只在提交的那份仍是当前草稿时）、`取消`（表单 `onCancel`）、删掉正在编辑的定义（`deleteDynamicVariable` 比 `editingId`）、标签**真的被关**（`closeTab` / `closeAllTabs` / `closeSavedTabs` / `pruneStaleTabs`，都走 `slices/ui-tabs.ts::withoutClosedDynamicDrafts`）、项目删除成功。`switchProjectTabs` **不许**清——它只暂存标签条，「不活动」不等于「被关」。`deleteProject` 里的清理跑在 `invokeWithRollback` **之后**、不在 `set()` 里：那次写会从一份不含草稿的快照回滚。
- 草稿不放瞬时状态（预览响应、`busy`、保存错误）：重挂丢预览可以接受，丢表单不行。
- `draftSession`：`beginCreateDynamicVariableDraft` / `beginEditDynamicVariableDraft` 各按项目自增，收尾比**会话而不是内容**——会话过期就一个字节都不动，`openedWith` 也不动。A 的 PATCH 在飞时用户可点 `取消` 再开新表单（`editingId` 也是 `null`），按形状匹配会把新表单改写成「正在编辑 A」，下次保存覆盖 A。
- `openedWith` 只推进**这份 patch 真带了的字段**（`advanceDraftBaseline`）：整份换成响应会把别的客户端刚改的描述从 B 拿回来而草稿还是 A，下次保存又把 A 发回去，静默回滚一次本表单没碰过的改动。
- **create 的在飞标记没有定义 id**，按项目键（`creatingDraftKey(projectId)`，即 `new:<project id>`）并共用 `savingDynamicVariableIds`；住 store 而非组件 state，因为切标签会卸载表单、归零而 POST 还在路上——再点 `保存` 就建出第二条并撞名字唯一。按钮读「保存中…」并禁用，切回来还是那样。

### 编辑只发变过的字段

`features/dynamic-variables/edit-patch.ts::buildEditPatch(initial: DynamicVariable, draft: DynamicVariableDraft)` 逐字段比对「打开表单那一刻的定义」与「此刻的草稿」，只把变了的键写进 patch；后端 `DynamicVariablePatch` 是双 Option（缺席 = 不动、`null` = 清除、值 = 写入）。

- 比对前两边归一到「保存后会是什么」：空串、全空白与定义里的 `null` 都算「没有」，故「本来就空、现在还是空」既不发 `null`（撞「必须有一个数据源」）也不发空串。
- 基线必须是 `draft.openedWith` 而不是「此刻的 `variable`」：表单会被卸载重挂、列表会被刷新（切环境、别处保存），拿刷新后的定义当基线会把别人刚存的改动判成「我改的」再写回去。
- 缓存时长走 `ttlSecondsOf`：空输入按 60 秒算，**不能**读成 0（0 是「不缓存，每次重取」）。
- 保存门判 `keepsSource(baseline, buildEditPatch(baseline, draft))`——**这份 patch 落到这条定义之后**还有没有来源，而不是「定义此刻有没有」：别的客户端把删掉的接口补回来之后，本地只改名字的 patch 里根本没有来源那两格，不该被本地过期列表禁掉；把两个来源都清空的必须挡下。创建仍要求来源。

### 值桶按项目 × 取数环境

- `dynamicValues` 是 `dynamicValueScope(projectId, environmentId) → name → status`（`dynamicErrors` 同形），`stores/dynamic-slice.ts::dynamicValueScope` 用 `\u0000` 连两个 id——不是 `projectId → name → status`，也不是扁平 `name → status`。只按项目分桶会把 A 的值带进 B 的请求。桶键在请求发出**之前**算好、结果发布回同一个键，故切环境之后才落地的慢响应写进 A 而界面读 B。
- `dropDynamicValue(projectId, name)` 清扫该项目的**每一格**（改名或删除时用户可能站在别的环境）。`HttpTester` 与面板的错误查找都读**当前** `activeEnvironmentId` 那一格。
- **新鲜度按格 × 变量名追踪**（`valueSeq` 键是 `scope\u0000name`）：批量解析与单条刷新同时在飞，按格的序号会让后到的单条刷新吞掉整批其他变量的结果。被同名新请求取代的结果**不写 store 但返回给调用方**（它要填自己的占位符）。
- `updateDynamicVariable` 让这个名字在**每一格**上在飞的发布作废，否则按旧规则取回的值会冒充新规则的结果。作废按**所属项目**圈定（`invalidateName(projectId, name)`）：只按名字匹配会推进 p2 的代号，p2 的解析把值返回给调用方却从不到达桶。成功的**删除**也作废（在 `dropDynamicValue` 之后），否则在飞的解析把值加回桶，留下一个定义已删的变量；删除失败时那些写入仍有效。
- 「取数中」是**计数器**（`dynamicInFlight`）：批量与单条同时在飞，先落地的不许在另一个还在路上时放开按钮；每条退出路径都减一，含拒绝与被取代。
- **列表**失败与成功同受代号规则管辖（`listSeq`）：被取代的请求不许写 `dynamicError`。列表与值桶同键：`loadDynamicVariables` 把 `dynamicVariablesScope`（环境在请求之前读）与行写在同一个 `set()`，面板用 `dynamicListMatchesEnvironment` 问「屏幕上这份属不属于这个环境」。换环境那一瞬间行还是上一个环境的，此时打开编辑器会把 A 的值抄进框而保存写进 B——故列表对上之前值相关动作不可用，卡片写「正在切换环境…」。
- 覆盖值只有一个判据：`overrideEditor(listScope, projectId, environmentId, currentValue)` 同时回答「能不能写」与「框里该是哪份文本」，两条入口（`编辑` 与「当前值」摘要）都走同一个 `openEditor`，`setDraft(editor.initialValue)` 每次重置。摘要若只切展开态，框里留的是挂载时那份文本。「恢复自动获取」也读这个 `editor.editable`：它删的是当前生效环境那一格，列表没换过来时删的是别人的。`setDynamicOverride` 成功后同样 `invalidateName(projectId, name)`（写覆盖前发起的刷新会把手写值顶掉），作废范围是这个名在**所有格子**上在飞的那几份，失败原因仍只清当前格。

### 脏标记

`beginCreate` / `beginEdit` / `patch` 把 `dynamicTabId(projectId)` 推进 `dirtyTabIds`，`clearDynamicVariableDraft` 与 `closeTab` 摘掉——标签上那份草稿是**唯一**一份未保存内容（表单不留 state），故「有草稿」=「有未保存改动」，与 endpoint 草稿一创建就标脏同一条约定。不标脏会被 `关闭已保存` 当已保存关掉，`withoutClosedDynamicDrafts` 顺手带走草稿。`closeSavedTabs` 跳过脏标签的过滤保持原样，别为它放宽。

**脏标记只减不重建。** `pruneStaleTabs` 与 `closeAllTabs` 从现有 `dirtyTabIds` 里减去真正被关掉的 id，不按可见标签重建：暂存进 `projectTabs` 的标签不在 `openTabs` 里，重建会连它们的脏标记一起抹掉，切回来 `关闭已保存` 就关掉半张表单。

### 保存收尾按提交的那一份分三种

保存中的标记是 `savingDynamicVariableIds`（按定义 id）：`busy` 是表单 state，卡片换环境就重挂、归零，而 PATCH 还在路上——web 传输下两个 PATCH 可乱序落地，旧的后到就盖掉新编辑，故同一条定义在飞期间再提交 `updateDynamicVariable` 直接返回 `null`（`null` 是「没发出去」，不是失败）。

收尾走 `finishDynamicVariableSave(projectId, submitted, saved)`：

1. 当前草稿仍是 `submitted` → 都落了库，清掉（同 `clearDynamicVariableDraft`）；
2. 草稿已换但 `editingId` 为空或是 `saved.id` → 留着并改写成刚落地那条的编辑态（`creating: false`、`editingId: saved.id`、`openedWith: saved`）；不改写则 `creating` 还挂着，再点保存就是第二次创建；
3. `editingId` 是别的 id → 保存期间用户点开了另一条定义的表单，一个字节都不动。

`openedWith` 必须推进到 `saved`：否则「改成 B 存下、再改回 A」会被 `buildEditPatch` 算成没变而漏发。取消、关标签、删除定义仍走 `clearDynamicVariableDraft` 无条件清。

### 来源选择器

懒加载的跨项目树：`GET /api/nodes/children` 展开时取一层（项目层用 `projectId`，因为模块 `parent_id` 是 NULL），`GET /api/nodes/search` 支撑筛选（250 ms 防抖，返回匹配端点及路径）。**只搜端点**（名称、描述、URL）——集合名不匹配其下端点。跨项目是有意的：A 的定义可取自 B 的端点，取数上下文随之跟着 B。

## 预览取数

用表单里**当前这份草稿**真发一次请求（`dynamic::preview` → `dynamic_api::preview`，薄壳 `dynamic_variables_preview` 与 `POST /api/dynamic-variables/preview`），把响应摊成可点的树（`PreviewJsonTree`），点节点即把路径写进抽取规则。

- **不修改本地持久状态**：不写缓存、覆盖值、环境变量与响应历史。但它**真的发请求**，非幂等接口点两次对端就生效两次。
- **唯一落库的是 OAuth2 令牌**：配了 OAuth2 时取数走真实取数器、经 `oauth2::ensure_token` 拿凭据，没有可用令牌就写一次 `oauth2_tokens`。令牌是发送的凭据而非变量的值。
- **先取数、后抽取**：抽取或脚本失败不让预览失败，正文照给、树照样能点。上限、超时与响应形态与真实取数同一份（走的就是发送那条通路）。
- 环境一致：点名的发它，没点名发调用方此刻选中的那个，与 `dynamicVariablesResolve` 同一个值（`ExtractPreview.tsx` 的 `environmentId: activeEnvironmentId || null`，与 store 的 `callerEnvironmentId` 同源）。
- **点选路径必须能被 `serde_json_path` 解析**：只有 `^[A-Za-z_][A-Za-z0-9_]*$` 的键走点号写法（`$.a.b`），其余（带连字符、数字开头、含中文）走括号写法（`$['a-b']`）——抽不到只是抽不到，解析器拒收等于整条定义存不下。只有一处实现：`src/features/dynamic-variables/extract-path.ts::pathForKey` 与同文件的 `buildScriptSnippet`。

结果**不进 store**（换定义、换环境之后不该仍被当成当前值）。面板标的是**发起时那份请求**的指纹：`preview-input.ts::previewPayload` 把草稿归一成真正发给 `dynamic_variables_preview` 的参数（数据源按形态把另一格归零），指纹只取**来源形态**（端点 id / 内联 JSON）、**抽取规则**与**脚本正文**——**环境不进指纹**，因为一份响应在哪个环境取到只有后端说得准（点名的、继承的、以及调用方环境不属于源接口项目时回落到那儿的默认环境），前端复刻只会错。环境由界面直说：结果旁一直写「环境：X」，与此刻会生效的环境（`previewExpectedEnvironment`：点名的用它，否则调用方当前的，内联则没有）不一致时补一行「这份结果取自 X」。于是「两格都有内容时切换来源形态」也算另一份输入，标「对应的是旧输入」；连点两次只有最后发出的那次上屏。

**失败同样按指纹归属，且多认一档环境**（`PreviewFailure.key` / `previewFailureMessage`）：改过草稿之后那条错误不属于当前这份，对不上就当「还没有结果」。失败另有归属键 `previewFailureKey(指纹, previewExpectedEnvironment(...))`：成功响应自带「环境：X」，失败只有一句原因，少了这档 A 的连接失败会挂在 B 名下。`previewExpectedEnvironment` 算出 `null` 时不提示「环境变了」——多半只是后端回落，误报会诱导用户重发非幂等请求。

两条「不取值」判定也在预览里执行，但**保留正文与抽取结果**：非 2xx 时 `extractError.kind` 是 `http_status`（`message` 带状态码）；序列化后的值超过 256 KiB 时是 `too_large`，两种情况 `value` 为空。`extractError.kind` 的完整集合是 `not_json` / `no_match` / `binary` / `http_status` / `too_large` / `script_error`，对外字符串，不许改名。

## 生成脚本片段

`buildScriptSnippet` 从用户刚点的节点造一段起始模板，**最后一句永远是 `return pm.data;`**：节点在抽取规则下面时用 `pm.data…`，否则用 `pm.response.json()…`。正文点出转换能用三样（`pm.data`、`pm.response.text()` / `pm.response.json()`、刚点的节点），常见变换留成注释。

原样保存必须正好等于「没有脚本」：`transform_script` 为空时后端原样返回抽取结果，而 `return pm.data;` 就是同一个值，故模板里不许藏任何默认变换。

点选路径写进 `//` 注释，故 `escapeKey` 除控制字符外还必须转义 U+2028 / U+2029：JSON 键名里这两个码点合法，JS 引擎却把它们当行终止符，不转义时注释在半截处结束、后半截被当源码。回归用例在 `features/dynamic-variables/__tests__/extract-path.test.ts`（断言没有裸码点且整段能被 `new Function` 解析）。

## 名字扫描

**扫的是这次请求真正会发出去的那份路径。** 路径前缀改写会换掉 URL 的一段：命中时扫改写后的路径，没命中扫原路径。故扫描器（`core::dynamic::list_names` 与 `src/lib/dynamic-vars.ts::listDynamicNames`）**不认识改写规则**，只认调用方算好的那份路径；改写的判定住在调用方（MCP 的 `http_ops/dynamic.rs` 与 `project_ops/endpoint/missing.rs`，界面侧的 `http-tester/model/dynamicVars.ts`）。改写没命中时目标里的标记不参与这次请求，扫了会让那个名字去取一次没人用的数，还可能被权限门拦下来挡住一个本来有效的请求。

## 权限门落在两个项目上

**所属项目**（`db::dynamic_variables::project_id_of`）决定定义归谁：创建、更新、删除、读取、写覆盖值都要它的权限。**源接口所属项目**（`db::dynamic_variables::source_project_id_of`）决定谁付取数的账：取数真用的是那个项目的 base URL 与凭据，故只读项目的客户端不许借别人的定义花自己的凭据。两道门在创建、更新（含 patch 要搬去的**新**所属项目与**新**源项目）与解析时都跑。`dynamic_var_list` 只在给了 `project_id` 时过门；列全部项目没有单一所属项目可查，与 `project_list` 一样交给全局 `CapabilityPolicy`。
