# Layer Model

The AIO sandbox organizes every environment piece into one of five profile
layers. The layer is set by the `category` field in a scenario's
`scenario.toml`, and it controls two things: how the TUI groups the row, and
the order `aio-config gen` assembles fragments into `Dockerfile.base`.

## The five layers

| Layer | `category` | What lives here | always_on? | Members (2026-09-22) |
|---|---|---|---|---|
| L1 OS / 基础环境 | `os` | foundational infra + the toolchain manager + runtimes everything depends on | node, python, **mise** are | apt/ca-certs (in head); node, python, **mise engine**, fonts |
| L2 Shell 便利 | `shell` | CLI convenience tools, pure binaries, no aliases | no | fzf, ripgrep, bat, fd, eza, zoxide, delta, starship, jq, yq |
| L3 语言开发链路 | `lang` | language toolchains, in **two schools** | no | **mise-managed**: rust, go, uv, ruff — **system-managed**: c23 |
| L4 应用 / AI agent | `app` | CLI applications and AI agents run from the terminal | no | opencode, claude-code, codex, pi, pi-web |
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
>
> **The one soft exception (2026-09-22):** every `mise`-managed tool fragment
> assumes the L1 `mise` engine fragment already ran (it needs `/opt/mise` and
> the four `MISE_*` env vars). That is guaranteed by layer ordering (`os`
> sorts before `shell`/`lang`/`app`), not by a declared dependency. If the
> ordering ever breaks, the tool fragment fails fast with
> `mise: command not found`. See scenario-authoring.md §"mise-managed tools".

## L1 in detail - the split that confuses everyone

L1 is special: it has three parts, handled differently.

**Non-versioned infrastructure** lives hardcoded in `Dockerfile.base.head` and
NEVER appears as a scenario:

- `FROM debian:bookworm-slim`
- HTTPS apt source rewrite (network policy blocks plain HTTP)
- ca-certs chicken-and-egg bootstrap (`Acquire::https::Verify-Peer=false` once,
  then `update-ca-certificates`)
- apt install of `curl git gnupg2 xz-utils build-essential pkg-config libssl-dev
  locales tzdata sudo`
- locale-gen en_US.UTF-8
- default user root (uid 0); home /root IS the workspace volume

If a request is "add an apt package that's pure system infrastructure"
(e.g. `htop`, `less`, `vim`), the answer is usually **edit `Dockerfile.base.head`'s
apt list**, not a new scenario. A scenario is overkill for a one-line apt add
that has no version selection and no narrative. Use a scenario only when it's a
meaningful, nameable environment piece (a toolchain, a version manager, a CLI app)
that benefits from being toggleable.

**The toolchain manager engine** `scenarios/mise/` is `always_on = true` with
`category = "os"`. It installs ONLY the mise binary + shims + the four
redirect env vars + `/etc/profile.d/mise.sh` — **no tools**. It exists so every
L2/L3/L4 mise-managed tool scenario has a manager to call. Cost: ~30MB.

**Versioned runtimes** Node and CPython ARE scenarios with `always_on = true`:

- `scenarios/node/` - nodejs.org tarball to `/usr/local`, version-selectable
- `scenarios/python/` - python-build-standalone tarball to `/usr/local`, version+tag

`always_on = true` means `gen` bakes them unconditionally regardless of the
selection manifest. The TUI shows them as locked `[*]` rows with a version
`[label]` you cycle with Left/Right - you pick a **version**, not whether to
install. They are always_on because `app` (web-builder stage + runtime pty) and
`code-server` depend on node; removing node breaks those builds.

## Why node/python/mise-engine are always_on (don't undo this)

- `app/Dockerfile` has a `web-builder` stage `FROM sandbox-base` that runs
  `npm ci && npm run build` - needs node at build time.
- `code-server/Dockerfile` is `FROM sandbox-base` - inherits node.
- The app runtime stage is `FROM sandbox-base` and spawns pty shells (terminal,
  opencode) - inherits node+python.
- **mise engine** is always_on so that every mise-managed tool scenario
  (L2/L3/L4) has `/opt/mise` + `MISE_*` available when its fragment runs. It
  is intentionally tool-free, so always_on costs only ~30MB — the tools
  themselves stay optional.

So node must be in base. If someone asks "can we make node optional to slim the
image?" - no, not without breaking app/code-server. (A future "headless base
without web-builder" is out of scope for this skill.)

## pi / pi-web are NOT always_on (changed 2026-09-10, S1)

Historical note — earlier revisions of this file said pi/pi-web were
`always_on`. That was reverted in the S1 task (09-10-sandbox-mgr-unified D1):

- Both are now ordinary **optional** scenarios (`always_on = false`).
- The mgr create-wizard's "services" switches own them; `routes.rs`
  `normalize_services` folds the switch state into `manifest.scenarios`, so the
  service area and the scenario list stay in sync.
- `scenarios/pi-web/scenario.toml` and `scenarios/pi/scenario.toml` carry the
  full rationale in their comments.

**UI落位（2026-09-22）**: even though all five L4 members are scenarios, only
**opencode / claude-code / codex** appear in the L4 scenario area of the
create wizard. **pi / pi-web are surfaced in the services area instead** —
`EnvPicker.tsx`'s `SERVICE_SCENARIOS = ["pi","pi-web"]` excludes them from the
layer list because pi-web has a cascade dependency on pi (enabling pi-web
auto-enables pi; disabling pi cascades pi-web off). One switch, one place.

## Where each layer installs tools

The install location follows the same rule for L1-L4 (see
scenario-authoring.md §"The two rules"): **system path, so it survives the
shared workspace volume mounting over `/root`.**

| Layer | Typical install path | Why |
|---|---|---|
| L1 node/python | `/usr/local` | system path, on PATH, survives volume |
| L1 mise engine | binary `/usr/local/bin/mise`; data `/opt/mise` | system path; data redirected away from `/root` |
| L2 shell tools | `/opt/mise/installs/<tool>` (mise-managed), shim on `/opt/mise/shims` | same system-path rule; shims are on PATH for all shells |
| L3 mise school | `/opt/mise/installs/<tool>` | ditto (rust also uses `/opt/mise/{rustup,cargo}`) |
| L3 system school (c23) | `/usr/bin` + `/usr/local/bin` symlinks | apt + version-suffix symlinks |
| L4 agents | `/opt/mise/installs/<tool>` | mise-managed, uniform with L2/L3 |

L5 services run in their own container; their install path is inside that
container's Dockerfile, not a scenario.

**Both L2 and L3/L4 mise tools land in the same tree** (`/opt/mise/installs`).
That is deliberate: mise owns them all, and a single shim directory serves
them. It also means `mise ls` is the one place to see every managed tool.

## Picking the layer for a new request

- "add a language toolchain" -> **L3 `lang`**, and prefer the **mise school**:
  a new directory `scenarios/<tool>/` whose fragment calls
  `mise use -g "<tool>@${VERSION}"`. Only fall back to the system school
  (apt, like c23) when the tool is not in the mise registry or must be
  ABI-matched to the distro's libc.
- "add a version manager for an existing runtime" -> that's the L1 `mise`
  engine. It complements the L1 runtimes; it doesn't replace them.
- "add a CLI tool I type in the terminal" -> L2 `shell` if it's a pure utility
  (fzf, rg), L4 `app` if it's a named application/agent with its own release
  cycle (opencode, claude-code, codex).
- "add an AI agent" -> **L4 `app`**, mise school:
  `scenarios/<agent>/` + `mise use -g "<agent>@${VERSION}"`. Note the
  binary name may differ from the scenario id (e.g. scenario `claude-code`
  installs the `claude` binary) — always check, and verify the real name with
  `mise ls-remote` / an actual install before writing the fragment.
- "add something with its own web UI on a port" -> L5 `service` (compose +
  caddy + services.toml), NOT a scenario. See compose-registry.md.
- "add a system apt package" -> edit `Dockerfile.base.head`'s apt list, no scenario.

When unsure between L2 and L4: if it's a small, generic, composition-style tool
(grep-like, ls-like, a pager), L2. If it's a distinct application you'd `--version`
and has its own identity/release cycle (an AI agent, a CLI dashboard), L4.
