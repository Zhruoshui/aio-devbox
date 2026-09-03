[English](README.md) · [中文](README.zh-CN.md)

# AIO Dev Sandbox

A self-hosted, all-in-one remote development environment. One `docker compose up`
brings up a Caddy gateway plus pluggable service containers, and a browser-served
workspace presents them behind a collapsible sidebar of buttons: VSCode in the
browser (code-server), Chromium over VNC, a terminal, on-demand AI-agent TUIs
(opencode, pi), and a model-configuration page — each button opens a tab, and
only capabilities actually present in the image get a button. Personal project.

Toolchains (Node, Python, Rust, Go, …) are **build-time scenario presets** — you
pick them (and Node/Python versions) in a TUI, and they are baked into a shared
`sandbox-base` image that every dev container inherits. The whole stack is
**offline-capable**: build on an online machine, ship the images, run air-gapped.

📖 Wiki: [project wiki](https://github.com/Zhruoshui/aio-devbox/wiki) —
architecture overview, scenario presets, offline bundle, and FAQ (skeleton
now, filled in progressively).

## Features

- **One command to a browser IDE.** `make up` → open `http://localhost:8080`
  (HTTP basic auth). A collapsible left sidebar lists your buttons; every click
  launches a NEW instance as a tab (terminal opens by default), and tabs can be
  dragged into split/tiled layouts (golden-layout) or closed via their ✕.
- **Pluggable buttons, auto-detected — three types.**
  - `web` (code-server, VNC): gated by compose profiles — no running container,
    no button (TCP-probe from the app).
  - `agent` (terminal, opencode, pi): appear only when the command actually
    exists on the login-shell PATH; launch on click in an xterm.js + WebSocket
    pty bridge inside the `app` container (multiple instances allowed).
  - `page` (模型配置 / model config): a native React pane, always enabled —
    unified provider/model configuration for the baked-in agent CLIs (edit
    config, import from pi, apply per agent, usage stats).

  No dead panes.
- **Register your own buttons.** The sidebar's `+` form registers a
  "terminal + command" button (persisted in `/root/.aio/buttons.toml` on the
  workspace volume via `POST/DELETE /api/buttons`), surviving container recreate.
  It also registers **web-type buttons** pointing at any dev server port you
  started in a terminal (vite, `python -m http.server`, …) — the button
  appears when the port is listening and opens the dev server in an iframe
  via the app's `/preview/<port>/` reverse proxy (WebSocket / SSE friendly).
- **Build-time scenario presets.** A Rust TUI (`aio-config`) lists scenarios
  grouped by layer; the selection is assembled into `Dockerfile.base` (a
  generated file, not in git) and built into `sandbox-base`. No per-container
  Dockerfiles for toolchains.
- **Versioned base runtimes.** Node and CPython are `always_on` scenarios with
  a version dropdown — pick a version, not whether to install.
- **Survives container recreate.** The workspace is a named Docker volume on
  `/root`; runtime user data (projects, configs, `~/.local/bin` tools) lives
  on the volume and survives `down`/`up`. Note: runtime `mise use` in a
  deployed sandbox lands on the container writable layer and is lost on
  recreate (known tradeoff — offline whole-dir transfer is the supported
  path, see `docs/offline-tool-install.md` §14).
- **Offline-ready.** Build online, ship via `make save` → `make load` (or plain
  `docker save`/`load`), run with `make up NOBUILD=1`. A full offline
  tool-install handbook lives in
  [`docs/offline-install-guide.md`](docs/offline-install-guide.md).

## Quick start

```sh
make hash                              # gateway password (default: admin)
make config                            # (optional) TUI: pick scenarios + Node/Python versions
make up PROFILES="code-server vnc"     # start with web buttons (terminal always on)
# → open http://localhost:8080   (admin / admin)
```

With no `PROFILES`, only the always-on services (`gateway` + `app`) start, so
the sidebar shows the terminal and model-config buttons (plus any baked-in
agent TUI); add `code-server` / `vnc` profiles to light up the browser-IDE and
Chromium buttons.

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
                          │ :30141 ← │ (pi-web, iframe pane, published to host)
                          └────┬─────┘          │                │
                               │                ▼                ▼
                 /api/term/ws  │ pty (root)  ┌────────────────────────────┐
                 /api/manifest │             │  aio_workspace  →  /root   │
                 /api/models/* │             │  (root, uid 0) shared vol   │
                 /api/buttons  │────────────►│  mounted by all three      │
                               │             └────────────────────────────┘

   build-only (never runs at runtime):  base  →  sandbox-base
        app/Dockerfile  and  code-server/Dockerfile  are  FROM sandbox-base
        vnc/Dockerfile  is  FROM debian:bookworm-slim  (decoupled — pure browser surface)
```

| Container | Image | Role |
|---|---|---|
| `gateway` | `caddy:2` | HTTP basic auth + reverse proxy to `app`, `code-server`, `vnc`. Serves the WS upgrades too. |
| `app` | `sandbox-app` (built) | Axum server: React SPA, `GET /api/manifest` (live buttons), `/api/term/ws` pty WebSocket bridge, `POST/DELETE /api/buttons` (user buttons), `/api/models/*` (model-config page), `/api/stats`, and the `/preview/<port>/` dev-server reverse proxy. Autostarts pi-web on `:30141` when baked. `FROM sandbox-base`. |
| `code-server` | `sandbox-code-server` (built) | VSCode in the browser. Profile-gated, auto-detected by TCP probe to `app:8200`. `FROM sandbox-base`. |
| `vnc` | `sandbox-vnc` (built) | Xvnc + Chromium + noVNC web client. Profile-gated, auto-detected by TCP probe to `app:6080`. `FROM debian:bookworm-slim` (decoupled from `sandbox-base`). `shm_size 2gb` for Chromium. |
| `base` | `sandbox-base` (built) | The shared base image. Gated behind the `build` profile so it **never** starts as a runtime container. |

**Shared network namespace:** `code-server` and `vnc` join app's network stack
via `network_mode: "service:app"` (their own sandbox-net DNS names no longer
exist — everything on the shared stack is reached as `app:PORT`). Chromium in
the VNC pane therefore reaches dev servers started in the workbench or
code-server terminals at `http://localhost:<port>` (same loopback, no
HTTPS-first upgrade). Reserved ports on the shared netns: `8088` (axum),
`8200` (code-server), `6080` (websockify), `5900` (Xvnc, loopback), `30141`
(pi-web, published to the host) — pick other ports for dev servers.

**pi-web host port:** the iframe URL carries the *host-side* publish port
(`http://<host>:<PI_WEB_HOST_PORT>/`), which is `30141` by default but differs
when the host republishes it (an `sbx` sandbox mapping it to another host
port, or a second instance reusing the port range). Set `PI_WEB_HOST_PORT`
(via `.env` or `PI_WEB_HOST_PORT=30142 make up`) and compose both publishes
that port and tells the app to render the iframe URL with it; unset = `30141`
(current behavior). **Pairing note (sbx):** the browser reaches the sandbox
only via `sbx ports`, and the iframe URL uses `PI_WEB_HOST_PORT` verbatim —
so the sbx publish must use the SAME port on both sides. With the variable
set, compose binds `PI_WEB_HOST_PORT` *inside the sandbox* too (`30142` →
container `30141`), so: `sbx ports <sandbox> --publish 30142:30142/tcp` —
not `:30141` (nothing listens on 30141 in the sandbox once the variable is
set). The in-container port is always `30141` — liveness probes and the
autostart are unaffected.

**Build order matters:** `app` and `code-server` are `FROM sandbox-base`, so
`sandbox-base` must be built and tagged first. The Makefile handles this
(`make up` → `build-base` → `compose up --build`).

## Scenario presets

Dev environments are organized into **profile layers**. Each scenario is a
build-time Dockerfile fragment baked into `sandbox-base`, tagged with a
`category` so the TUI groups scenarios by layer:

| Layer | `category` | What lives here | Selectable? |
|---|---|---|---|
| L1 OS packages | `os` | non-versioned infra (apt, ca-certs, build-essential, fonts) in `Dockerfile.base.head` or fixed fragments; **versioned runtimes Node + Python** as `always_on` scenarios | infra: hardcoded; node/python: version-selectable, always on |
| L2 Shell conveniences | `shell` | CLI tools (fzf / rg / bat / fd) | yes |
| L3 Language toolchains | `lang` | mise (rust + go + uv + ruff + opencode, all-in-one) / c23 | yes |
| L4 Applications | `app` | CLI apps / AI-agent CLIs (opencode, pi, pi-web) | yes |
| L5 External services | `service` | _(future, not yet implemented)_ | — |

The L1 **non-versioned infra** (HTTPS apt, ca-certs self-bootstrap,
build-essential) stays hardcoded in `Dockerfile.base.head` and never reaches
the TUI — it's the foundation every `FROM sandbox-base` service inherits. The
**versioned runtimes** Node + Python are `always_on` scenarios: always baked
(code-server and the app web-builder depend on Node), shown in the TUI as
locked rows `[*]` whose version `[label]` cycles with **Left/Right**. L2–L4
are normal toggleable preferences.

Current scenarios (all install to **system paths** — `/opt`, `/usr/local`,
`/etc/profile.d` — never `/root/*`, which the shared workspace volume would
mask):

| Scenario | Layer | `always_on` | Versions | Installs to |
|---|---|---|---|---|
| `node` | L1 `os` | ✓ | 22.23.2 *(default)* / 22.11.0 / 20.18.0 / 18.20.4 | nodejs.org tarball → `/usr/local` |
| `python` | L1 `os` | ✓ | 3.12.7 *(default)* / 3.13.0 / 3.11.10 | python-build-standalone → `/usr/local` |
| `fonts` | L1 `os` | — | — | Maple Mono NF CN (mono + Nerd Font icons + CJK, ~78MB) → `/usr/local/share/fonts`, aliased as default for mono/sans/serif via `/etc/fonts/local.conf`; fixes tofu in server-side rendering |
| `shell-utils` | L2 `shell` | — | — | fzf / ripgrep / bat / fd → `/usr/local/bin` (Debian `bat`→`batcat`, `fd`→`fdfind` symlinks) |
| `c23` | L3 `lang` | — | — | clang-22 (apt.llvm.org, full C23) + gcc-12 reuse + gdb / cmake / ninja / valgrind / cppcheck / strace; unversioned symlinks → `/usr/local/bin` |
| `mise` | L3 `lang` | — | — | mise (L3 toolchain manager) bakes rust + go + uv + ruff + opencode into `/opt/mise` (all five, all-or-nothing, ~1.5GB); versions via the fragment's ARG block; visibility = ENV shims PATH + `/etc/profile.d/mise.sh` activate |
| `opencode` | L4 `app` | — | — | (baked by the `mise` scenario) opencode AI-agent CLI via mise shims. Sidebar button only when baked (command-exists detection). |
| `pi` | L4 `app` | — | — | pi coding agent → `/usr/local/bin`; extensions baked to `/opt/pi-extensions` and registered offline into `~/.pi` (volume) by running `aio-pi-extensions` once in a terminal |
| `pi-web` | L4 `app` | — | — | pi Web UI (npm global; needs node ≥ 22.19); autostarted by app's entrypoint on `:30141`, embedded as an iframe pane via a published port (Next.js root-absolute assets rule out a gateway subpath) |

**Workflow.** `make config` opens the TUI (ratatui): scenarios grouped by
layer; toggle with **Space**, cycle `always_on` versions with **Left/Right**,
`s` saves to `.aio/enabled.toml`. `make build-base` then runs `aio-config gen`,
which assembles `Dockerfile.base` from `Dockerfile.base.head` + the `always_on`
L1 runtimes + the enabled `scenarios/<id>/fragment.Dockerfile` files (ordered
by `category` then id) + `Dockerfile.base.tail`, substituting the selected
version's `{{version}}`/`{{tag}}` into versioned fragments, and builds
`sandbox-base`.

```sh
make config                       # TUI: pick scenarios + L1 versions → .aio/enabled.toml
make up                           # gen + build sandbox-base + compose up
docker exec aio-app-1 bash -lc 'node --version; python3 --version'   # L1 runtimes ready
```

**Presets & wildcard.** `.aio/presets/{minimal,full}.toml` are ready-made
selections: `minimal` = always_on baseline only (`scenarios = []`), `full` =
`scenarios = ["*"]`, which `gen` expands to every discovered non-`always_on`
scenario (so new scenarios are auto-included). CI copies a preset to
`.aio/enabled.toml` to build the two GHCR variants; the wildcard must be the
only element (`["*", "mise"]` is an error).

**Adding a scenario** = drop `scenarios/<id>/{scenario.toml,fragment.Dockerfile}`
and set `category` in `scenario.toml`. For a versioned scenario, add `always_on`
(if always baked), `default_version`, and a `[[versions]]` array (each entry:
`label` for the dropdown + extra keys substituted into `{{key}}` placeholders in
the fragment). Defaults: `category="lang"`, `always_on=false`, no versions — no
change to the configurator. Changing the selection rebuilds the image (the
offline path is unchanged).

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
| `make save` / `make load` | Offline bundle: `save` packs images + `.env` + gateway hash + selection into `aio-offline-bundle/`; `load` restores them on the offline machine. |
| `make clean` | Destructive: `down -v` + remove built images. |
| `make pull [VARIANT=…]` | Pull prebuilt images from GHCR + retag to local compose names (see below). |

Internal helpers: `build-config` (builds the `aio-config` image), `ensure-hash`
(writes the default-password hash if missing; run by `up` / `pull`).

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

```sh
# online machine
make save                                  # → aio-offline-bundle/: images.tar + env + hash + enabled.toml
# offline machine (ship the bundle over)
make load                                  # restore images + .env + hash + selection
make up NOBUILD=1 PROFILES="code-server vnc"
```

If the `pi` scenario is baked, run `aio-pi-extensions` once in a terminal after
first start to register the baked extensions into `~/.pi`. The `aio-config`
image also fetches crates from crates.io at build time, so it is built online
and loaded offline like the rest.

For the full handbook — how to add arbitrary tools/packages to a running offline
stack without rebuilding or networking (7 tested recipes: static binaries, npm
globals, apt debs, cargo crates, rust toolchains, python+uv, scripts) — see
[`docs/offline-install-guide.md`](docs/offline-install-guide.md).

## Prebuilt image install (GitHub Actions)

Don't want to build the images locally? Every push to `main` (and every `v*`
tag) is built by GitHub Actions and published to GitHub Container Registry
(GHCR). Two variants of the base-derived images are available, plus a single
`sandbox-vnc`:

- `minimal` — the bare always-on baseline (Node + Python), nothing else.
- `full` — every scenario fragment baked in (mise [rust/go/uv/ruff/opencode] / c23 / pi / …).

```sh
make pull VARIANT=full           # pull + retag to local names (default: full)
make up NOBUILD=1 PROFILES="code-server vnc"   # start without building
# → open http://localhost:8080   (admin / admin)
```

`make pull` fetches `sandbox-base` / `sandbox-app` / `sandbox-code-server` at
`:minimal` or `:full`, plus `sandbox-vnc:latest`, retags them to the local
compose names, and prepares the two gitignored host files the stack needs
(`.env` from the example, and the gateway password hash for the default password
`admin`). It never touches `.aio/enabled.toml` — a pure consumer doesn't care
about the scenario selection, and `make up NOBUILD=1` skips `gen`.

Point the pull at your registry with `REGISTRY_PREFIX` (defaults to this
repo's GHCR namespace, `ghcr.io/zhruoshui`; override it to pull from a fork)
and pick a leaner set with `VARIANT=minimal`. If your machine has no registry
access at all, use the offline path above (`make save` / `make load`).

## Project layout

```
Dockerfile.base          sandbox-base image (GENERATED by `make gen`, not in git)
Dockerfile.base.head     sandbox-base head (root bootstrap: apt; no language runtimes)
Dockerfile.base.tail     sandbox-base tail (USER root + WORKDIR /root)
scenarios/               scenario library, layered by category; <id>/{scenario.toml,fragment.Dockerfile}
config/                  aio-config crate (Rust): TUI picker + Dockerfile.base generator
app/                     axum app (Cargo.toml, src/, Dockerfile, services.toml)
  └ services.toml        built-in workspace buttons (id/type/target/url/label/cmd)
web/                     React SPA (Vite + TS + sidebar/tab-stack + xterm.js), baked into the app image
gateway/                 Caddyfile + entrypoint.sh (+ secrets/hash, generated)
vnc/                     Xvnc + Chromium + noVNC (FROM debian:bookworm-slim)
code-server/             VSCode-in-browser image (FROM sandbox-base)
docker-compose.yml       gateway + app + code-server + vnc + base (build profile)
Makefile                 config / gen / build-base / up / hash / save / load / pull / clean
.env / .env.example      SANDBOX_USER (hash is generated, not env-delivered)
docs/                    offline-install-guide.md (+ offline-tool-install.md test log)
.aio/enabled.toml        scenario selection (written by `make config`, read by `gen`)
.aio/presets/            minimal.toml / full.toml — CI presets (`["*"]` wildcard = all)
aio-offline-bundle/      output of `make save` (gitignored)
```

## Status

Built phase by phase. The MVP is complete: gateway + app (axum + React SPA) +
code-server + vnc, the scenario-preset system with four layers and versioned L1
runtimes, offline support, the sidebar-button workspace (auto-detected
web/agent/page buttons, user-registered agent and web buttons with dev-server
port preview, unified model config), and the pi / pi-web agent stack. Not yet
done: L5 external services beyond on-demand TUI buttons, and multi-instance
terminals.

### Dev server preview (`/preview/<port>/`)

Register a web-type button (sidebar `+` → type "Web port preview", enter the
port your dev server listens on) and click it to open the server in an iframe.
The app reverse-proxies `/preview/<port>/*` to `127.0.0.1:<port>` on the shared
network namespace, so servers started in ANY workbench/code-server terminal are
reachable — including ones bound to loopback only. The button shows when the
port has a listener (TCP probe, same semantics as the built-in buttons) and
hides when it doesn't. WebSocket (vite HMR) and SSE streams pass through
unbuffered.

Two known boundaries:

- **Root-absolute asset URLs break under any subpath.** Apps that emit
  `/_next/...`-style URLs cannot sit behind `/preview/<port>/` (same reason
  pi-web needs a dedicated origin). The proxy does no HTML rewriting.
- **vite needs two config lines** to run under the subpath — in
  `vite.config.ts`:
  ```ts
  export default defineConfig({
    base: '/preview/5173/',
    server: { hmr: { path: '/preview/5173/' } },
  });
  ```
  Servers emitting purely relative URLs (`python -m http.server`, most static
  previews) work with zero configuration.
