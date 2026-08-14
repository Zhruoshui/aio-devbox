[English](README.md) · [中文](README.zh-CN.md)

# AIO Dev Sandbox

A self-hosted, all-in-one remote development environment. One `docker compose up`
brings up a Caddy gateway plus pluggable service containers, and a browser-served
tiling workspace presents your panes: VSCode in the browser (code-server),
Chromium over VNC, a terminal, and an opencode AI-agent pane. Personal project.

Toolchains (Node, Python, Rust, Go, nvm, uv, …) are **build-time scenario
presets** — you pick them (and Node/Python versions) in a TUI, and they are baked
into a shared `sandbox-base` image that every dev container inherits. The whole
stack is **offline-capable**: build on an online machine, `docker save`/`load`
the images, and run air-gapped.

## Features

- **One command to a browser IDE.** `make up` → open `http://localhost:8080`
  (HTTP basic auth). A golden-layout tiling workspace arranges your panes.
- **Pluggable panes, auto-detected.** Web panes (code-server, VNC) are gated by
  compose profiles and the UI hides a pane whose container isn't running — start
  only what you need. Agent panes (terminal, opencode) run over an xterm.js +
  WebSocket pty bridge inside the `app` container.
- **Build-time scenario presets.** A Rust TUI (`aio-config`) lists scenarios
  grouped by layer; the selection is assembled into `Dockerfile.base` and built
  into `sandbox-base`. No per-container Dockerfiles for toolchains.
- **Versioned base runtimes.** Node (18 / 20 / 22) and CPython (3.11 / 3.12 /
  3.13) are `always_on` scenarios with a version dropdown — pick a version, not
  whether to install.
- **Survives container recreate.** The workspace is a named Docker volume on
  `/home/gem`; runtime version managers (nvm, uv) install into the volume so
  `nvm install` / `uv python install` survive `down`/`up`.
- **Offline-ready.** Build online, ship images via `docker save`/`load`, run with
  `make up NOBUILD=1`. A full offline tool-install handbook lives in
  [`docs/offline-install-guide.md`](docs/offline-install-guide.md).

## Quick start

```sh
make hash                              # gateway password (default: admin)
make config                            # (optional) TUI: pick scenarios + Node/Python versions
make up PROFILES="code-server vnc"     # start with web panes (terminal+opencode always on)
# → open http://localhost:8080   (admin / admin)
```

With no `PROFILES`, only the always-on services (`gateway` + `app`) start, so the
workspace shows the terminal and opencode panes; add `code-server` / `vnc` profiles
to light up the browser-IDE and Chromium panes.

```sh
make down                              # stop (keeps images + workspace volume)
make logs                              # tail logs
make clean                             # stop, drop the volume, remove built images
```

## Architecture

```
                         ┌──────────────────────────────────────────────┐
   browser :8080 ───────► │  gateway   (caddy:2)                          │
   admin:admin            │  basicauth + reverse_proxy                    │
                         └──────┬───────────────┬───────────────┬────────┘
                                │ /             │ /code-server/  │ /vnc/
                                ▼               ▼                ▼
                          ┌──────────┐   ┌──────────────┐  ┌────────────┐
                          │ app      │   │ code-server  │  │ vnc       │
                          │ axum +   │   │ :8200        │  │ :6080     │
                          │ React    │   │ profile:     │  │ profile:  │
                          │ SPA      │   │ code-server  │  │ vnc       │
                          │ :8088    │   └──────┬───────┘  └─────┬──────┘
                          └────┬─────┘          │                │
                 /api/term/ws  │ pty (uid 1000)  │                │
                 /api/manifest │                │                │
                               ▼                ▼                ▼
                  ┌──────────────────────────────────────────────────────┐
                  │  shared named volume  aio_workspace  →  /home/gem    │
                  │  (uid 1000, user "gem")  — mounted by all three      │
                  └──────────────────────────────────────────────────────┘

   build-only (never runs at runtime):  base  →  sandbox-base
        app/Dockerfile  and  code-server/Dockerfile  are  FROM sandbox-base
        vnc/Dockerfile  is  FROM debian:bookworm-slim  (decoupled — pure browser surface)
```

| Container | Image | Role |
|---|---|---|
| `gateway` | `caddy:2` | HTTP basic auth + reverse proxy to `app`, `code-server`, `vnc`. Serves the WS upgrades too. |
| `app` | `sandbox-app` (built) | Axum server: serves the React SPA, `GET /api/manifest` (which panes are live), and the `/api/term/ws` pty WebSocket bridge. `FROM sandbox-base`. |
| `code-server` | `sandbox-code-server` (built) | VSCode in the browser. Profile-gated (`--profile code-server`), auto-detected by TCP probe to `code-server:8200`. `FROM sandbox-base`. |
| `vnc` | `sandbox-vnc` (built) | Xvnc + Chromium + noVNC web client. Profile-gated (`--profile vnc`), auto-detected by TCP probe to `vnc:6080`. `FROM debian:bookworm-slim` (decoupled from `sandbox-base`). `shm_size 2gb` for Chromium. |
| `base` | `sandbox-base` (built) | The shared base image. Gated behind the `build` profile so it **never** starts as a runtime container. |

**Build order matters:** `app` and `code-server` are `FROM sandbox-base`, so
`sandbox-base` must be built and tagged first. The Makefile handles this
(`make up` → `build-base` → `compose up --build`).

## Scenario presets

Dev environments are organized into **profile layers**. Each scenario is a
build-time Dockerfile fragment baked into `sandbox-base`, tagged with a `category`
so the TUI groups scenarios by layer:

| Layer | `category` | What lives here | Selectable? |
|---|---|---|---|
| L1 OS packages | `os` | non-versioned infra (apt, ca-certs, build-essential, user `gem`) in `Dockerfile.base.head`; **versioned runtimes Node + Python** as `always_on` scenarios | infra: hardcoded; node/python: version-selectable, always on |
| L2 Shell conveniences | `shell` | CLI tools (fzf / rg / bat / fd) | yes |
| L3 Language toolchains | `lang` | rust / go / python-dev + version managers nvm / uv | yes |
| L4 Applications | `app` | CLI apps / AI-agent CLIs (opencode) | yes |
| L5 External services | `service` | _(future, not yet implemented)_ | — |

L1 has two parts. The **non-versioned infra** (HTTPS apt, ca-certs self-bootstrap,
build-essential, user `gem`) stays hardcoded in `Dockerfile.base.head` and never
reaches the TUI — it's the foundation every `FROM sandbox-base` service inherits.
The **versioned runtimes** Node + Python are `always_on` scenarios: always baked
(code-server and the app web-builder depend on Node), shown in the TUI as locked
rows `[*]` with a version `[label]` cycled by **Left/Right** — you pick a version,
not whether to install. L2–L4 are normal toggleable preferences.

Current scenarios:

| Scenario | Layer | `always_on` | Versions | Installs to |
|---|---|---|---|---|
| `node` | L1 `os` | ✓ | 20.18.0 / 22.11.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 `os` | ✓ | 3.12.7 / 3.11.10 / 3.13.0 | python-build-standalone → `/usr/local` |
| `shell-utils` | L2 `shell` | — | — | fzf / ripgrep / bat / fd → `/usr/local/bin` (Debian `bat`→`batcat`, `fd`→`fdfind` symlinks) |
| `rust` | L3 `lang` | — | — | rustup stable + rustfmt + clippy + rust-analyzer → `/opt/rust`, proxies → `/usr/local/bin` |
| `python-dev` | L3 `lang` | — | — | uv + ruff → `/usr/local/bin` (overlaps with `uv` — enable one) |
| `go` | L3 `lang` | — | — | Go 1.23 tarball → `/usr/local/go` |
| `nvm` | L3 `lang` | — | — | nvm.sh → `/opt/nvm`; runtime `NVM_DIR=~/.nvm` (on the volume) so `nvm install` survives recreate. Login shells only. |
| `uv` | L3 `lang` | — | — | uv → `/usr/local/bin`; runtime `uv python install` → volume (overlaps with `python-dev`) |
| `opencode` | L4 `app` | — | — | opencode AI-agent CLI → `/usr/local/bin`. Web pane gated by `ENABLE_OPENCODE` (default `true`). |

**Workflow.** `make config` opens the TUI (ratatui): scenarios are listed grouped
by layer. Toggle selectable scenarios with **Space**; L1 `always_on` rows show
`[*]` with a version `[label]` cycled by **Left/Right** (can't be unchecked). `s`
saves the selection (scenario ids + version labels) to `.aio/enabled.toml`.
`make build-base` then runs `aio-config gen`, which assembles `Dockerfile.base`
from `Dockerfile.base.head` + the `always_on` L1 runtimes + the enabled
`scenarios/<id>/fragment.Dockerfile` files (ordered by `category` then id) +
`Dockerfile.base.tail`, substituting the selected version's `{{version}}`/`{{tag}}`
into versioned fragments, and builds `sandbox-base`.

```sh
make config                       # TUI: pick scenarios + L1 versions → .aio/enabled.toml
make up                           # gen + build sandbox-base + compose up
docker exec aio-app-1 bash -lc 'node --version; python3 --version'   # L1 runtimes ready
```

**Adding a scenario** = drop `scenarios/<id>/{scenario.toml,fragment.Dockerfile}`
and set `category` in `scenario.toml`. For a versioned scenario, add `always_on`
(if always baked), `default_version`, and a `[[versions]]` array (each entry:
`label` for the dropdown + extra keys substituted into `{{key}}` placeholders in
the fragment). Defaults: `category="lang"`, `always_on=false`, no versions — no
change to the configurator. Scenario tools install to **system paths** (`/opt`,
`/usr/local`, `/etc/profile.d`) as root before `USER gem`, never `/home/gem/*`
(the workspace named volume would mask it). Changing the selection rebuilds the
image (the `docker save`/`load` offline path is unchanged).

> **Rebuild after reselecting.** `make up` rebuilds the `sandbox-base` image but
> does not recreate already-running containers. After changing the selection, run
> `make down && make up` (or `docker compose up -d --force-recreate`) so `app` /
> `code-server` pick up the new base image.

## Configuration

### Makefile targets

| Target | What it does |
|---|---|
| `make config` | Interactive TUI picker → writes `.aio/enabled.toml`. |
| `make gen` | Assemble `Dockerfile.base` from head + enabled fragments + tail. (Internal; run by `build-base`.) |
| `make build-base` | `gen` + `docker build -t sandbox-base -f Dockerfile.base .` |
| `make build` | `build-base` + `docker compose build` |
| `make up [PROFILES=…]` | `build-base` (or skip with `NOBUILD=1`) + `compose up -d --build` |
| `make hash [PASS=…]` | Generate the gateway bcrypt hash for password `PASS` (default `admin`). |
| `make down [PROFILES=…]` | Stop the stack (keeps images and the workspace volume). |
| `make restart` / `make logs` | Restart / tail logs. |
| `make clean` | Destructive: `down -v` + remove built images. |

Pass optional services as space-separated profiles: `make up PROFILES="code-server vnc"`.
With no `PROFILES`, only the always-on services (`gateway` + `app`) start.
`NOBUILD=1` skips `build-base` / `gen` / `--build` — for offline machines that
`docker load` pre-built images instead of building.

### Auth

The gateway uses Caddy `basicauth` (user `admin`, password `admin` by default;
set the user with `SANDBOX_USER` in `.env`). The bcrypt hash contains `$`
characters, which docker-compose corrupts when passed through `env_file` /
`environment` (it interpolates `$VAR` patterns inside env values). The hash is
therefore generated to `gateway/secrets/hash` (gitignored) and delivered to Caddy
via `gateway/entrypoint.sh`, which exports it before exec'ing Caddy. The
Caddyfile still uses the `{$SANDBOX_PASSWORD_HASH}` placeholder as designed.

```sh
make hash              # generate hash for password "admin" (default)
make hash PASS=secret  # custom password
```

## Offline install

Build on an online machine, `docker save` the images, `docker load` on the
offline machine, then `make up NOBUILD=1` (skips `build-base` / `gen`). The
`aio-config` image also fetches crates from crates.io at build time, so it is
built online and loaded offline like the rest.

For the full handbook — how to add arbitrary tools/packages to a running offline
stack without rebuilding or networking (7 tested recipes: static binaries, npm
globals, apt debs, cargo crates, rust toolchains, python+uv, scripts) — see
[`docs/offline-install-guide.md`](docs/offline-install-guide.md).

## Project layout

```
Dockerfile.base          sandbox-base image (generated: head + scenarios + tail)
Dockerfile.base.head     sandbox-base head (root bootstrap: apt/user gem; no language runtimes)
Dockerfile.base.tail     sandbox-base tail (USER gem + WORKDIR)
scenarios/               scenario library, layered by category; <id>/{scenario.toml,fragment.Dockerfile}
config/                  aio-config crate (Rust): TUI picker + Dockerfile.base generator
app/                     axum app (Cargo.toml, src/, Dockerfile, services.toml)
  └ services.toml        single source of truth for workspace panes (id/type/target/url/enable/cmd)
web/                     React SPA (Vite + TS + golden-layout + xterm.js), baked into the app image
gateway/                 Caddyfile + entrypoint.sh (+ secrets/hash, generated)
vnc/                     Xvnc + Chromium + noVNC (FROM debian:bookworm-slim)
code-server/             VSCode-in-browser image (FROM sandbox-base)
docker-compose.yml       gateway + app + code-server + vnc + base (build profile)
Makefile                 build-config / config / gen / build-base / build / up / hash / down / clean
.env / .env.example      SANDBOX_USER (hash is generated, not env-delivered)
docs/                    offline-install-guide.md (+ offline-tool-install.md test log)
.aio/enabled.toml        scenario selection (written by `make config`, read by `gen`)
```

## Status

Built phase by phase. The MVP is complete: gateway + app (axum + React SPA) +
code-server + vnc, the scenario-preset system with four layers and versioned L1
runtimes, and offline support. Not yet done: L5 external services, and
auto-detecting baked L4 agents (e.g. opencode) to drive Web buttons — the
opencode Web pane is currently hardcoded on via `ENABLE_OPENCODE`.
