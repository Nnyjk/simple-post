# docker/

容器部署用到的全部文件。部署方式、环境变量与运维步骤见仓库根 `README.md` 的「部署」一节，这里只记目录与构建约束。

| 文件 | 用途 |
| --- | --- |
| `Dockerfile` | 三阶段构建：SPA → `simple-post-server` → 无包运行时（uid 10001，`/app/spa` + `/data`）。 |
| `Dockerfile.dockerignore` | 构建上下文的排除清单（`target/`、`node_modules/`、`dist/` 等）。 |
| `docker-compose.yml` | 单机 Compose；与 `docker stack deploy` 同形。 |
| `docker-swarm.yml` | Swarm 命令行栈：`replicas: 1` + `placement` 约束 + `start-first`。 |
| `docker-portainer.yml` | Portainer 建栈用：`version: "3.8"`、`dnsrr` + `mode: host`、`stop-first`。 |

## 命令（都在仓库根执行）

```bash
docker build -f docker/Dockerfile -t simple-post:local .
docker compose --project-directory . -f docker/docker-compose.yml up -d
docker stack deploy -c docker/docker-swarm.yml simple-post
```

Portainer 是另一种加载方式：把 `docker-portainer.yml` 整段粘进 Stacks → Add stack → Web editor。

## 更新

容器形态不会自己更新：界面只负责提示与给出命令，搬新镜像的那一步在宿主上做。

**版本提示从哪来。** `serve-web` 的 `GET /api/update/check` 抓 `https://api.github.com/repos/Nnyjk/simple-post/releases/latest`，结果存在进程内缓存里 6 小时；`?force=1` 绕过缓存，但同一进程 60 秒内最多强制一次。抓取失败也回 HTTP 200，原因放在响应体的 `error` 字段（`unreachable` / `rate_limited` / `no_release` / `bad_response`），失败的响应不进缓存。口令模式下这三条 `/api/update/*` 都要带 bearer。

**更新动作。** 发现新版本时对话框给出的是宿主上执行的命令：

```bash
docker compose --project-directory . -f docker/docker-compose.yml pull simple-post
docker compose --project-directory . -f docker/docker-compose.yml up -d simple-post
```

`pull` 拉的是 `SP_IMAGE` 指向的那个名字。它的默认值 `simple-post:local` 是本机 `docker build` 的标签，pull 它什么也拉不到，所以先把 `.env` 里的 `SP_IMAGE` 指向仓库镜像（路径全小写），再执行上面两条：

```
SP_IMAGE=ghcr.io/nnyjk/simple-post:latest
```

数据在命名卷里，换镜像不动数据。Portainer 建栈时对应动作是改栈文件里的 `image` 行再 Update the stack。

**可选的就地执行。** 在 `.env` 里写 `SP_UPDATE_COMMAND`（对应容器里的 `SIMPLE_POST_UPDATE_COMMAND`），对话框才会出现「立即执行更新」：

- 两道门同时成立才可执行：变量非空，且设置了 `SP_WEB_PASSWORD`（口令模式）。少一道，`applyEnabled` 就是 `false`，`POST /api/update/apply` 回 403。
- 命令由容器内的 bash 执行，而镜像的运行阶段是 `debian:bookworm-slim`，只有 `simple-post-server` 与那个用 bash `/dev/tcp` 写的健康探针——没有 docker CLI、curl、wget、nc。故命令只能落在 bash 与 coreutils 之内，可接法是给旁车或宿主上的一个 HTTP 接口发一个最小请求（`exec 3<>/dev/tcp/<host>/<port>`），由那一侧执行 `docker compose pull`。
- 命令只来自这个环境变量，请求体不参与。
- 挂宿主 `/var/run/docker.sock` 不会让它成立：镜像里没有 docker 可执行文件；为它装一个 CLI 会推翻「运行阶段不装任何包」。

**离线与限流。** 抓不到 `releases/latest` 时顶栏不出现更新入口；设置 →「关于与更新」写明原因（连不上发布服务器 / GitHub 接口限流 / 仓库里还没有发布版本 / 发布接口返回了读不懂的内容）。换镜像本身不依赖这条检查，离线环境照旧用 `docker pull` 加 `up -d`。

## 三条约束

- **构建上下文必须是仓库根（`.`）**，`-f` 只用来指定 Dockerfile。写成 `docker build docker/` 会让 `COPY` 找不到 `crates/`、`src/`、`package*.json`。
- **ignore 文件叫 `Dockerfile.dockerignore`，不能改名 `.dockerignore`。** Docker 只从上下文根读 `.dockerignore`；BuildKit 另外认「与 Dockerfile 同目录、同名 + `.dockerignore` 后缀」的旁置文件。本 Dockerfile 首行是 `# syntax=docker/dockerfile:1.7`，必须走 BuildKit，所以旁置方案成立。仓库根不再保留副本，避免两份漂移。
- **compose 必须带 `--project-directory .`。** 否则 Compose 把 `docker/` 当项目目录：读不到根目录 `.env`，项目名变成 `docker`，数据卷跟着改名成 `docker_simple-post-data`。
