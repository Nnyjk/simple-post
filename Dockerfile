# syntax=docker/dockerfile:1.7

# =============================================================================
# simple-post — container image (issue #17)
#
# Three stages: SPA (Node) → server binary (Rust) → slim runtime.
#
# The runtime stage installs **nothing**. That is the whole point of the
# `crates/simple-post-server` split: the desktop binary links Tauri, so a
# Linux build of it drags in the entire GTK / WebKit / X11 stack, while
# `simple-post-server` is Tauri-free and links:
#
#   * SQLite   — `rusqlite` with the `bundled` feature (static, C-compiled)
#   * TLS      — `reqwest` 0.12 `rustls-tls` + webpki-roots (bundled Mozilla
#                roots), so no system OpenSSL and no ca-certificates store
#
# If you ever find yourself adding `libwebkit2gtk-4.1` / `libgtk-3` /
# `libssl-dev` here, the split has been undone — read README 「容器部署」
# before you do it.
#
# The health probe therefore cannot use `curl` (installing it would pull
# libssl3 / libcurl4 in as a *probe* dependency, not an app one), so the
# runtime writes a ~6 line bash probe over `/dev/tcp` instead. Same probe
# is referenced by `docker-compose.yml` / `docker-swarm.yml`.
# =============================================================================


# -----------------------------------------------------------------------------
# Stage 1 — SPA bundle (`vite build` → dist/)
# -----------------------------------------------------------------------------
FROM node:22-bookworm-slim AS spa

WORKDIR /app

# Lockfile first: `npm ci` only invalidates its layer when the manifests
# actually change, so a source-only edit does not re-download 400 packages.
COPY package.json package-lock.json ./

# `--include=dev` is required, not optional: the build toolchain itself
# (typescript / vite / tailwind / @vitejs/plugin-react) lives in
# devDependencies, and an image build inherits NODE_ENV=production from some
# base images / CI runners. Mirrors what ci.yml does for the same reason.
RUN npm ci --include=dev

# Only what `tsc -b` and `vite build` read. tsconfig.json has no project
# references (`include: ["src", "vite.config.ts"]`), so this is the whole
# dependency set — no scripts/, no eslint config.
COPY index.html vite.config.ts tsconfig.json tailwind.config.js postcss.config.js ./
COPY src ./src
COPY public ./public

# `npm run build` is `tsc -b && vite build`, so a type error fails the image
# build rather than shipping a bundle that 404s at runtime.
RUN npm run build


# -----------------------------------------------------------------------------
# Stage 2 — `simple-post-server` release binary
# -----------------------------------------------------------------------------
FROM rust:bookworm AS rust-build

WORKDIR /src

# The whole workspace has to be present even though only one member is
# compiled: cargo fails to load a workspace whose members are missing, and
# `-p simple-post-server` skips *building* the others. `src-tauri` is copied
# for that reason alone — its GTK build dependencies are never invoked,
# which is why this stage needs no `libwebkit2gtk-4.1-dev`.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src-tauri ./src-tauri

# Registry + git caches survive across builds; `target` is a cache mount too,
# so an incremental rebuild is ~seconds instead of a full LTO pass. The cache
# mount is invisible to the image filesystem, hence the explicit copy-out to
# /out — without it the `COPY --from` below would find an empty directory.
#
# No `--profile` / no `-C` overrides: `[profile.release]` in the workspace
# root (`panic = "abort"`, `lto`, `opt-level = "s"`, `codegen-units = 1`,
# `strip`) is what makes this image small, and overriding it here would
# silently diverge the binary from every other release build.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    mkdir -p /out \
 && cargo build --release -p simple-post-server \
 && cp /src/target/release/simple-post-server /out/simple-post-server


# -----------------------------------------------------------------------------
# Stage 3 — runtime (no package manager ever runs here)
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="simple-post" \
      org.opencontainers.image.description="API client / debugger with a built-in MCP server (serve-web shape)" \
      org.opencontainers.image.source="https://github.com/Nnyjk/simple-post" \
      org.opencontainers.image.licenses="MIT"

# Non-root. `--no-create-home` because /data below is the home and is
# created explicitly, owned by this uid.
RUN groupadd --gid 10001 simple-post \
 && useradd --uid 10001 --gid 10001 --no-create-home \
            --home-dir /data --shell /usr/sbin/nologin simple-post \
 && mkdir -p /data /app/spa \
 && chown -R 10001:10001 /data /app/spa

COPY --from=rust-build /out/simple-post-server /usr/local/bin/simple-post-server

# The SPA dir must contain the *contents* of the Vite `dist/` tree — an
# `index.html` at its root plus `assets/` — which is what
# `static_files::spa_index` and the fallback handler expect.
COPY --from=spa /app/dist /app/spa

# Unauthenticated health probe. `/healthz` is on the always-public bypass list
# (it has to be — the container runtime sends no Authorization header).
# Implemented with bash's /dev/tcp so the runtime stage stays package-free.
#
# The path is prefix-aware on purpose. `build_router` nests the **entire**
# router under `SIMPLE_POST_PUBLIC_BASE_PATH`, `/healthz` included — so a
# bare `GET /healthz` returns 404 in exactly the deployment that sets a base
# path, and a hardcoded probe would mark that container unhealthy forever.
# The prefix is read from the container's own environment, which is the same
# source the entrypoint's flag defaults come from, so the two cannot drift.
RUN printf '%s\n' \
      '#!/bin/bash' \
      '# Probe <prefix>/healthz over a raw socket: no curl/wget/nc in this image,' \
      '# and installing one just to answer a healthcheck would put runtime' \
      '# deps back into a stage whose entire premise is having none.' \
      'prefix="${SIMPLE_POST_PUBLIC_BASE_PATH:-}"' \
      'prefix="${prefix%/}"' \
      'path="${prefix}/healthz"' \
      '[ -n "$prefix" ] || path="/healthz"' \
      'exec 3<>/dev/tcp/127.0.0.1/4173 || exit 1' \
      'printf "GET %s HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n" "$path" >&3' \
      '# Read the whole response, then require BOTH the status line and the' \
      '# health JSON. Grepping the socket twice does not work: the first' \
      '# `grep -q` buffers and closes fd 3, so the second read can miss the' \
      '# body. A `200` alone is also not enough - the SPA fallback answers' \
      '# 200 text/html for an unrouted path, which is how a broken probe' \
      '# once reported a healthy container.' \
      'response="$(cat <&3)"' \
      'case "$response" in' \
      '  *"200 OK"*) ;;' \
      '  *) echo "probe got no 200: ${response:0:120}" >&2; exit 1 ;;' \
      'esac' \
      'case "$response" in' \
      '  *\"status\":\"ok\"*) exit 0 ;;' \
      '  *) echo "probe got no health JSON: ${response:0:120}" >&2; exit 1 ;;' \
      'esac' \
      > /usr/local/bin/simple-post-healthcheck \
 && chmod +x /usr/local/bin/simple-post-healthcheck

# Data dir. Declared as an ENV so a bare `docker run` (no compose file, no
# swarm) still writes inside the mounted /data rather than to /root/.local.
ENV SIMPLE_POST_DATA_DIR=/data

WORKDIR /app

# 4173 是管理平台 + REST（主端口上也有 `/mcp`）。
#
# **MCP 专用端口不是这里定的**，而是设置面板「连接 → HTTP 服务配置」里的那个
# 端口：那一栏写 0 时 MCP 只走主端口，写了具体端口时服务端会另开一个只服务 MCP
# 的监听器（那个端口上没有 REST、没有 SPA）。容器里要把这个端口发布出去，
# 发布的目标端口必须与那一栏写的数字一致。
EXPOSE 4173
VOLUME ["/data"]

# start_period covers the first boot's `ensure_current_schema` + seed, which
# runs before the listener binds.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/usr/local/bin/simple-post-healthcheck"]

# Drop privileges LAST. Every instruction above runs as root on purpose: the
# user/group creation, the chowns, and writing the probe into
# /usr/local/bin all need it.
#
# Without this line the image created uid 10001, chowned /data to it, and then
# cheerfully ran as root anyway — so the CI assertion `id -u == 10001` was the
# only thing standing between a pass and a silent contradiction of the image's
# own comment. Switch here so the assertion can pass for the right reason.
USER simple-post:simple-post

# `--bind` must be 0.0.0.0 here: the binary defaults to 127.0.0.1, which
# inside a container is unreachable from outside it. `--spa-dir` /
# `--data-dir` are passed explicitly rather than left to their
# executable-relative / OS-default resolution, so the image keeps working if
# the layout above is ever reshuffled.
# `--mcp-auto-start` is intentionally omitted: omitting it honours whatever
# the persisted preference says, which is what a desktop-synced store wants.
ENTRYPOINT ["/usr/local/bin/simple-post-server"]
CMD ["serve-web", \
     "--bind", "0.0.0.0", \
     "--port", "4173", \
     "--spa-dir", "/app/spa", \
     "--data-dir", "/data"]
