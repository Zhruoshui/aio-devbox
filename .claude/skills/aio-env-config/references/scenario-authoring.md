# Scenario Authoring

A **scenario** is a build-time Dockerfile fragment that `aio-config gen`
assembles into `Dockerfile.base`. Each scenario is a directory:

```
scenarios/<id>/
├── scenario.toml        # metadata: id, name, description, category, always_on?, versions?
└── fragment.Dockerfile  # the RUN steps, run as root, inserted between head and tail
```

The `id` in `scenario.toml` **must equal the directory name** or `gen` bails.
Fragments are assembled sorted by `(category_rank, id)`, so the order in the
built Dockerfile is deterministic regardless of tick order.

## scenario.toml contract

Minimal (non-versioned, selectable):

```toml
id = "rust"
name = "Rust 工具链"
description = "rustup stable + rustfmt + clippy + rust-analyzer,装到 /opt/rust"
category = "lang"
```

Versioned + always_on (like node/python):

```toml
id = "node"
name = "Node.js"
description = "Node 运行时(nodejs.org tarball /usr/local;app web-builder 依赖,必装)"
category = "os"
always_on = true
default_version = "20.18.0"

[[versions]]
label = "20.18.0"
version = "20.18.0"

[[versions]]
label = "22.11.0"
version = "22.11.0"
```

Fields:

- `id` (required): must match the directory name. `gen` resolves the fragment
  by id, and the TUI lists it by id.
- `name` (required): TUI display name.
- `description` (required): TUI one-line description, shown dimmed next to the
  name. Put the install path and any "depends on X" note here for the human.
- `category` (default `"lang"`): the layer. One of `os`/`shell`/`lang`/`app`/
  `service`. Unknown values sort last. Predating scenarios without `category`
  default to `lang` for back-compat - but always set it explicitly on new files.
- `always_on` (default false): if true, `gen` bakes it unconditionally; the TUI
  shows it as a locked `[*]` row (Space is a no-op). Only its version selection
  (if any) lives in `enabled.toml`, never the id itself.
- `versions` (default empty): a list of `[[versions]]` tables. Each has a
  `label` (the TUI dropdown display) plus any extra fields flattened into a
  `vars` map that `gen` substitutes into the fragment's `{{key}}` placeholders.
  Empty = not versioned; the fragment is assembled verbatim.
- `default_version` (default None): which version label `gen` picks if the
  manifest has no entry for this scenario. Falls back to `versions[0]`.

`gen` substitutes every `{{key}}` in the fragment using the selected version's
`vars`. If a fragment contains `{{` but the scenario has no versions, `gen`
bails. If after substitution any `{{` remains (a var you forgot to declare),
`gen` bails. So a versioned fragment must declare every `{{key}}` it uses in
every version entry (e.g. python uses `{{version}}` AND `{{tag}}`, both
present in each `[[versions]]`).

## enabled.toml selection contract (incl. the wildcard)

`scenarios` in `enabled.toml` is a list of scenario ids — or the single-element
wildcard `["*"]`, which expands at `gen`/TUI-load time to **every discovered
non-`always_on` scenario** (expansion lives in `config/src/manifest.rs::expand`,
the single owner of selection semantics):

```toml
scenarios = ["*"]   # full preset: all selectable scenarios, always_on excluded
```

- `"*"` must be the **only** element; `["*", "rust"]` makes `gen` bail
  (a clear contract beats lenient dedup).
- An explicit id list behaves exactly as before (byte-identical `gen` output).
- The wildcard is why a **new scenario directory is automatically picked up**
  by the CI full-variant pipeline (`.aio/presets/full.toml` uses `["*"]`) with
  zero config. CI never enumerates scenario ids itself — the always_on
  exclusion rule must stay inside aio-config, never leak into CI scripts.

## fragment.Dockerfile contract

The fragment is concatenated (with a blank-line separator) between
`Dockerfile.base.head` and `Dockerfile.base.tail`. Head runs as root and
installs infra; tail does `USER gem`. So the fragment runs **as root**, before
the user switch. Wrap it in the scenario banner for readability:

```dockerfile
# >>> scenario: <id> >>>
# one-line description of what this does and WHY it's installed where it is
ARG SOME_VERSION=1.2.3
RUN ... install steps ...
# <<< scenario: <id> <<<
```

Use the banner format the existing scenarios use (`# >>> scenario: id >>>` /
`# <<< scenario: id <<<`) - `gen` does not parse them, but they make the
generated `Dockerfile.base` readable and let you grep for which scenario a RUN
layer came from.

## The two rules that cause most scenario bugs

### Rule 1: install to a system path, never /home/gem

The shared workspace volume `aio_workspace` is mounted over `/home/gem` in the
app, code-server, and vnc containers. Anything baked into the image layer at
`/home/gem/...` is **masked** by the volume at runtime - the container sees the
volume's contents (possibly empty), not what you installed. This is the
single most common scenario bug.

Install baked tools to a system path the volume does not cover:

- `/usr/local/bin` - single binaries (uv, ruff, opencode, the rust/go proxies)
- `/usr/local` - tarball trees (node, python-build-standalone)
- `/opt/<tool>` - bigger toolchains (rust at `/opt/rust`, nvm.sh at `/opt/nvm`)
- `/etc/profile.d/<x>.sh` - login-shell PATH/profile hooks (nvm)

The only things that belong under `/home/gem` are **runtime** (not baked)
user data: `~/.nvm/versions` (nvm installs at runtime), `~/.local/share/uv`
(uv installs pythons at runtime), `~/.cargo` for a user's own `cargo install`.
Those go on the volume on purpose, to survive container recreate.

### Rule 2: make the tool findable in a login shell

The WebUI terminal pane runs `/bin/bash -l` (a login shell). `/etc/profile`
sources `/etc/profile.d/*.sh` and then **resets PATH** to a standard set,
dropping any `ENV PATH=...` you set in the Dockerfile. So a tool installed to
a custom location (e.g. `/opt/rust/cargo/bin`, `/usr/local/go/bin`) is
invisible in the terminal pane even though it's on PATH for non-login shells.

Two patterns, both used in the repo:

1. **Symlink the binary/proxy into `/usr/local/bin`** (on every shell's PATH).
   Used by rust (symlinks every binary in `/opt/rust/cargo/bin` to
   `/usr/local/bin`) and go (`/usr/local/go/bin/{go,gofmt}` -> `/usr/local/bin`).
   Best for a fixed set of command names.
2. **Drop a `/etc/profile.d/<x>.sh`** that exports PATH. Used by nvm, which
   needs `NVM_DIR` + sourcing `nvm.sh` at login. Best when the PATH depends on
   runtime state or there's a function to define.

A tool at `/usr/local/bin` needs neither - it's already on the default PATH. A
tool that needs `ENV PATH=/opt/foo/bin:$PATH` in the Dockerfile will be missing
in a login shell unless you also do (1) or (2). Verify with
`docker exec aio-app-1 bash -lc 'which <tool>'` (the `-lc` matters - it
replicates the terminal pane's shell).

### Rule 3 (network): HTTPS only

The sandbox network policy blocks plain HTTP (port 80) and the NodeSource host.
`curl`/`wget`/apt in a fragment must use `https://` URLs. apt sources are
already forced to HTTPS in `Dockerfile.base.head` with a ca-certs bootstrap;
don't add a plain-http apt source in a fragment. NodeSource is blocked, so
node uses nodejs.org; opencode.ai installer is blocked, so opencode uses the
GitHub release tarball. Prefer GitHub releases and official HTTPS tarballs.

### Rule 4 (versions): pin with an ARG

Even for non-TUI-versioned scenarios, pin the upstream version in an `ARG` at
the top of the fragment (`ARG GO_VERSION=1.23.4`) so builds are reproducible
and bumping is a one-line change. For a versioned scenario (with `[[versions]]`),
use `{{version}}` (and `{{tag}}` if needed) in the fragment and the real values
live in `scenario.toml`.

### Rule 5 (apt in a fragment): re-run `apt-get update`

`Dockerfile.base.head` ends with `rm -rf /var/lib/apt/lists/*`, so a scenario
that calls `apt-get install` must first `apt-get update` again (see the
`shell-utils` fragment). Forgetting this gives "package not found" at build.

## Checklist before you save a new scenario

- [ ] `scenarios/<id>/scenario.toml` with `id` == directory name, `name`,
      `description`, `category` set (don't rely on the `lang` default).
- [ ] `scenarios/<id>/fragment.Dockerfile` wrapped in `# >>>/<<< scenario: id`.
- [ ] Installs to a system path (`/usr/local`, `/opt`, `/usr/local/bin`), NOT
      `/home/gem` (unless it's intentional runtime data for the volume).
- [ ] Tool is findable in a **login** shell (symlink to `/usr/local/bin` OR a
      `/etc/profile.d/<x>.sh`), or it's already on the default PATH.
- [ ] All `curl`/`wget` URLs are HTTPS. No NodeSource. No plain-HTTP apt.
- [ ] Version pinned with an `ARG`, or `{{version}}` with matching `[[versions]]`.
- [ ] If versioned: every `{{key}}` used in the fragment is present in every
      `[[versions]]` entry (or `gen` bails on an unresolved placeholder).
- [ ] `# apt-get install` fragments start with `apt-get update`.
- [ ] `make config` to tick it (or hand-edit `enabled.toml`), `make build-base`
      to regenerate `Dockerfile.base`, verify in a container with
      `docker exec aio-app-1 bash -lc '<tool> --version'`.
- [ ] If the scenario ships a representative CLI: consider adding it to the
      full-variant probe list in `.github/workflows/images.yml` (`bash -lc` +
      `command -v` — Rule 2 is exactly what makes the probe pass; shell
      functions like `nvm` are probeable via their `profile.d` hook). A new
      scenario joins the CI full build automatically via the `["*"]` preset;
      the probe list is the only manual follow-up.
