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

## 三条约束

- **构建上下文必须是仓库根（`.`）**，`-f` 只用来指定 Dockerfile。写成 `docker build docker/` 会让 `COPY` 找不到 `crates/`、`src/`、`package*.json`。
- **ignore 文件叫 `Dockerfile.dockerignore`，不能改名 `.dockerignore`。** Docker 只从上下文根读 `.dockerignore`；BuildKit 另外认「与 Dockerfile 同目录、同名 + `.dockerignore` 后缀」的旁置文件。本 Dockerfile 首行是 `# syntax=docker/dockerfile:1.7`，必须走 BuildKit，所以旁置方案成立。仓库根不再保留副本，避免两份漂移。
- **compose 必须带 `--project-directory .`。** 否则 Compose 把 `docker/` 当项目目录：读不到根目录 `.env`，项目名变成 `docker`，数据卷跟着改名成 `docker_simple-post-data`。
