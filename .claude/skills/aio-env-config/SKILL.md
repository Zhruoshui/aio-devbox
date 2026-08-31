---
name: aio-env-config
description: Configure the AIO dev-sandbox environment (this repo) — authoring scenario files, enabling profiles, registering UI buttons, and planning a layered config across the build-time Dockerfile.base scenario system. Use whenever the user wants to add/change/remove installed dev tools, language runtimes, version managers, shell utilities, AI agents, or containerized web services in this sandbox. Trigger phrases: "add a scenario", "install X in the sandbox", "add a new language/runtime", "pin a version of node/python/go", "make X available in code-server", "add a web service / a button / a pane", "enable vnc/code-server", "which layer does X go in", or any question about .aio/enabled.toml / make config / make build-base / scenarios/ / Dockerfile.base. Also use it for a "comprehensive environment config plan" that touches multiple layers at once — that is exactly the whole-system planning this skill exists for.
---

# AIO 环境配置

> This skill configures the **build-time environment** of the AIO dev sandbox
> (this repo, `sandboxes/aio`). It is about *what tools/runtimes/services get
> baked into the `sandbox-base` image and surfaced in the WebUI*, not about
> running the app or writing app features.
>
> The repo has a layered "scenario preset" system: `scenarios/<id>/` files are
> assembled by a Rust TUI/configurator (`aio-config`) into `Dockerfile.base`,
> which builds the `sandbox-base` image every derived container inherits.
> Adding an environment piece = mostly adding a scenario; sometimes also a
> compose service + gateway route + services.toml entry. This skill knows the
> whole map.

## When to use this skill

Use it when the user wants to **change what's in the sandbox environment**:

- "加个 Deno / Bun / Java / .NET 场景" → new language/runtime
- "把 Go 升到 1.24" / "Node 换成 22" → pin a version
- "装 ripgrep / jq / htop" → shell utility
- "加个新的 AI agent CLI" → L4 app
- "加个 Jupyter / code-server 一样的 web 服务面板" → compose profile + service + caddy
- "给侧边栏加个按钮" → runtime button (buttons.toml) or built-in (services.toml)
- "我想要 node + python + rust + go + 一套 web 预览,整体给我规划一下" → **comprehensive multi-layer plan** (the headline use case)

Do **not** use it for: writing the React frontend, the axum app routes, the
gateway auth, or the TUI ratatui code — those are app/feature work, not
environment configuration. (If a request is both, the env-config parts belong
here; the app-feature parts do not.)

## The whole system in one screen

```
                    make config  (TUI writes selection)
                          │
                          ▼
                 .aio/enabled.toml   ← scenarios[] + [[versions]]
                          │
            make gen  (aio-config gen, Rust)
                          │  assemble:
                          ▼
   Dockerfile.base.head  +  Σ enabled scenario fragments  +  Dockerfile.base.tail
                                   (sorted L1→L2→L3→L4, always_on forced in)
                          │
            make build-base  (docker build -t sandbox-base)
                          │
                          ▼
   sandbox-base  ──FROM──►  app (runtime) , code-server   [vnc is NOT from base]
```

Five things you can add, by increasing scope:

1. **A scenario fragment** (`scenarios/<id>/`) — a root-context Dockerfile snippet
   baked into `sandbox-base`. Covers L1–L4. The 90% case.
2. **A version pin** on an existing versioned scenario (node/python) — edit
   `scenario.toml` `[[versions]]`, no new files.
3. **A built-in WebUI button** (`app/services.toml`) — needs an app rebuild.
4. **A runtime-only button** (`/root/.aio/buttons.toml`, via the UI/API) —
   no image rebuild, but only `type=agent` in the MVP.
5. **A containerized web service** (`docker-compose.yml` profile + `Caddyfile`
   `handle_path` + `services.toml` `type=web`) — the heaviest; a whole new pane.

Read `references/layers.md` for the layer model; `references/scenario-authoring.md`
for the scenario file contract + the two path/PATH rules that bite everyone;
`references/compose-registry.md` for services/buttons/caddy; and
`references/recipes.md` for worked end-to-end examples (a new language, a
version pin, a web service pane).

## How to decide what a request needs (decision tree)

Walk this top-down. The first match wins. "X" = the thing the user wants added/changed.

```
Is X a whole new containerized process with its own port + a WebUI pane?
├─ YES → it's a web SERVICE (L5). Read references/compose-registry.md
│         §"Adding a containerized web service (L5)". You need ALL of:
│           • docker-compose.yml: a new service under a `profiles: [<name>]`,
│             expose its port, join sandbox-net (mount the workspace volume only
│             if it actually needs /root).
│           • gateway/Caddyfile: a `handle_path /<prefix>/*` block BEFORE the
│             catch-all `handle`, reverse_proxy <service>:<port>.
│           • app/services.toml: a [[service]] with type=web, target=<svc>:<port>,
│             url=/<prefix>/... ; then REBUILD the app image.
│         (Existing examples: code-server on 8200, vnc on 6080.)
└─ NO  → X is baked into sandbox-base. Continue.

Is X a runtime/tool that needs to exist at build time and survive container recreate?
├─ YES → it's a SCENARIO fragment under scenarios/<id>/. Continue.
└─ NO  → it's a RUNTIME-ONLY install into the shared volume (~/.local/bin),
         no image rebuild. See references/paths-and-offline.md §"Baked vs runtime
         - which to choose". (Also the right answer for "just let me try X once
         without rebuilding".)

What layer does the scenario belong to? (category in scenario.toml; sets TUI group + sort)
├─ "os"     (L1) foundational runtime, always present. Use `always_on = true` if it
│            MUST be in every image (node is, because app/code-server build on it;
│            python ships alongside as the default runtime). Otherwise L1 infra stays
│            in Dockerfile.base.head — do NOT add a scenario for plain apt packages; edit head.
├─ "shell"  (L2) CLI conveniences (fzf/rg/bat/fd). apt or static binaries to system path.
├─ "lang"   (L3) language toolchains (rust/go/python-dev) and version managers (nvm/uv).
├─ "app"    (L4) CLI apps / AI agents (opencode). Single binary to /usr/local/bin.
└─ "service"(L5) reserved/future for external services. Not wired yet.

Does it need version selection in the TUI?
├─ YES → add `[[versions]]` (label + template vars) + `default_version`; use
│        `{{version}}`/`{{tag}}` placeholders in fragment.Dockerfile. gen substitutes them.
└─ NO  → plain fragment; version pinned by an ARG inside the fragment (e.g. ARG X_VERSION=…).

Does it also need a WebUI button or a compose service?
├─ CLI tool the user runs in the terminal pane → NO button needed (terminal is always there).
│  If you want a dedicated sidebar button for it → references/compose-registry.md
│  §"WebUI button / pane (app/services.toml)".
├─ AI agent CLI you want as a pane → app/services.toml type=agent, cmd=<binary>;
│  the button auto-shows only when the binary is on PATH (command_exists probe),
│  so it stays in sync with whether the scenario is baked. REBUILD app image.
└─ Web app with its own UI in an iframe → it's a web SERVICE (top of this tree).
```

## The 5-step flow (run this for every config change)

Follow these in order. The references hold the detail; this is the spine.

### 1. Classify the request
Run the decision tree above. State the conclusion out loud: layer, whether it's
a new scenario vs a version edit vs a service vs a button, and which files will
change. If the request spans multiple pieces (the "comprehensive plan" case),
list each piece and its layer — that list IS the plan.

### 2. Author the changes
- **New scenario**: create `scenarios/<id>/scenario.toml` + `scenarios/<id>/fragment.Dockerfile`.
  The `id` MUST equal the directory name or `gen` rejects it. Follow
  `references/scenario-authoring.md` — especially the two rules that cause most
  bugs: **install to a system path, not `/root`** (the workspace volume masks
  it), and **make the tool findable in a login shell** (symlink into `/usr/local/bin`
  or write a `/etc/profile.d/*.sh`, because `bash -l` resets PATH and drops custom
  ENV PATH). Use HTTPS URLs (the network policy blocks plain HTTP and NodeSource).
  Pin versions with an `ARG` so builds are reproducible.
- **Version pin on node/python**: edit the existing `scenarios/node/scenario.toml`
  or `scenarios/python/scenario.toml` `[[versions]]`. For python, the release
  `tag` is coupled to the version (python-build-standalone) — get both from the
  same upstream release, or the curl 404s.
- **New web service / button**: see `references/compose-registry.md`.

### 3. Enable it
- Scenarios: run `make config` (interactive TUI) and tick it, **or** hand-edit
  `.aio/enabled.toml` (add the id to `scenarios = [...]`; for a versioned
  scenario add a `[[versions]]` block with id+label). always_on scenarios are
  never listed in `scenarios` — only their version selection lives in `[[versions]]`.
- Web services: add the `--profile <name>` to your `make up` invocations (or
  set `PROFILES=` in the Makefile call). The profile gates the *container*;
  `services.toml` + caddy gate the *button/pane*.

### 4. Generate the exact build/verify command list
Produce a copy-pasteable command list for the user to run (the user chose to run
builds themselves — do NOT run `make build` / `docker` from this skill). Tailor
to what changed:

- Scenario changed →
  `make config` (if not already), then `make build-base` (runs `gen` + `docker
  build -t sandbox-base`). Then `make up` (rebuilds the base-derived services in
  the current run and starts the stack). Note: `make up` only carries the
  services whose profiles you pass — `make up PROFILES=code-server,vnc` for
  those, plain `make up` for app/gateway only.
- **Rebuild gotcha (measured, not theoretical):** building the base image does
  NOT by itself swap a *running* container to the new image. Base-derived
  services (app, code-server) rebuild their image via BuildKit's FROM-digest
  detection, but a container that is already Up is not automatically recreated.
  If a service must pick up the change: `docker compose up -d --force-recreate
  <service>` (or `make down && make up` with the same `PROFILES=`). vnc is worse:
  it is NOT derived from sandbox-base, so a base change never rebuilds vnc — its
  own Dockerfile change needs an explicit `docker compose build vnc && docker
  compose up -d --force-recreate vnc`. See references/recipes.md §"Rebuild gotchas".
- Version pin only → same as above (`make build-base` regenerates Dockerfile.base
  with the new `{{version}}` substituted).
- services.toml changed → the app image must be rebuilt (`make up` rebuilds it;
  it's `include_str!` into the binary, so a rebuild is required, not just a restart).
- Caddyfile / compose changed → `make up` (or `docker compose up -d` for the
  gateway profile). Caddyfile changes need the gateway container recreated.
- Verify inside the container: `docker exec aio-app-1 bash -lc '<tool> --version'`
  (note `bash -lc` so profile.d / PATH is loaded the way the real terminal pane
  does). Check both the tool exists AND a login shell finds it.

### 5. Document & hand off
Summarize: what layer each piece went into, which files changed, the exact
commands the user should run, and how to verify. For multi-piece plans, a short
table (piece | layer | files | commands) reads best. If something was a known
gotcha (python-build-standalone tag coupling, NodeSource block, login-shell PATH
reset, volume masking /root), call it out — the user will hit it otherwise.

## Critical rules (these bite — read scenario-authoring.md for the why)

1. **System path, not /root.** Anything baked into the image must land in
   `/usr/local`, `/opt`, `/usr/local/bin` — NOT under `/root`. The shared
   workspace volume (`aio_workspace`) is mounted over `/root` at runtime,
   so anything baked there is masked by the volume (the container sees the
   volume's contents, not your install). This is the #1 scenario bug.

2. **Login shell resets PATH.** The terminal pane runs `bash -l`, which sources
   `/etc/profile` and resets PATH, dropping any `ENV PATH=` you set in a
   Dockerfile. Either symlink the binary into `/usr/local/bin` (on every shell's
   PATH) or add a `/etc/profile.d/<x>.sh` that exports PATH. See rust/go scenarios.
3. **always_on scenarios never appear in `enabled.toml` `scenarios = [...]`.**
   They're baked unconditionally; only their version selection goes in
   `[[versions]]`. The TUI renders them as locked `[*]` rows.
4. **id == directory name.** `scenario.toml`'s `id` field must equal the
   `scenarios/<id>/` directory name, or `aio-config gen` bails.
5. **Versioned fragments need `[[versions]]` with all placeholder vars.** A
   fragment with `{{version}}` but no versions list is an error; a version
   whose vars don't cover every `{{key}}` leaves `{{…}}` in the output and bails.
6. **Network policy blocks plain HTTP (port 80) and NodeSource.** Use HTTPS
   URLs. apt sources in head are already forced to HTTPS with a ca-certs
   chicken-and-egg bootstrap; don't revert that.
7. **node is `always_on` because the build pipeline needs it.** The app's
   `web-builder` stage (`FROM sandbox-base`) runs `npm ci && vite build`, and
   code-server (`FROM sandbox-base`) is a node app — both break if node is
   absent. python is `always_on` alongside it as the default dev runtime (the
   app runtime shell and the terminal panes inherit both from base). Don't
   "remove node/python to slim the image" without breaking app/code-server
   builds and the default dev environment.
8. **vnc is NOT derived from sandbox-base** (it's `FROM debian:bookworm-slim`).
   Adding a dev tool to a scenario does NOT put it in vnc — and that's intentional
   (vnc is a pure browser surface). Don't add dev tools expecting vnc to get them.

## References (read on demand)

- `references/layers.md` — the 5-layer model in full: what each layer is, what
  belongs there, what's hardcoded in head vs scenario, and the sort order.
- `references/scenario-authoring.md` — the scenario.toml + fragment.Dockerfile
  contract, the two PATH rules, the volume-masking rule, version template
  mechanics, and a checklist before you save a new scenario.
- `references/compose-registry.md` — compose profiles, the `app/services.toml`
  built-in button registry (type=web vs type=agent), the runtime
  `buttons.toml`, the gateway Caddyfile subpath pattern, and when to use which.
- `references/paths-and-offline.md` — where tools can live (system path vs
  shared volume vs container writable layer), the `~/.local/bin` auto-PATH trick,
  and how offline installs differ from baked-in scenarios.
- `references/recipes.md` — worked end-to-end examples: add a new L3 language
  scenario, pin a node/python version, add an L2 apt utility, add an L4 AI agent
  with a WebUI button, add a containerized web service pane, and the rebuild
  gotchas. Model new work on these.
