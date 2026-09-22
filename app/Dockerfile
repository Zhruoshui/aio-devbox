# syntax=docker/dockerfile:1
#
# Multi-stage build for the axum app server + static redirect page.
#
# IMPORTANT: the build context is the REPO ROOT (docker-compose sets
# `context: .` + `dockerfile: app/Dockerfile`), so COPY paths are prefixed
# with `app/`. This lets one Dockerfile reach the Rust source and the static
# page in one context.
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

# Dep-caching layer: the repo is a cargo WORKSPACE (sandbox-mgr Phase 0), so
# cargo needs the root manifest + the shared root Cargo.lock + every member
# manifest to resolve. Dummy sources make the members buildable without their
# real code; `--locked` pins the dep graph to the committed lock (fails loudly
# when a member Cargo.toml changes without regenerating the lock - same
# contract as config/Dockerfile).
COPY Cargo.toml Cargo.lock ./
COPY app/Cargo.toml app/Cargo.toml
COPY config/Cargo.toml config/Cargo.toml
COPY aio-models/Cargo.toml aio-models/Cargo.toml
COPY mgr/Cargo.toml mgr/Cargo.toml
RUN mkdir -p app/src aio-models/src config/src mgr/src \
 && echo '' > aio-models/src/lib.rs \
 && echo '' > config/src/lib.rs \
 && echo 'fn main() {}' > config/src/main.rs \
 && echo 'fn main() {}' > mgr/src/main.rs \
 && echo 'fn main() { println!("dummy"); }' > app/src/main.rs \
 && cargo build --release --locked -p aio-app \
 && rm -rf app/src aio-models/src

# Real source. `touch` defeats BuildKit's COPY mtime: COPY pins file mtimes to
# the context/build time which cargo can read as "older than the fingerprint"
# written by the dummy build, causing it to skip recompiling the bin target and
# ship the dummy binary. touch forces cargo to notice the changed source. Both
# app/src (dummy main) and aio-models/src (dummy empty lib) need it — the
# fingerprint-vs-mtime trap hits every member whose sources were dummied in
# the dep-cache layer.
# `-p aio-app` targets this member in the shared workspace (mgr placeholder is
# a bin-only crate and app does not depend on it).
#
# services.toml is baked into the binary via include_str! in config.rs, so it
# must be present at compile time. Copied here (not in the dep-cache layer) so
# editing it doesn't force a full dependency recompile.
COPY app/src ./app/src
COPY aio-models/src ./aio-models/src
COPY app/services.toml ./app/
RUN touch app/src/main.rs aio-models/src/lib.rs \
 && cargo build --release --locked -p aio-app

# --- Runtime: derive from the shared dev-env base image. ---
# Carries node / python / git / opencode because Phase E/F spawn pty shells
# (terminal, opencode) inside this container.
FROM ${BASE_IMAGE}

COPY --from=builder /app/target/release/aio-app /usr/local/bin/aio-app

# Entrypoint: autostarts pi-web (if the pi-web scenario baked it into base)
# and patches the static redirect target (MGR_URL set) before exec'ing the
# CMD. See app/entrypoint.sh for the why. --chmod because the source file
# carries no exec bit on the host; COPY --chmod sets it at build time
# without a separate RUN chmod.
COPY --chmod=755 app/entrypoint.sh /usr/local/bin/aio-entrypoint.sh

# mise volume seeder (issue: runtime `mise use` was lost on recreate). The
# entrypoint runs it at boot; it symlinks the baked /opt/mise trees into the
# workspace volume and regenerates the volume's config.toml so user-installed
# tools persist. See the script's header for the full rationale.
COPY --chmod=755 app/aio-mise-volume.sh /usr/local/bin/aio-mise-volume

# Static redirect page (unified Phase 6, D1): web/ — the per-sandbox
# workbench SPA — is retired; "/" now serves a minimal page that bounces to
# the Sandbox manager (MGR_URL set, patched by the entrypoint) or explains
# where the UI went (stock stacks). ALL /api/* routes are unchanged and
# remain the surface mgr-web's proxy consumes.
# Lives under /app (not /root, which is the persistent workspace volume).
COPY app/redirect/ /app/static/

USER root
EXPOSE 8088
ENTRYPOINT ["/usr/local/bin/aio-entrypoint.sh"]
CMD ["aio-app"]
