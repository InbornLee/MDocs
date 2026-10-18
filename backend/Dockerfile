# Single-container full-stack image for MDocs.
#
# Requires BuildKit (default in current Docker / buildx) for the RUN --mount
# cache mounts used below.
#
#   stage 1  Node   -> build the Vue 3 SPA
#   stage 2  Rust   -> embed the SPA and compile the release binary
#   stage 3  Debian -> minimal runtime; /data is the only stateful volume
#
# BuildCache: the Rust stage mounts the cargo registry and the target
# directory as BuildKit caches, so changing application sources only
# recompiles the `app` crate instead of every dependency. Requires
# BuildKit (default in modern Docker / buildx).
#
# The SPA is embedded into the binary with rust-embed, so the runtime image
# needs no asset directory and serves the frontend, the REST API and the MCP
# endpoint from the same process and port.
#
# Package sources default to mainland-China mirrors (apt / npm / cargo) for
# fast builds. Pass an empty value to fall back to the upstream mirrors:
#   --build-arg APT_MIRROR=
#   --build-arg NPM_REGISTRY=
#   --build-arg CARGO_REGISTRY_MIRROR=

# ---------- stage 1: frontend ----------
FROM node:24-alpine AS frontend
WORKDIR /fe

# npm registry (npmmirror); empty = registry.npmjs.org
ARG NPM_REGISTRY="https://registry.npmmirror.com"

# dependencies first so the layer survives source-only edits
COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    if [ -n "$NPM_REGISTRY" ]; then npm config set registry "$NPM_REGISTRY"; fi \
    && npm ci

COPY frontend/ ./
# the repo default outDir points into the (not yet existing) backend tree,
# so build to a neutral path for the copy step below
RUN npx vite build --outDir /out --emptyOutDir

# ---------- stage 2: backend ----------
FROM rust:1.97-slim AS backend
WORKDIR /app

# crates.io index (rsproxy); empty = crates.io
ARG CARGO_REGISTRY_MIRROR="sparse+https://rsproxy.cn/index/"

# 国内镜像偶发抖动时多重试几次，避免一次超时就整层重建。
ENV CARGO_NET_RETRY=5 \
    CARGO_HTTP_TIMEOUT=60

RUN if [ -n "$CARGO_REGISTRY_MIRROR" ]; then \
        printf '[source.crates-io]\nreplace-with = "mirror"\n\n[source.mirror]\nregistry = "%s"\n' \
            "$CARGO_REGISTRY_MIRROR" >> "${CARGO_HOME:-/usr/local/cargo}/config.toml"; \
    fi

COPY backend/ ./
# rust-embed picks the SPA up at compile time
COPY --from=frontend /out ./crates/app/frontend-dist

# Dependency artifacts live in the BuildKit cache mounts, so a source-only
# edit reuses every compiled dependency and only rebuilds `app`. Cache mount
# contents are not part of the image layer, so the binary is copied out
# inside the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release -p app --locked \
    && cp target/release/mdocs /usr/local/bin/mdocs

# ---------- stage 3: runtime ----------
FROM debian:bookworm-slim

# Debian package mirror (aliyun); empty = deb.debian.org
ARG APT_MIRROR="http://mirrors.aliyun.com"

# bookworm ships deb822-style sources; rewrite both shapes when present
RUN if [ -n "$APT_MIRROR" ]; then \
        sed -i \
            -e "s|http://deb.debian.org/debian-security|${APT_MIRROR}/debian-security|g" \
            -e "s|http://security.debian.org/debian-security|${APT_MIRROR}/debian-security|g" \
            -e "s|http://deb.debian.org/debian|${APT_MIRROR}/debian|g" \
            /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list 2>/dev/null || true; \
    fi \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=backend /usr/local/bin/mdocs /usr/local/bin/mdocs

# All mutable state (SQLite database + uploaded avatars) lives under /data.
ENV APP_LISTEN=0.0.0.0:8080 \
    APP_DATA_DIR=/data \
    RUST_LOG=info

RUN mkdir -p /data
VOLUME ["/data"]
EXPOSE 8080

# Runs as root by default so a freshly mounted, root-owned volume is writable.
# Mount with --user "$(id -u):$(id -g)" or chown the volume if you prefer.
ENTRYPOINT ["/usr/local/bin/mdocs"]
