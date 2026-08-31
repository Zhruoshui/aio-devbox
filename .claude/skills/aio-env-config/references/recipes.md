# Recipes

Worked, end-to-end examples. Model new work on the one that matches - then
verify with the build/verify commands at the end of each. All commands are for
the user to run (this skill does not run builds).

## Recipe 1: add a new L3 language scenario (e.g. Bun)

The 90% case. A language/runtime as a toggleable scenario.

**1. Create the scenario files.**

`scenarios/bun/scenario.toml`:
```toml
id = "bun"
name = "Bun"
description = "Bun 运行时(GitHub release 单二进制,装 /usr/local/bin)"
category = "lang"
```

`scenarios/bun/fragment.Dockerfile`:
```dockerfile
# >>> scenario: bun >>>
# L3:Bun 运行时。GitHub release 单二进制,装 /usr/local/bin(系统路径,
# 不被共享卷遮盖;在 PATH 上,login/非 login shell 都可见)。
ARG BUN_VERSION=1.1.42
RUN curl -fsSL "https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-linux-x64.zip" -o /tmp/bun.zip \
 && apt-get update && apt-get install -y --no-install-recommends unzip && rm -rf /var/lib/apt/lists/* \
 && unzip /tmp/bun.zip -d /tmp/bun \
 && install -m 0755 /tmp/bun/bun-linux-x64/bun /usr/local/bin/bun \
 && rm -rf /tmp/bun /tmp/bun.zip \
 && bun --version
# <<< scenario: bun <<<
```

Why each thing: `id == "bun"` matches the dir. Single binary to `/usr/local/bin`
(rule 1: system path, not `/root`; rule 2: `/usr/local/bin` is on every
shell's PATH, no profile.d needed). `unzip` needs `apt-get update` first (rule 5).
HTTPS GitHub URL (rule 3). `ARG` pins the version (rule 4).

**2. Enable + build.**
```bash
make config                 # tick "Bun" under L3, press s
make build-base             # gen assembles Dockerfile.base with the bun fragment
make up                     # rebuilds app (FROM sandbox-base) + starts the stack
```
Note: `make up` only rebuilds/starts the always-on services (gateway + app).
If you run code-server/vnc, pass their profiles: `make up PROFILES=code-server`
(and see §"Rebuild gotchas" if a container is already running — the built image
doesn't auto-swap a running container).

**3. Verify.**
```bash
docker exec aio-app-1 bash -lc 'bun --version && which bun'
# expect: 1.1.42  /usr/local/bin/bun   (which must succeed in a LOGIN shell)
```

## Recipe 2: pin a different version of node (or python)

No new files - edit the existing versioned scenario.

**1. Edit `scenarios/node/scenario.toml`**, add a `[[versions]]` entry:
```toml
[[versions]]
label = "23.3.0"
version = "23.3.0"
```

**2. Pick it.** Either via the TUI (`make config`, cycle to 23.3.0 on the node
row, `s`) or hand-edit `.aio/enabled.toml`:
```toml
[[versions]]
id = "node"
label = "23.3.0"
```

**3. Build + verify.**
```bash
make build-base
make up
docker exec aio-app-1 bash -lc 'node --version'   # expect v23.3.0
```

For **python**, the version is coupled to a release `tag` (python-build-standalone):
```toml
[[versions]]
label = "3.13.1"
version = "3.13.1"
tag = "20241201"      # MUST come from the same astral-sh/python-build-standalone release
```
A mismatched tag -> the `curl` 404s at build time. Get version+tag together
from the upstream releases page.

## Recipe 3: add an L2 apt utility (e.g. htop)

Could be a scenario or a head edit. A scenario is right if it's a named,
toggleable tool; for pure infra apt, edit `Dockerfile.base.head` instead. Here's
the scenario version:

`scenarios/shell-extras/scenario.toml`:
```toml
id = "shell-extras"
name = "Shell 额外工具"
description = "htop / jq / tree 等便利工具(apt 装系统路径)"
category = "shell"
```

`scenarios/shell-extras/fragment.Dockerfile`:
```dockerfile
# >>> scenario: shell-extras >>>
RUN apt-get update \
 && apt-get install -y --no-install-recommends htop jq tree \
 && rm -rf /var/lib/apt/lists/* \
 && htop --version && jq --version && tree --version
# <<< scenario: shell-extras <<<
```

`apt-get update` first (head cleared the lists - rule 5). These land in
`/usr/bin`, already on PATH. Build + verify as in Recipe 1.

## Recipe 4: add an L4 AI agent with a WebUI button (e.g. aichat)

A CLI agent you want both installed AND surfaced as a sidebar pane.

**1. Scenario** (`scenarios/aichat/scenario.toml` + `fragment.Dockerfile`) -
single binary to `/usr/local/bin`, like Recipe 1 but `category = "app"`.

**2. Register the button** in `app/services.toml`:
```toml
[[service]]
id = "aichat"
type = "agent"
cmd = "aichat"        # button shows only if `aichat` is on PATH
label = "aichat"
```

**3. Build + verify.** `services.toml` is compiled into the app binary
(`include_str!`), so the **app image must rebuild** (not just restart):
```bash
make config            # tick aichat
make build-base        # bake the aichat scenario into sandbox-base
make up                # rebuilds app (picks up services.toml) + starts stack
docker exec aio-app-1 bash -lc 'aichat --version'
```
The button auto-shows in the WebUI because `command_exists(aichat)` is true.
Don't bake the scenario and the button hides (no dead pane).

## Recipe 5: add a containerized web service pane (e.g. Jupyter)

The L5 path - three files change together. See `references/compose-registry.md`
for the full reasoning. Sketch:

**docker-compose.yml** - new service under a profile, join app's netns
(`network_mode: "service:app"`; no networks/expose - they're mutually exclusive;
pick a port clear of the reserved 8088/8200/6080/5900):
```yaml
jupyter:
  build: { context: ., dockerfile: jupyter/Dockerfile }
  image: sandbox-jupyter
  restart: unless-stopped
  profiles: [jupyter]
  network_mode: "service:app"
  volumes: [workspace:/root]
```

**gateway/Caddyfile** - a `handle_path` BEFORE the catch-all:
```caddyfile
handle_path /jupyter/* {
    reverse_proxy app:8888
}
```

**app/services.toml** - a `type=web` entry:
```toml
[[service]]
id = "jupyter"
type = "web"
target = "app:8888"
url = "/jupyter/"
label = "Jupyter"
```

**Run + verify.**
```bash
make up PROFILES=jupyter
docker exec aio-app-1 sh -c 'curl -sS -o /dev/null -w "%{http_code}\n" http://localhost:8888/'
# button shows (manifest enabled=true via TCP probe); iframe opens at /jupyter/
```
Rebuild the app image so `services.toml` is picked up. If the app's assets are
absolute (not relative), a subpath won't work - check the target app's base-path
support before committing to the `handle_path` strip pattern.

## Recipe 6: the comprehensive multi-layer plan

When the user wants several things at once ("node 22 + python 3.13 + rust + go
+ a web preview + an agent"), don't do them one at a time - produce a plan
table first, then execute. This is the headline use case for the skill.

Produce a table like:

| Piece | Layer | New? | Files to change | Version |
|---|---|---|---|---|
| Node 22 | L1 `os` always_on | existing | `scenarios/node/scenario.toml` + `.aio/enabled.toml` | 22.11.0 |
| Python 3.13 | L1 `os` always_on | existing | `scenarios/python/scenario.toml` + `.aio/enabled.toml` | 3.13.0 / tag 20241016 |
| Rust | L3 `lang` | existing | `.aio/enabled.toml` (tick) | stable |
| Go | L3 `lang` | existing | `.aio/enabled.toml` (tick) | 1.23.4 |
| Jupyter pane | L5 `service` | new | `docker-compose.yml` + `Caddyfile` + `app/services.toml` + `jupyter/Dockerfile` | - |
| aichat | L4 `app` + button | new | `scenarios/aichat/*` + `app/services.toml` | v1.18.7 |

Then execute in this order. The new pieces (aichat scenario, jupyter service)
must be **authored on disk before** `make config`, because the TUI only lists
scenario.toml files that already exist under `scenarios/`:
```bash
# 0. author the new files first (aichat: scenarios/aichat/* + services.toml entry;
#    jupyter: docker-compose.yml service + Caddyfile block + services.toml entry + jupyter/Dockerfile)
# 1. scenario selection + versions (TUI now sees aichat too)
make config                # tick rust, go, aichat; set node=22.11.0, python=3.13.0
# 2. assemble + build the base image with all baked scenarios
make build-base
# 3. rebuild derived images (app picks up services.toml changes too) + start
make up PROFILES=jupyter
# 4. verify each
docker exec aio-app-1 bash -lc 'node --version; python3 --version; rustc --version; go version; aichat --version'
docker exec aio-app-1 sh -c 'curl -sS -o /dev/null -w "jupyter:%{http_code}\n" http://jupyter:8888/'
```

Call out the gotchas that apply: python tag coupling, login-shell `which` for
each tool, `app/services.toml` needing an app rebuild, the jupyter profile.

## Rebuild gotchas

These are the measured behaviors of this stack. When in doubt, `make down &&
make up` (with the same `PROFILES=`) is the reliable restart.

- **Building the base image does not swap a *running* container to it.** After
  `make build-base`, derived services (app, code-server) DO rebuild their image
  (BuildKit tracks the `FROM sandbox-base` digest), but a container that is
  already Up is not automatically recreated onto the new image. Force it:
  `docker compose up -d --force-recreate <service>` (or `make down && make up`
  with the same `PROFILES=`).
- **`compose up --build` may skip a profile service that is already running.**
  The bake plan for an actively-Running service can silently miss its rebuild -
  observed with vnc (changed vnc/Dockerfile -> `make up PROFILES=...` rebuilt
  only app; vnc stayed on the old image). If a profile service's own Dockerfile
  changed, do it explicitly:
  `docker compose -f ... build <svc> && docker compose up -d --force-recreate <svc>`.
- **`make up` without `PROFILES=` drops optional services.** If you had
  code-server/vnc up, recreate with the same `PROFILES=code-server,vnc` or they
  won't start. `make down` takes the same flags so the teardown matches.
- **`NOBUILD=1` skips the base build.** Don't use it after changing a scenario -
  the change only lands via `make build-base`.
- **services.toml/Caddyfile changes need an app/gateway recreate**, not just a
  restart. `make up` rebuilds app; for caddy, `docker compose up -d --force-recreate gateway`.
- **`make config` writes `.aio/enabled.toml` host-owned** (the Makefile runs
  aio-config as the host uid). `make gen` / `make build-base` regenerate
  `Dockerfile.base` host-owned too. Don't run them as root or the files become
  root-owned and awkward to edit later.
- **vnc is decoupled from sandbox-base.** Rebuilding base for a new scenario
  does NOT rebuild vnc (good - it doesn't need the tool). Don't expect a new
  dev tool to appear in the vnc/Chromium container; it won't, and shouldn't.
  Conversely, vnc's own changes never ride on `make build-base` - see the second
  bullet.

## Verify checklist (run these, don't guess)

After building, verify in a container that replicates the real terminal shell
(`bash -lc` so profile.d and PATH load as the pane sees them):

```bash
# tool exists AND a login shell finds it
docker exec aio-app-1 bash -lc 'command -v <tool> && <tool> --version'
# L1 runtimes
docker exec aio-app-1 bash -lc 'node --version; python3 --version; npm --version'
# an L4 agent button will show because command_exists passes:
docker exec aio-app-1 bash -lc 'command -v opencode'
# a web service sidecar is reachable on app's shared netns:
docker exec aio-app-1 sh -c 'curl -sS -o /dev/null -w "%{http_code}\n" http://<svc>:<port>/'
# the generated Dockerfile.base has no leftover {{ placeholders:
grep -n '{{' Dockerfile.base   # should print nothing
# the manifest reflects services.toml (after app rebuild):
curl -sS -u admin:admin http://localhost:8080/api/manifest | head
```
