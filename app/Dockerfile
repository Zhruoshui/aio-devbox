# syntax=docker/dockerfile:1
#
# Multi-stage build for the axum app server + React SPA.
#
# IMPORTANT: the build context is the REPO ROOT (docker-compose sets
# `context: .` + `dockerfile: app/Dockerfile`), so COPY paths are prefixed with
# `app/` and `web/`. This lets one Dockerfile reach both the Rust source and
# the frontend source.
#
# The runtime stage is `FROM sandbox-base`, so the `sandbox-base` image must be
# built and tagged first. Use `make build` / `make up` (which run
# `make build-base` first), or `docker compose --profile build build base`.

# Global build arg: lets CI's buildx (docker-container driver, which cannot see
# local daemon images) point the base-derived stages at a GHCR-hosted
# sandbox-base. Defaults to the local `sandbox-base`, so local builds are
# unchanged. Declared before the first FROM so every FROM below sees it; not
# used in any RUN, so no per-stage re-declaration is needed.
ARG BASE_IMAGE=sandbox-base

# --- Builder: compile the Rust release binary on the official Rust image. ---
FROM rust:1-bookworm AS builder

WORKDIR /app

# Force the sparse registry index (faster, and the git-protocol clone path is
# unnecessary since index.crates.io is reachable here).
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

# Dep-caching layer: copy manifests first and build a dummy binary so that
# source changes don't re-fetch/recompile the dependency graph.
COPY app/Cargo.toml app/Cargo.lock ./
RUN mkdir src && echo 'fn main() { println!("dummy"); }' > src/main.rs \
 && cargo build --release \
 && rm -rf src

# Real source. `touch` defeats BuildKit's COPY mtime: COPY pins file mtimes to
# the context/build time which cargo can read as "older than the fingerprint"
# written by the dummy build, causing it to skip recompiling the bin target and
# ship the dummy binary. touch forces cargo to notice the changed source.
#
# services.toml is baked into the binary via include_str! in config.rs, so it
# must be present at compile time. Copied here (not in the dep-cache layer) so
# editing it doesn't force a full dependency recompile.
COPY app/src ./src
COPY app/services.toml ./
RUN touch src/main.rs && cargo build --release

# --- web-builder: compile the React SPA with Vite (sandbox-base has node 20). ---
FROM ${BASE_IMAGE} AS web-builder
WORKDIR /web
# Lockfile-first for a deterministic, cacheable install.
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
# `npm run build` runs `tsc --noEmit && vite build`; output lands in /web/dist.
RUN npm run build

# --- Runtime: derive from the shared dev-env base image. ---
# Carries node / python / git / opencode because Phase E/F spawn pty shells
# (terminal, opencode) inside this container.
FROM ${BASE_IMAGE}

COPY --from=builder /app/target/release/aio-app /usr/local/bin/aio-app

# Entrypoint: autostarts pi-web (if the pi-web scenario baked it into base)
# before exec'ing the CMD. See app/entrypoint.sh for the why. --chmod because
# the source file carries no exec bit on the host; COPY --chmod sets it at
# build time without a separate RUN chmod.
COPY --chmod=755 app/entrypoint.sh /usr/local/bin/aio-entrypoint.sh

# Static SPA tree (React build output from the web-builder stage).
# Lives under /app (not /root, which is the persistent workspace volume).
COPY --from=web-builder /web/dist /app/static

USER root
EXPOSE 8088
ENTRYPOINT ["/usr/local/bin/aio-entrypoint.sh"]
CMD ["aio-app"]
