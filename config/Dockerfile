# syntax=docker/dockerfile:1
#
# aio-config image: the build/host-side scenario configurator (TUI + gen). NOT
# part of the running sandbox stack - it runs via `docker run` from the Makefile
# (`make config`, `make build-base`'s gen step). Multi-stage: build the Rust
# binary in a rust:1-bookworm stage, copy into a slim runtime image.
#
# This build fetches crates from crates.io, so it must run on an ONLINE build
# machine (same constraint as sandbox-base). Offline machines `docker load` the
# pre-built image; they never build this.

FROM rust:1-bookworm AS build
WORKDIR /src
# Copy the manifest + lock + sources and build. Cargo.lock is committed (binary
# crate, like app/) for reproducible builds; --locked fails the build if the lock
# is out of sync with Cargo.toml.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
# Minimal runtime: the tool only does file IO (gen) and terminal IO (tui). No
# network, no locale niceties needed - crossterm raw-mode TTY works as-is.
COPY --from=build /src/target/release/aio-config /usr/local/bin/aio-config
ENTRYPOINT ["aio-config"]
