#!/bin/sh
# docker/healthcheck.sh — invoked by Dockerfile.web's HEALTHCHECK
# directive every 30s. Exits 0 when healthy, non-zero otherwise.
#
# We hit `/api/healthz` (public, no auth — see src-tauri/src/auth.rs).
# The body is JSON: {"status":"ok","version":"…","shape":"server"}.
# `grep -q '"status":"ok"'` matches the JSON fragment regardless of
# field order. We don't use `curl` because Alpine's curl pulls in a
# bigger footprint than `wget` for this trivial use.
#
# Port matches the Dockerfile's EXPOSE / ENV SIMPLE_POST_WEB_PORT.
# Override via the healthcheck arg if you change the port:
#   HEALTHCHECK CMD /usr/local/bin/healthcheck.sh 8080

PORT="${1:-${SIMPLE_POST_WEB_PORT:-4173}}"
URL="http://127.0.0.1:${PORT}/api/healthz"

# `wget --spider` does a HEAD and exits 0 on 200/3xx. We then do a
# full GET with body to confirm the JSON actually parses — `wget -qO-`
# is silent and prints the body, which `grep -q` consumes.
#
# 5s is the Dockerfile's --timeout; we use a slightly smaller in-script
# timeout so wget itself doesn't outlast the watchdog.
if ! wget -qO- --timeout=3 --tries=1 "${URL}" 2>/dev/null \
        | grep -q '"status":"ok"'; then
    # Stderr is captured by `docker inspect` — keep it short.
    echo "healthcheck: ${URL} did not return ok" >&2
    exit 1
fi

exit 0