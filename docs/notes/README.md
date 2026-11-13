> 适用范围：第一次接手 simple-post 的人或 agent，在 `docs/notes/` 里找入口时先读它
> 权威来源：AGENTS.md、README.md、crates/*/README.md、本目录各篇
> 最后核对：2026-11-13（v0.1.34）
> 前置阅读：无（本文件即入口）

# simple-post 接手认知结构

## 一句话结论

`docs/notes/` 是**导览与判断**，不是第二份规格书：它告诉你「改这里会破什么、该去读哪份权威文件」，凡是硬性运行时契约都以 `AGENTS.md` 与源码为准。读完本目录的 01、02、10、11 四篇，你就知道仓库长什么样、哪条线不能踩、哪几处文档正在说错话；其余八篇（03–09、12）按模块展开。

## 文档地图

| 文件 | 一句话摘要 | 目标读者 |
| --- | --- | --- |
| [README.md](README.md) | 索引、阅读路径、笔记与权威文档的分工、维护规则 | 所有人 |
| [01-overview.md](01-overview.md) | 产品定位、三种交付形态与各自入口、技术栈、目录地图、命令速查 | 新人、要跑起来的人 |
| [02-architecture.md](02-architecture.md) | crate 分层与依赖方向、Tauri/server 拆分、双传输不变量、请求装配双路径、前端分包红线 | 要改任何一侧代码的人 |
| [03-data-model.md](03-data-model.md) | 全表清单与列、单一 `store.db` 与三种形态的目录落点、WAL、`root_id` 反范式化、id 生成、`SCHEMA_VERSION` 与增列/删库策略 | 改库、加字段、改权限门的人 |
| [04-request-pipeline.md](04-request-pipeline.md) | 从草稿到落历史的一条序列（脚本插在哪一步）、两条装配路径、七种请求体形态、鉴权与 multipart、历史快照 | 改请求行为、改预览、查「界面与实发不一致」的人 |
| [05-mcp.md](05-mcp.md) | JSON-RPC 派发、48 个工具、两道授权门、daemon 端口与 `mcp.port`、stdio 桥 | 改 MCP 工具、接 AI 客户端的人 |
| [06-dynamic-and-scripts.md](06-dynamic-and-scripts.md) | 动态变量引擎（合并/缓存/single-flight）与 QuickJS 脚本沙箱、脚本链与回滚 | 改变量取值、写脚本、调沙箱的人 |
| [07-frontend.md](07-frontend.md) | Zustand 切片、三个 surface、两层 `bootstrap()`、标签与草稿、首屏分包 | 改 React 界面的人 |
| [08-interchange-and-parity.md](08-interchange-and-parity.md) | IR 的三条边与诊断、导入导出编排、`fixtures/` 的跨语言回归网 | 改导入导出、改 parity 规则的人 |
| [09-engineering-process.md](09-engineering-process.md) | 本地门禁与 CI 三个 job、测试放置与源码级哨兵、版本发布、worktree 与数据目录 | 提 PR、发版、配环境的人 |
| [10-glossary.md](10-glossary.md) | 中英术语表 + 关键不变量速查卡 | 所有人（卡住时回查） |
| [11-handover-open-issues.md](11-handover-open-issues.md) | 已核实的文档/实现矛盾总表 + 接手核对顺序 + 欠账候选 | 刚接手、要决定先查什么的人 |
| [12-update-channel.md](12-update-channel.md) | 版本信息的来源与两条链路、`latest.json` 与 release 正文的分工、`apply` 的两道门、容器与源码的更新动作、离线降级 | 改更新流程、发版、排查「没有更新提示」的人 |

## 三条阅读路径

**① 15 分钟建立全局观**
[01-overview](01-overview.md) → [02-architecture](02-architecture.md) → [10-glossary](10-glossary.md)。
产出：知道三种形态各自的二进制入口、依赖为什么单向、哪五处必须一起改。

**② 改后端某个模块之前**
[02-architecture](02-architecture.md)（先确认这条命令要不要走双传输）→ [03-data-model](03-data-model.md)（表与 domain 类型）→ 按模块选：[04-request-pipeline](04-request-pipeline.md) / [05-mcp](05-mcp.md) / [06-dynamic-and-scripts](06-dynamic-and-scripts.md) / [08-interchange-and-parity](08-interchange-and-parity.md) → `AGENTS.md` 对应小节。
产出：知道编排层该放哪个 crate、哪些规则要改两份、回归样本在哪。

**③ 改前端某个功能之前**
[07-frontend](07-frontend.md) → [02-architecture](02-architecture.md) 的「前端首屏分包红线」→ [04-request-pipeline](04-request-pipeline.md) 里 `prepareRequest` 那段（界面侧组装规则）→ [10-glossary](10-glossary.md)（`surface` / `dirty tab` / `current environment`）。
产出：知道状态该进哪个 slice、为什么不能顺手 `gsap.from()`、哪些字段必须发给后端。

动手之前先扫一遍 [11-handover-open-issues](11-handover-open-issues.md) 的总表：21 条已核实的文档/注释矛盾都带影响面与建议动作，其中几条（数据目录落点的四处说法与误判删库位置 #20、schema 拒收门的不对称 #7、47/48 工具数 #2）会直接决定你的改法。

## 笔记与三份权威文档的分工

| 载体 | 它回答什么 | 何时改它 |
| --- | --- | --- |
| `docs/notes/`（本目录） | **导览与判断**：这条线是什么、改它会破什么、权威定义在哪 | 结构变了、判断变了、多了一条陷阱 |
| `AGENTS.md` | **硬性运行时契约**：不变量、陷阱、文件地图。agent 不知道就会写坏 | 新增/推翻一条不变量 |
| `README.md` | **面向用户的产品文档**：定位、三种分发形态、部署与运维 | 用户可见的行为或部署方式变了 |
| `crates/*/README.md` | **模块契约**：该 crate 的边界、目录、专属陷阱 | 模块边界或内部结构变了 |

同一句话只应有一处权威。笔记里写「见 `AGENTS.md` §Architecture & Data Flow」而不是复制段落；crate 的专属陷阱（例如 interchange 的三条边）只在 crate README 里展开，笔记只留指针。

## 认知扫描范围与忽略清单

这份结构是「读 `AGENTS.md` 与 `README.md` 的标题与正文 + 实际 `ls`/读源码交叉核对」得来的，**没有**逐行通读 150KB 的 `AGENTS.md`。以下目录被显式排除，原因如下：

| 路径 | 为什么排除 |
| --- | --- |
| `node_modules/` | 第三方包，与项目事实无关 |
| `dist/` | `vite build` 产物，随时可重建 |
| `target/`、`src-tauri/target/` | Cargo 构建产物；workspace 拆分后产物落在根 `target/` |
| `src-tauri/gen/` | Tauri 生成的 schema，`.gitignore` 已忽略 |
| `.git/` | 版本历史，不属于「当前结构」 |
| `.worktrees/` | agent 运行时创建的**完整第二份 checkout**，被 `.gitignore` 与 lint/format ignore 三方排除；扫进去会把别人的工作当成仓库现状 |
| `coverage/` | 本仓库未配置覆盖率工具，目录可能残余 |
| `.scratch/`、`scripts/.scratch/` | gitignored agent 草稿区 |
| `.thinkrail/` | 本地 agent 工具状态（`context/`）；**未被 `.gitignore` 覆盖**，所以只能靠自觉不扫 |
| `docs/screenshots/` | 产品截图，供 `README.md` 引用 |

另有几个「只看一眼」的位置：`crates-guide.html`（一份 crates 全景的可视化页面，非权威）与 `docker/docker-*.yml`（部署形态，权威说明在 `README.md §部署`）。

## 维护规则

1. **什么时候更新**：新增/删除后端命令（02、10）、改表结构或 `SCHEMA_VERSION`（03）、新增 parity 样本（08、10）、CI job 或发布流程变化（09）、`package.json` / 根 `Cargo.toml` 版本号变化（全篇的「最后核对」）。
2. **怎么标日期**：每个文件顶部四行元信息里的「最后核对」写**你实际核对过的日期与版本**，不要继承上一版的日期。只核对了一部分就在正文里对该部分标 `[未核实]`。
3. **不确定就标 `[未核实]`**：宁可留一个显式缺口，也不要写一句读起来像事实的猜测。
4. **文档与代码矛盾**：不要改文档去迁就，也不要改代码；把矛盾记进 [11-handover-open-issues.md](11-handover-open-issues.md) 的总表（编号、位置、双方说法、影响面、建议动作），并注明双方位置。各篇末尾的「文档与实现不一致」保留本模块的上下文，但**总表是唯一索引**——新发现只写在某篇末尾等于没人会再看到它。
5. **不要复制长段**：`AGENTS.md` 的段落用「§小节名」引用；源码用 `路径:行号`。
6. **篇长 60–220 行**：超过说明你在重复权威文档，少于 60 说明还没有判断只有目录。

## 相关笔记

- [01-overview.md](01-overview.md) — 先跑起来
- [10-glossary.md](10-glossary.md) — 术语卡住时回查
