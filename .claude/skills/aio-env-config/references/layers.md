# Layer Model

The AIO sandbox organizes every environment piece into one of five profile
layers. The layer is set by the `category` field in a scenario's
`scenario.toml`, and it controls two things: how the TUI groups the row, and
the order `aio-config gen` assembles fragments into `Dockerfile.base`.

## The five layers

| Layer | `category` | What lives here | always_on? | Example |
|---|---|---|---|---|
| L1 OS / 基础环境 | `os` | foundational infra + versioned runtimes that everything depends on | node, python are | apt, ca-certs, build-essential, user `gem` (in head); Node, CPython (scenarios) |
| L2 Shell 便利 | `shell` | CLI convenience tools, pure binaries, no aliases | no | fzf, ripgrep, bat, fd |
| L3 语言开发链路 | `lang` | language toolchains + language version/package managers | no | rust, go, python-dev, nvm, uv |
| L4 应用 / AI agent | `app` | CLI applications and AI agents run from the terminal | no | opencode |
| L5 外部服务 | `service` | containerized web services with their own port + pane | reserved/future | code-server, vnc (wired today as compose profiles, not scenarios) |

The canonical order is `["os", "shell", "lang", "app", "service"]` (see
`category_rank` in `config/src/scenario.rs`). `gen` sorts enabled fragments by
`(category_rank, id)` so the assembled Dockerfile.base always reads head -> L1
-> L2 -> L3 -> L4 -> tail, regardless of the order you ticked them in. Unknown
categories sort last by `category` then `id`, so adding a new layer later
won't reorder the known ones.

> Layer order is **for readability only**. Each scenario fragment is an
> independent `RUN` layer with no build-time dependency on the others. There is
> no dependency graph between fragments - don't invent one. If tool X truly
> needs tool Y at build time (e.g. a Python wheel that needs a compiler), that
> dependency lives inside X's own fragment, not across scenarios.

## L1 in detail - the split that confuses everyone

L1 is special: it has two parts, handled differently.

**Non-versioned infrastructure** lives hardcoded in `Dockerfile.base.head` and
NEVER appears as a scenario:

- `FROM debian:bookworm-slim`
- HTTPS apt source rewrite (network policy blocks plain HTTP)
- ca-certs chicken-and-egg bootstrap (`Acquire::https::Verify-Peer=false` once,
  then `update-ca-certificates`)
- apt install of `curl git gnupg2 xz-utils build-essential pkg-config libssl-dev
  locales tzdata sudo`
- locale-gen en_US.UTF-8
- workspace user `gem` (uid 1000)

If a request is "add an apt package that's pure system infrastructure"
(e.g. `htop`, `less`, `vim`), the answer is usually **edit `Dockerfile.base.head`'s
apt list**, not a new scenario. A scenario is overkill for a one-line apt add
that has no version selection and no narrative. Use a scenario only when it's a
meaningful, nameable environment piece (a toolchain, a version manager, a CLI app)
that benefits from being toggleable.

**Versioned runtimes** Node and CPython ARE scenarios with `always_on = true`:

- `scenarios/node/` - nodejs.org tarball to `/usr/local`, version-selectable
- `scenarios/python/` - python-build-standalone tarball to `/usr/local`, version+tag

`always_on = true` means `gen` bakes them unconditionally regardless of the
selection manifest. The TUI shows them as locked `[*]` rows with a version
`[label]` you cycle with Left/Right - you pick a **version**, not whether to
install. They are always_on because `app` (web-builder stage + runtime pty) and
`code-server` depend on node; removing node breaks those builds.

## Why node/python are always_on (don't undo this)

- `app/Dockerfile` has a `web-builder` stage `FROM sandbox-base` that runs
  `npm ci && npm run build` - needs node at build time.
- `code-server/Dockerfile` is `FROM sandbox-base` - inherits node.
- The app runtime stage is `FROM sandbox-base` and spawns pty shells (terminal,
  opencode) - inherits node+python.

So node must be in base. If someone asks "can we make node optional to slim the
image?" - no, not without breaking app/code-server. (A future "headless base
without web-builder" is out of scope for this skill.)

## Where each layer installs tools

The install location follows the same rule for L1-L4 (see
scenario-authoring.md §"The two rules"):

| Layer | Typical install path | Why |
|---|---|---|
| L1 node/python | `/usr/local` | system path, on PATH, survives volume |
| L2 shell-utils | `/usr/bin` (apt) or `/usr/local/bin` (symlinks) | apt + Debian-rename symlinks |
| L3 rust/go | `/opt/rust`, `/usr/local/go` + symlinks to `/usr/local/bin` | custom ENV PATH + symlink for login shells |
| L3 nvm | `/opt/nvm` (baked) + `~/.nvm` (runtime, on volume) | nvm.sh baked system-side; versions on volume to survive recreate |
| L3 uv | `/usr/local/bin` (baked) + `~/.local/share/uv` (runtime, on volume) | binary system-side; managed pythons on volume |
| L4 opencode | `/usr/local/bin` | single static binary |

L5 services run in their own container; their install path is inside that
container's Dockerfile, not a scenario.

## Picking the layer for a new request

- "add a language + compiler" -> L3 `lang` (rust, go). If it ships a runtime the
  app depends on, consider L1 `always_on` (like node) - but that's rare.
- "add a version manager for an existing runtime" -> L3 `lang` (nvm, uv). It
  complements the L1 default; it doesn't replace it.
- "add a CLI tool I type in the terminal" -> L2 `shell` if it's a pure utility
  (fzf, rg), L4 `app` if it's a named application/agent (opencode).
- "add something with its own web UI on a port" -> L5 `service` (compose +
  caddy + services.toml), NOT a scenario. See compose-registry.md.
- "add a system apt package" -> edit `Dockerfile.base.head`'s apt list, no scenario.

When unsure between L2 and L4: if it's a small, generic, composition-style tool
(grep-like, ls-like, a pager), L2. If it's a distinct application you'd `--version`
and has its own identity/release cycle (an AI agent, a CLI dashboard), L4.
