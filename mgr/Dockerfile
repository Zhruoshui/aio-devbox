# syntax=docker/dockerfile:1
#
# aio-mgr: the sandbox-mgr control plane (multi-sandbox lifecycle manager).
#
# Build context is the REPO ROOT (mgr/compose.yml sets `context: ..` /
# `dockerfile: mgr/Dockerfile`), mirroring app/Dockerfile, because mgr is a
# member of the root cargo workspace and depends on the aio-config and
# aio-models path crates.
#
# Unlike app, mgr has no scenario-derived base image - it is a plain
# self-contained axum server, so the official Rust image is also the natural
# runtime (nothing from sandbox-base is needed inside the container; docker
# itself is reached via the mounted host socket, not a docker CLI).
#
# Phase 3 adds a web-builder stage for the mgr-web management SPA (same
# pattern as app/Dockerfile's web-builder, but mgr does not build FROM
# sandbox-base, so it pulls node:20-bookworm-slim explicitly). The dist tree
# is baked to /app/static and served by the axum process (main.rs MGR_WEB_DIR
# default in-container value).

FROM rust:1-bookworm AS builder

WORKDIR /app

# Sparse registry (same rationale as app/Dockerfile).
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Dep-cache layer: root manifest + lock + every member manifest with dummy
# sources (same contract as app/Dockerfile; `--locked` pins the shared lock).
# mgr-web does not affect this layer: it is an npm project, not a cargo
# member, so no dummy sources for it here. Since Phase 4 mgr depends on the
# aio-models lib too (canonical model-config store), so its dummy sources
# come along (sandbox-mgr-ops.md contract 5: dep-cache must dummy EVERY
# workspace member, real-source touch list must follow).
COPY Cargo.toml Cargo.lock ./
COPY app/Cargo.toml app/Cargo.toml
COPY config/Cargo.toml config/Cargo.toml
COPY aio-models/Cargo.toml aio-models/Cargo.toml
COPY mgr/Cargo.toml mgr/Cargo.toml
RUN mkdir -p app/src aio-models/src config/src mgr/src \
 && echo '' > aio-models/src/lib.rs \
 && echo '' > config/src/lib.rs \
 && echo 'fn main() {}' > config/src/main.rs \
 && echo 'fn main() {}' > app/src/main.rs \
 && echo 'fn main() {}' > mgr/src/main.rs \
 && cargo build --release --locked -p aio-mgr \
 && rm -rf mgr/src

# Real source. mgr depends on the aio-config path crate's lib (scenario
# catalog + gen assembly) AND the aio-models lib (canonical model config,
# Phase 4), so both crates' real sources come along here too (the
# dep-cache layer had only the dummy empty libs). touch defeats BuildKit's
# COPY-mtime trap (see app/Dockerfile) for every file the dummy layer
# touched - COPY pins old context mtimes, which cargo can read as "older
# than the dummy build's fingerprint", skipping the recompile and shipping
# the dummy lib. lib.rs especially: it IS the lib target's unit fingerprint.
COPY config/src ./config/src
COPY aio-models/src ./aio-models/src
COPY mgr/src ./mgr/src
RUN touch config/src/lib.rs config/src/main.rs aio-models/src/lib.rs mgr/src/main.rs \
 && cargo build --release --locked -p aio-mgr

# --- web-builder: compile the mgr-web management SPA with Vite (Phase 3). ---
# mgr has no sandbox-base, so pull node explicitly (same image the app's
# web-builder inherits through sandbox-base). Lockfile-first for a
# deterministic, cacheable npm install; `npm run build` runs
# `tsc --noEmit && vite build` (typecheck is a build gate) into /mgr-web/dist.
FROM node:20-bookworm-slim AS web-builder
WORKDIR /mgr-web
COPY mgr-web/package.json mgr-web/package-lock.json ./
RUN npm ci
COPY mgr-web/ ./
RUN npm run build

# --- Runtime: slim bookworm + docker CLI (client only).
# mgr shells out to `docker ...` / `docker compose ...` as subprocesses (D2:
# CLI, never the Docker API), so the client AND its compose/buildx plugins
# must exist in the image; the ENGINE is the host daemon reached through the
# mounted /var/run/docker.sock.
#
# The CLI comes from the official docker:cli image, not Debian's docker.io
# package: bookworm's docker.io is an ancient CLI with NO compose/buildx
# plugins (mgr would die on its first `docker compose ps`), and dragging the
# full engine package just to delete the daemon is waste. docker:cli binaries
# are static Go builds - copying them onto glibc bookworm works.
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY --from=docker:cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker:cli /usr/local/libexec/docker/cli-plugins/docker-compose \
                     /usr/local/libexec/docker/cli-plugins/docker-compose
COPY --from=docker:cli /usr/local/libexec/docker/cli-plugins/docker-buildx \
                     /usr/local/libexec/docker/cli-plugins/docker-buildx

COPY --from=builder /app/target/release/aio-mgr /usr/local/bin/aio-mgr

# mgr-web SPA tree (React build output from the web-builder stage). Matches
# the in-container MGR_WEB_DIR default of /app/static set below.
COPY --from=web-builder /mgr-web/dist /app/static

# main.rs defaults MGR_WEB_DIR to <MGR_REPO>/mgr-web/dist; the repo mount in
# the containerized form has no dist (it is .dockerignore'd + built in-image),
# so pin the baked path explicitly.
ENV MGR_WEB_DIR=/app/static

EXPOSE 8089
ENTRYPOINT ["/usr/local/bin/aio-mgr"]
