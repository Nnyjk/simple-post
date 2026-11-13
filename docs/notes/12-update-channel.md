# 12 更新通道

- 适用范围：版本提示从哪来、桌面端怎么自动装、「立即执行更新」在什么部署里成立、离线时界面显示什么。
- 权威来源：`crates/simple-post-core/src/update/{version,check}.rs`、`crates/simple-post-server/src/update_api.rs`、`crates/simple-post-server/src/web_api/routes/update.rs`、`src-tauri/src/commands/update.rs`、`src-tauri/tauri.conf.json`、`.github/workflows/release.yml`、`src/stores/update-slice.ts`、`src/features/update/`、`docker/Dockerfile`、`docker/*.yml`。
- 最后核对：2026-11-13（v0.1.34）
- 前置阅读：01-overview.md、02-architecture.md（双传输不变量）、09-engineering-process.md（发布与部署）

## 结论

1. 「有没有新版本」只有一个数据源：`https://api.github.com/repos/Nnyjk/simple-post/releases/latest`。桌面进程与 `serve-web` 进程各持一份进程内缓存（TTL 6 小时，强制刷新 60 秒内最多一次）。
2. 「谁来装」有两种答案，故命令分成两组：桌面端装安装包（Tauri updater 插件），web 端只提示并给出按部署形态的更新动作。`update_check` 两处都有，`update_deployment` / `update_apply` 只在 `serve-web`。
3. 桌面端显示的「这一版改了什么」来自线上 release 正文，不是 `latest.json` 的 `notes`（那一份是构建期模板，写的是安装说明）。
4. 「立即执行更新」需要两道门同时成立：运维声明了 `SIMPLE_POST_UPDATE_COMMAND`，且服务端跑在口令模式。容器镜像是无包运行时，容器自己执行不了 `docker compose pull`。
5. 抓不到 release 时界面不拦路：顶栏不出现入口，原因写在设置 →「关于与更新」。

## 一条版本信息的两条链路

### 桌面进程

```
Tauri updater 插件 → GET .../releases/latest/download/latest.json → 签名校验 → 下载安装包 → 重启进程
        │
        └─ 插件报出新版 → update_slice 调 update_check → core::update::check → GitHub releases/latest
                                                      → notes / html_url 覆盖对话框里的发布说明
```

- 命令只有一条：`src-tauri/src/commands/update.rs` 的 `update_check(force)`，返回 `UpdateCheck`。抓取失败也返回 `Ok`，原因在 `error` 字段——与 REST 侧同一个约定。
- 判断「有没有新版」的是插件：`tauri.conf.json` 的 `plugins.updater.endpoints` 指向 `releases/latest/download/latest.json`，`pubkey` 校验签名；`bundle.createUpdaterArtifacts: true` 才会产出更新包与 `.sig`。
- 安装与重启：`src/lib/updater.ts` 的 `installDesktopUpdate` 走 `downloadAndInstall`，完成后调 `plugin-process` 的 `relaunch()`；能力由 `capabilities/default.json` 的 `updater:default` 与 `process:allow-restart` 授予。Windows 的 `installMode: passive` 让 NSIS 安装过程不需要逐页交互。
- 启动检查由 `src/lib/update/time.ts` 的 `dueForCheck` 节流：24 小时；记录只在真拿到结论时写（`saveLastUpdateCheckAt`），一次限流不会吃掉接下来一天的自动检查。「稍后再说」把版本号写进 `dismissedVersion`，顶栏入口按版本消失。
- `isUpdaterAvailable()` = 在 Tauri 壳里且不是 `tauri dev`；开发构建没有签名产物，检查只会失败。

### serve-web 进程

```
浏览器 → GET /api/update/check[?force=1]      → update_api::check → core 缓存 → GitHub
       → GET /api/update/deployment           → update_api::deployment（形态 + 镜像 + 命令 + applyEnabled）
       → POST /api/update/apply               → update_api::apply → sh -c <SIMPLE_POST_UPDATE_COMMAND>
```

| 路由 | 鉴权 | 失败形状 |
|---|---|---|
| `GET /api/update/check` | 口令模式下要 bearer | 除鉴权外一律 200，`error` 字段是 `unreachable` / `rate_limited` / `no_release` / `bad_response` |
| `GET /api/update/deployment` | 同上 | 只读环境变量与 `/.dockerenv`，不失败 |
| `POST /api/update/apply` | 同上 | 403 `update_disabled`（两道门没都成立）、409 `update_in_progress`（已有一次在跑） |

`check` 的失败也回 200 是约定，不是疏忽：渲染层的 `invokeCmd` 把非 2xx 变成异常，异常里只剩一句 HTTP 状态，读不出「上游限流」与「这台机器断网」的区别。`apply` 的 403 / 409 相反，它们本身就是「没执行」。

## latest.json 与 release 正文的分工

| 位置 | 谁写 | 内容 | 谁读 |
|---|---|---|---|
| `releases/latest/download/latest.json` | `tauri-action` 构建期写 | 版本号、各平台安装包地址与签名、`notes`、`pub_date` | Tauri updater 插件 |
| release 正文（`body`） | 人工在 draft 上补 | 「这一版的改动」 | `core::update::check` → `UpdateCheck.notes`，两条传输共用 |

`release.yml` 的 `releaseBody` 模板只有安装说明，加上 `### 这一版的改动` 一节占位；tauri-action 把整段抄进 `latest.json` 的 `notes`，所以插件那份 notes 说的是「怎么装」，不是「改了什么」。桌面端因此在新版出现时另调 `update_check` 取线上正文；取不到就退回插件那份（`update-slice.ts::liveReleaseNotes` 捕获一切异常、不写 `updateError`）。

正文读取有两处上限：`NOTES_LIMIT_BYTES = 128 KiB`（按字符边界截断，末尾补一行「发布说明过长，已截断」），以及抓取的 8 秒出站超时。

## `apply` 的两道门

`update_api::apply` 与 `deployment` 用同一条判据：`applyEnabled = password_mode && update_command().is_some()`。两处必须一起成立，否则界面显示「可更新」而按钮回 403，或者反过来。

- **门一：`SIMPLE_POST_UPDATE_COMMAND` 非空白。** 空白串视同未声明——空命令会以 `sh -c ""` 成功返回，看起来像「更新成功」。命令只从环境变量读，**不接受请求体**：接受请求体等于把「执行任意命令」交给调用方。
- **门二：口令模式。** 判据来自 `WebState.password`（`serve-web` 启动时从 `SIMPLE_POST_WEB_PASSWORD` / `--password` 解析），路由把 `state.password.is_some()` 传进去，`update_api` 不重读环境变量。理由：能触发 `apply` 的人就是拿到 bearer 的人，没设口令的部署对它开放 `/api/*` 就等于交出那台机器。

执行细节：同一时刻只准一次（`APPLY_GATE.try_lock()`，第二个请求立刻 409）；命令 120 秒后被终止且 `kill_on_drop` 保证子进程不留在后台；stdout / stderr 各截到 8 KiB；退出码、输出原样返回，服务端不做成败判断。

**容器里能跑什么。** 镜像的运行阶段是 `debian:bookworm-slim`，只 `COPY` 了 `simple-post-server` 与那个用 bash `/dev/tcp` 写的健康探针，进程是 uid 10001，没有 docker CLI、curl、wget、nc。故运维声明的命令只能用 bash 与 coreutils 的能力，例如给旁车或宿主上的一个 HTTP 接口发一个最小请求，由那一侧执行 `docker compose pull`。挂宿主 `/var/run/docker.sock` 不能让 `docker compose pull` 成立：镜像里没有 docker 可执行文件，而为它装一个 CLI 会推翻「运行阶段不装任何包」。

## 两种部署的更新动作

| 部署形态 | 界面给什么 | 实际动作 |
|---|---|---|
| 容器 | `GET /api/update/deployment` 返回 `shape: "docker"`、镜像 `ghcr.io/nnyjk/simple-post:latest`、命令 `docker compose pull simple-post && docker compose up -d simple-post` | 在**宿主**改 `.env` 的 `SP_IMAGE` 并执行 pull / up。`pull` 拉的是 `SP_IMAGE` 指向的名字，默认 `simple-post:local` 是本机构建标签，pull 它什么也拉不到 |
| 源码 / 二进制 | `shape: "binary"`，`image` 与 `command` 都是 `null` | 拉代码重新 `cargo build --release -p simple-post-server`（或换二进制），重启 `serve-web` 进程 |

`shape` 的判定在 `update_api::is_docker`：存在 `/.dockerenv`，或 `SIMPLE_POST_DEPLOYMENT` 被设成 `docker`（供没有 `/.dockerenv` 的运行时使用）。

## 离线与降级

- 抓不到 release：`error` 非空，`latest` / `notes` / `url` 全为 `null`，`available` 为假。顶栏的 `UpdatePill` 因此返回 `null`（不留空槽），对话框不弹。
- 设置 →「关于与更新」（`AboutUpdateCard`）把原因写出来，四类文案在 `src/lib/update/messages.ts`；卡片底部那行「更新方式」按 `desktop / docker / binary` 三种说法之一。
- 缓存只对**可用**结果生效（`CacheState.fresh`），失败不缓存：断网机器不会连着 6 小时报同一次失败。`force` 被节流时返回上一次的答案并标 `cached: true`。
- 发布侧缺 `TAURI_SIGNING_PRIVATE_KEY` 时，`release.yml` 把 `createUpdaterArtifacts` 改成 `false`、删掉 `plugins.updater`，本次不产出更新包与 `latest.json`，应用内自动更新保持不可用。这是刻意的降级，避免整次发布因为签名失败而变红。
- 离线部署的更新不依赖这条检查：改 `SP_IMAGE`、在宿主 `docker pull` 加 `up -d` 照样成立，只是界面不会主动提示。

## 陷阱

- **版本比较切掉 `-` 后缀**（`version.rs::parse`）：`0.1.34-rc1` 与 `0.1.34` 视为同版。代价是 `0.2.0-rc1` 不会判成比 `0.1.99` 新；补偿在别处——`check.rs` 把 `draft` / `prerelease` 的 release 判成 `no_release`，发布流程也不让预发布挂 `releases/latest`。
- **逐段按数字比，缺段补 0**：`1.0 == 1.0.0`；任一段不是纯数字就是 `BadResponse`。不要改成字符串比较，`0.1.9` 与 `0.1.10` 会按字典序判反。
- **缓存是进程级的**：桌面进程与容器进程各一份，互不影响；一个进程里的 6 小时不代表另一个进程也查过。
- **`update_apply` 的端到端只在「运维声明了命令」的部署里成立。** 把界面上的 `docker compose pull …` 复制到容器里执行不会成功；那一条是给宿主执行的。
- **不要在桌面壳里补 `update_deployment` / `update_apply`。** 加之前先回答「这台机器怎么更新」是谁的问题：桌面由插件装安装包，这两条在桌面没有语义（见 `AGENTS.md` 的同名不变量）。
- **发布正文要写在这一版的改动前面。** 对话框显示的就是这段正文，正文只写安装说明时用户看到的是「怎么装」，不是「改了什么」。

## 相关笔记

- [README.md](README.md)：索引与维护规则。
- [02-architecture.md](02-architecture.md)：双传输不变量与 Tauri / server 拆分。
- [09-engineering-process.md](09-engineering-process.md)：发布流程、CI 三道闸、容器部署。
- [11-handover-open-issues.md](11-handover-open-issues.md)：已核实的文档/实现矛盾总表。
- `AGENTS.md` → `update_check` / `update_deployment` / `update_apply` 那一段：硬性不变量。
