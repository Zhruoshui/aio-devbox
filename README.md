# AIO-style Dev Sandbox

A self-hosted remote development environment: `docker compose up` starts a caddy
gateway plus pluggable service containers, and a web UI presents a workspace of
panes (code-server, VNC/Chromium, terminal, opencode). Personal use.

This repository is being built phase by phase (see
`.trellis/tasks/07-28-server-mvp/implement.md`). **Phase A** delivers the base
image, the compose skeleton, the caddy gateway (basicauth), and a minimal axum
app returning `ok` on `/`.

## Architecture (Phase A)

```
host :8080 -> caddy gateway (basicauth) --reverse_proxy--> app (axum) :8088
                                                            USER gem (uid 1000)
                                                            volume: /home/gem
```

- `sandbox-base` (Dockerfile.base): Debian slim + node 20 + python3 + git +
  user `gem` (uid 1000). Shared base image for `app`, `code-server`,
  and (later) `vnc`. (opencode moved to the L4 `opencode` scenario.)
- `app` (app/Dockerfile, multi-stage): axum server, `FROM sandbox-base`.
- `gateway` (caddy:2): basicauth + reverse proxy to `app:8088`.

## Build order

`app/Dockerfile` is `FROM sandbox-base`, so `sandbox-base` must be built and
tagged **before** `app`. The Makefile handles this:

```sh
make build-base   # docker build -t sandbox-base -f Dockerfile.base .
make build        # build-base, then docker compose build (builds app)
make up           # build-base, ensure auth hash, then docker compose up -d --build
```

`docker compose --profile build build base` also tags `sandbox-base` (the `base`
service is gated behind the `build` profile so it never starts at runtime).

## Auth

The gateway uses caddy `basicauth` (user `admin`, password `admin` by default).
The bcrypt hash contains `$` characters, which this docker-compose build
corrupts when passed through `env_file`/`environment` (it interpolates `$VAR`
patterns inside env values). The hash is therefore generated to
`gateway/secrets/hash` (gitignored) and delivered to caddy via
`gateway/entrypoint.sh`, which exports it before exec'ing caddy. The Caddyfile
still uses the `{$SANDBOX_PASSWORD_HASH}` placeholder as designed.

```sh
make hash              # generate hash for password "admin" (default)
make hash PASS=secret  # custom password
```

## Run (Phase A)

```sh
make up
curl -u admin:admin http://localhost:8080/   # -> ok (200)
curl http://localhost:8080/                  # -> 401
make down
```

## Scenario presets (build-time toolchains)

Dev environments are organized into **profile layers**. Each scenario is a
build-time Dockerfile fragment baked into `sandbox-base`, tagged with a
`category` so the TUI can group scenarios by layer:

| Layer | `category` | What lives here | Selectable? |
|-------|-----------|------------------|-------------|
| L1 OS packages | `os` | apt packages, node 20, user `gem` | no - hardcoded in `Dockerfile.base.head` |
| L2 Shell conveniences | `shell` | CLI tools (fzf/rg/bat/fd) | yes |
| L3 Language toolchains | `lang` | rust / go / python-dev (toolchain + LSP/formatter/linter) | yes |
| L4 Applications | `app` | CLI apps / AI agent CLIs (opencode) | yes |
| L5 External services | `service` | _(future, not yet implemented)_ | - |

L1 is the foundation every compose service `FROM sandbox-base` inherits (curl
for L3, node for code-server, build-essential for compilers), so it stays in
`Dockerfile.base.head` and is **never selectable** - unchecking it would break
derived containers. L2-L4 are the selectable personal preferences.

Current scenarios:

- `shell-utils` (L2 - `shell`): fzf / ripgrep / bat / fd-find (with Debian
  `bat`->`batcat`, `fd`->`fdfind` symlinks into `/usr/local/bin` so the
  conventional names work). Tools only - no shell aliases (alias coverage is
  inconsistent across login/non-login shells; this layer ships binaries that
  work in every shell PATH).
- `rust` (L3 - `lang`): rustup stable + rustfmt + clippy + rust-analyzer,
  installed under `/opt/rust` (avoids the workspace volume masking
  `/home/gem`), with proxies symlinked into `/usr/local/bin` so every shell
  finds `cargo`/`rustc` regardless of PATH.
- `python-dev` (L3 - `lang`): Python development enhancements.
- `go` (L3 - `lang`): Go toolchain.
- `opencode` (L4 - `app`): opencode AI agent CLI (GitHub release binary to `/usr/local/bin`). Moved out of L1/head into L4 (AI agents belong in L4). **MVP caveat:** the Web opencode pane is gated by `ENABLE_OPENCODE` (compose, hardcoded `true`), decoupled from whether opencode is baked - so disabling this scenario leaves a dead pane ("command not found") until the future "auto-detect L4 agents -> Web buttons" feature.

`make config` opens a TUI (ratatui) that lists scenarios **grouped by layer**
(L2 - Shell / L3 - Language / ...). Space toggles, `s` saves the selection to
`.aio/enabled.toml`. `make build-base` then runs `aio-config gen`, which
assembles `Dockerfile.base` from `Dockerfile.base.head` + the enabled
`scenarios/<id>/fragment.Dockerfile` files (layer order: by `category` then id) +
`Dockerfile.base.tail`, and builds `sandbox-base`. A chosen toolchain is baked
into the image at build time - after `make up` it is ready with no manual
install.

```sh
make config                       # TUI: pick scenarios (grouped by layer) -> .aio/enabled.toml
make up                           # gen + build sandbox-base + compose up
docker exec aio-app-1 bash -lc 'cargo --version'   # rust ready, no manual install
```

Adding a scenario = drop `scenarios/<id>/{scenario.toml,fragment.Dockerfile}`
and set `category` in `scenario.toml` (`os`/`shell`/`lang`/`app`/`service`;
defaults to `lang` for back-compat) - no change to the configurator. Scenario
tools install to **system paths** (`/opt`, `/usr/local`, `/etc/profile.d`) as
root before `USER gem`, never `/home/gem/*` (the workspace named volume masks
it). Changing the selection rebuilds the image (the `docker save`/`load`
offline path is unchanged).

> **Rebuild after reselecting.** `make up` rebuilds the `sandbox-base` image but
> does not recreate already-running containers. After changing the selection,
> run `make down && make up` (or `docker compose up -d --force-recreate`) so
> `app`/`code-server` pick up the new base image.

Offline: build on an online machine, `docker save` the images, `docker load` on
the offline machine, then `make up NOBUILD=1` (skips `build-base`/gen). The
`aio-config` image itself also fetches crates from crates.io at build time, so
it is built online and loaded offline like the rest.

## Layout

```
Dockerfile.base        sandbox-base image (generated: head + scenarios + tail)
Dockerfile.base.head   sandbox-base head (root bootstrap: apt/node/user)
Dockerfile.base.tail   sandbox-base tail (USER gem + WORKDIR)
scenarios/             scenario library, layered by category; <id>/{scenario.toml,fragment.Dockerfile}
config/                aio-config crate (Rust): TUI picker + Dockerfile.base generator
app/                   axum app (Cargo.toml, src/main.rs, Dockerfile)
gateway/               Caddyfile + entrypoint.sh (+ secrets/hash, generated)
docker-compose.yml     gateway + app (+ base under build profile)
Makefile               build-config / config / build-base / build / up / hash / down / clean
.env / .env.example    SANDBOX_USER (hash is generated, not env-delivered)
```
