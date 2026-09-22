# Paths, Persistence & Offline

This explains where a tool can live in the AIO sandbox, why, and how offline
installs differ from baked-in scenarios. The key distinction is **baked
(image layer)** vs **runtime (shared volume)** vs **container writable layer**.

## The three install locations

| Location | What's there | Survives container recreate? | Visible to | When to use |
|---|---|---|---|---|
| Image layer (baked) | `/usr/local`, `/opt`, `/usr/local/bin`, `/etc/profile.d` | Yes (it's in the image) | every container `FROM sandbox-base` | persistent system tooling - **scenarios install here** |
| Shared volume | `/root/...` (`~/.local/bin`, user runtime data) | Yes (named volume `aio_workspace`) | app, code-server, vnc (all mount the volume) | runtime user data + self-contained runtime tools |
| Container writable layer | `/usr`, `/etc` written at runtime (not via a build) | NO (gone on recreate) | only that container | temporary / verification only |

The `sandbox-base` image + scenarios own the **baked** row. Everything you do
with `aio-config gen` / `make build-base` produces image-layer content.
`/root` is the **shared volume** and is intentionally NOT baked - the
volume mounts over it.

## The volume-masking rule (again, because it's the #1 bug)

The named volume `aio_workspace` mounts at `/root` in the app, code-server,
and vnc containers. At runtime, `/root` shows the **volume's** contents, not
the image layer's. So if a scenario does `RUN install -m 0755 foo
/root/.local/bin/foo`, then `foo` is in the image layer but **masked** - the
container's `/root/.local/bin` is whatever the volume has (probably empty on
a fresh volume), not your installed binary. The bake silently does nothing at
runtime.

That's why every scenario installs to a system path (`/usr/local/bin`, `/opt`,
`/usr/local`) that the volume doesn't cover. The only things that go under
`/root` are written **at runtime** (by the user), so they're meant to live on
the volume and survive recreate. The mise scenario redirects ALL its state
(`MISE_DATA_DIR`, `MISE_CONFIG_DIR`, `RUSTUP_HOME`, `CARGO_HOME` → `/opt/mise`)
for exactly this reason - mise's defaults all live under `/root` and would be
masked.

## The ~/.local/bin auto-PATH trick (runtime tools)

Debian's default `~/.profile` (which login bash sources) contains:

```sh
if [ -d "$HOME/.local/bin" ] ; then
    PATH="$HOME/.local/bin:$PATH"
fi
```

So anything you drop in `/root/.local/bin` (on the shared volume) is
automatically on PATH in **every login shell** - across the app, code-server,
and vnc containers (they all run login shells and share the volume), and it
**survives container recreate** because it's on the named volume. This is the
go-to location for self-contained binaries you want at runtime without an
image rebuild: `docker cp` a static binary in, or `curl` it inside the terminal
pane, `chmod +x`, done.

This is the **runtime install** path - when the user wants to "just try X
once" or install a tool without rebuilding, point them at `~/.local/bin`. The
tradeoff: it's not in the image, so a fresh checkout / different machine won't
have it. To make it permanent, promote the runtime install into a scenario
(baked to `/usr/local/bin`).

## Baked vs runtime - which to choose

| | Baked scenario | Runtime `~/.local/bin` |
|---|---|---|
| Survives recreate | Yes (image) | Yes (volume) |
| On a fresh checkout | Yes | No |
| Needs `make build-base` | Yes | No |
| Version selectable in TUI | Yes (if versioned) | No |
| Toggleable per-image | Yes (TUI tick) | No (it's there or it isn't) |
| Cross-machine reproducible | Yes | No |

Default to a **baked scenario** for anything that's part of the environment
("this sandbox ships Go"). Use a **runtime install** for one-off tools, trials,
or user-specific tooling. The comprehensive-plan case (the headline use of
this skill) is almost always baked scenarios.

## Offline installs (the air-gapped path)

There's a separate, thorough offline guide at `docs/offline-install-guide.md`
(the methodology) and `docs/offline-tool-install.md` (test records). The
short version for this skill:

Offline install = get a **self-contained artifact** on an online machine, move
it to the offline host, install it into the shared volume (`~/.local/bin`) -
without rebuilding or touching the network from the running stack. Three
primitives: online prepare -> transfer (`docker cp`) -> offline install to
`~/.local/bin` with `chmod +x` (containers run as root - no chown needed).

- **Static/musl binary** (ripgrep, fd, uv): one file to `~/.local/bin`. Easiest.
- **npm global package**: tar `bin/` + `lib/node_modules/` together, extract to
  `~/.local` (they relative-reference each other).
- **Python wheelhouse**: `pip download` (with native wheels) on the online
  machine, `uv pip install --no-index --find-links` offline into a venv on the
  volume.
- **apt package**: `apt-get install --download-only` to collect `.deb`s, `dpkg
  -i` offline. But this lands on the **container writable layer** (gone on
  recreate) - so apt is only for temporary verification; for persistence bake
  it into the image (`docker save`/`load`) or make it a scenario.
- **From source**: online machine bundles source + vendored deps + the toolchain
  (e.g. `cargo vendor` for Rust); offline machine builds with `--offline`.

Artifacts must match the offline machine's architecture (x86_64), glibc
(bookworm 2.36), and language ABI (e.g. cp311). When a baked scenario won't do
and the user is on an air-gapped box, this is the path - and it installs to the
volume, not a scenario.

## Prebuilt images from GHCR (`make pull`)

A third way to get the baked images, the online mirror of the offline
`make save`/`load` flow: the GitHub Actions pipeline
(`.github/workflows/images.yml`) builds every push to `main` (and every `v*`
tag) and publishes to GHCR, and `make pull` fetches those instead of building
locally or shipping a bundle.

- `make pull VARIANT=minimal|full` pulls `sandbox-base` / `sandbox-app` /
  `sandbox-code-server` at the floating `:minimal`/`:full` tag plus
  `sandbox-vnc:latest`, retags them to the local compose names, and prepares the
  gitignored host file the stack needs to start (`.env`, copied from the example
  if missing). `REGISTRY_PREFIX` (default
  `ghcr.io/zhruoshui`, this repo's pipeline target; override to pull a fork)
  points the pull at your GHCR namespace.
- It does NOT touch `.aio/enabled.toml`: a pure consumer doesn't care about the
  scenario selection, and `make up NOBUILD=1` skips `gen`. The pulled images are
  exactly what a scenario bake would produce - the scenario-authoring rules in
  this skill still govern what's IN the image; `pull` just skips the local build.
- Choose: `make load` for an air-gapped machine (a `make save` bundle), `make
  pull` for an online machine that doesn't want to build. Both end the same way:
  `make up NOBUILD=1 PROFILES="code-server vnc"`.

## Persistence summary (decide where a thing goes)

```
Does it need to be part of the shipped image (reproducible, on fresh checkouts)?
├─ YES -> scenario fragment, install to a SYSTEM path (/usr/local, /opt). make build-base.
└─ NO  -> runtime install on the shared volume
          ├─ self-contained binary/script -> ~/.local/bin  (auto-PATH, survives recreate)
          ├─ offline mise tool transfer -> whole-data-dir move to the SAME absolute path
          │    (see docs/offline-tool-install.md §14; MISE_DATA_DIR + MISE_CONFIG_DIR
          │     must be overridden together, single tar carries both)
          └─ apt package (temporary only) -> container writable layer (gone on recreate; don't rely on it)
```

Note on mise: the **engine** and every **baked tool** live at `/opt/mise`
(image layer) — that is forced by the workspace volume covering `/root`.

Runtime `mise use -g <tool>` nevertheless **survives recreate** (since
2026-09-22). At app-container boot, `app/aio-mise-volume.sh` seeds
`~/.local/share/mise` on the volume: it symlinks the image's `installs/`,
`rustup/` and `cargo/` trees (so the 1.9GB is NOT copied — the volume costs
tens of KB), runs `mise reshim` there, and regenerates the volume's
`config.toml` from the image's (authoritative for baked tools) plus the user's
own entries. `/etc/profile.d/mise.sh` then probes for that seeded layout and
picks the volume when present, falling back to the image layout otherwise.

The probe is in `profile.d`, so it only fires for **login shells** (the WebUI
terminal, code-server's terminal, `bash -lc`). A non-login `docker exec -it
<c> bash` inherits the image's ENV channel and stays on the BAKED layout —
deliberate: ENV is a build-time constant, and pointing it at the volume path
would leave a no-volume `docker run --rm <base> bash` with an empty data dir
instead of the baked toolchains, losing the only fallback. Use a login shell
when you need `mise use -g` to persist.

Four traps this design had to work around, worth knowing before touching it:
`rustup/`+`cargo/` must be linked too (else rust breaks); `MISE_GLOBAL_CONFIG_FILE`
**replaces** rather than layers, so it must NOT be set; the volume's `shims/`
cannot be a symlink to the image's (mise rejects foreign shims — use `mise
reshim`); and `mise activate` strips the shim dirs from PATH, so they must be
re-appended *after* the activate call.

The offline whole-directory recipe above remains the supported way to add
mise-managed tools to an offline machine.
