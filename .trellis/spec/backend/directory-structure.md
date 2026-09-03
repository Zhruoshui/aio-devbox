# Directory Structure

The backend is a single axum crate at `app/`.

```
app/
├── Cargo.toml              # deps; binary name `aio-app`
├── Cargo.lock              # committed (binary crate)
├── Dockerfile              # multi-stage: builder (cargo) + web-builder (vite) + runtime
├── services.toml           # the pane registry - SINGLE SOURCE OF TRUTH (baked in via include_str!)
└── src/
    ├── main.rs             # entry: router setup, route registration, ServeDir
    ├── config.rs           # parse services.toml -> Service/ServiceType; build Manifest
    ├── state.rs            # AppState (shared across handlers via axum::State)
    ├── pty.rs              # portable-pty bridge: spawn_pty() + PtySession
    └── routes/
        ├── mod.rs          # re-exports handlers
        ├── manifest.rs     # GET /api/manifest
        ├── stats.rs        # GET /api/stats (container-view CPU/mem/disk, cgroup v2)
        ├── terminal.rs     # GET /api/term/ws (pty WS bridge)
        └── seam.rs         # 502 stub for reserved /api /v1 /mcp
```

## Conventions

- **One file per route group** under `routes/`; `routes/mod.rs` re-exports. Add a
  new route -> add a handler file + register it in `main.rs`.
- **`services.toml` is the registry** for workspace panes. Adding a service =
  a `[[service]]` entry there (+ container/profile/caddy route for `type=web`).
  It is `include_str!`-baked, so changes require an app image rebuild.
- **Static dir** is `/app/static` in the image (outside `/home/gem`, which is the
  workspace volume). Set by `STATIC_DIR` in `main.rs`.
- The Dockerfile builds the React SPA (`web/`) in a `web-builder` stage and
  copies `web/dist` into `/app/static` - the backend serves it, it does not own
  the frontend source.

## Build/host-side tooling (`config/`)

`config/` is a SEPARATE Rust crate (not part of the running stack) producing the
`aio-config` binary: `tui` (scenario picker) + `gen` (assembles `Dockerfile.base`
from `Dockerfile.base.head` + enabled `scenarios/<id>/fragment.Dockerfile` +
`Dockerfile.base.tail`). Built into the `aio-config` image, run via `docker run`
from the Makefile. See `.trellis/tasks/08-03-scenario-preset-profiles/`.

**Scenario fragment rules (must follow when adding `scenarios/<id>/`)**:
- Fragments run as root (inserted before the tail's `USER gem`).
- Install to **system paths** (`/opt`, `/usr/local`), NEVER `/home/gem/*` -- the
  workspace named volume masks `/home/gem`, so baked-in tools there go stale.
- Tools in a **custom bin dir** are NOT on the login-shell PATH: `bash -l`
  sources `/etc/profile` which resets PATH, dropping the custom dir. Two
  accepted patterns: **symlink the proxies into `/usr/local/bin`** (fixed
  command set), or the mise scenario's **dual-channel** approach — ENV
  (`PATH=/opt/mise/shims:$PATH`, inherited by every process) plus
  `/etc/profile.d/mise.sh` re-exporting the env vars and running
  `eval "$(mise activate bash)"` for login shells. Tools installed
  directly to `/usr/local/bin` (the mise binary itself, node) need neither.
- `id` in `scenario.toml` must equal the directory name (`gen` enforces it).

## Common Mistake: `RUN chmod` after `FROM sandbox-base`

**Symptom**: `chmod: changing permissions of ...: Operation not permitted`
during `docker compose build app`.

**Cause**: `Dockerfile.base` ends with `USER gem`, so every stage derived via
`FROM sandbox-base` runs its `RUN`s as `gem`. `COPY` defaults to root
ownership, and a non-root `RUN chmod` on a root-owned file is EPERM.

**Fix / Prevention**: set the mode on the `COPY` itself - BuildKit needs no
shell and no privileges for it:

```dockerfile
# Wrong (EPERM: RUN executes as gem, file is root-owned)
COPY app/entrypoint.sh /usr/local/bin/aio-entrypoint.sh
RUN chmod 755 /usr/local/bin/aio-entrypoint.sh

# Correct
COPY --chmod=755 app/entrypoint.sh /usr/local/bin/aio-entrypoint.sh
```
