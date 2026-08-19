# Compose, Registry & Buttons (L5 / WebUI)

This covers the two ways a tool or service becomes visible/launchable in the
WebUI, plus the compose profiles that gate the heavier containerized services.

## Three surfaces, increasing scope

| Surface | What it does | Image rebuild needed? | File |
|---|---|---|---|
| Runtime button | A sidebar button that runs a CLI in a pty pane | No (writes a file in the running container) | `/home/gem/.aio/buttons.toml` (in the container) |
| Built-in button | Same as above but shipped with the image | Yes (app rebuild, `include_str!`) | `app/services.toml` |
| Web service (pane) | A whole container with its own port, served in an iframe via the gateway | Yes (compose + caddy + app rebuild) | `docker-compose.yml`, `gateway/Caddyfile`, `app/services.toml` |

## WebUI button / pane (app/services.toml)

The axum app parses `app/services.toml` at **compile time** (`include_str!`) and
exposes `GET /api/manifest`. The React frontend renders one button per entry.

Two `type`s:

- **`type = "web"`**: a containerized service shown in an iframe. `target` is
  `host:port` for a TCP liveness probe (the manifest's `enabled` field = is the
  container reachable). `url` is the gateway path the iframe opens. Needs the
  matching compose service + caddy route (see below). Existing: `codeServer`
  (`code-server:8200`, url `/code-server/`), `vnc` (`vnc:6080`, url
  `/vnc/vnc.html?...&path=vnc/websockify`).
- **`type = "agent"`**: a CLI launched in an xterm pty pane. `cmd` is the
  command; `""` = default login shell (the `terminal` button). `enabled` =
  whether `cmd` is on the login-shell PATH (a `command_exists` probe at manifest
  time, with a 60s PATH cache). So the button **auto-shows only when the tool
  is actually installed** - bake the scenario and the button appears; don't
  bake it and it hides. This replaced the old `ENABLE_*` env-var scheme that
  produced "dead panes" (button shown, tool missing). Existing: `terminal` (cmd
  `""`, always present), `opencode` (cmd `opencode`, shown only if the
  `opencode` scenario is baked).

`label` is optional (falls back to a friendly form of `id`).

So for an **L4 AI agent CLI** you want as a pane:
1. Bake it as a scenario (L4 `app`) so the binary lands on the PATH.
2. Add a `[[service]]` with `type = "agent"`, `id = "<stable-id>"`,
   `cmd = "<binary>"`, `label = "..."` to `app/services.toml`.
3. Rebuild the app image (`make up` rebuilds it; it's `include_str!` so a
   restart is NOT enough).
4. The button auto-shows because `command_exists(opencode)` is true.

For a **runtime-only button** (no image rebuild), the MVP supports only
`type = "agent"` buttons registered via `POST /api/buttons`, which writes
`/home/gem/.aio/buttons.toml` on the shared volume (survives recreate). Use
this when a tool was installed at runtime into `~/.local/bin` (see
paths-and-offline.md) and you want a button for it.

## Adding a containerized web service (L5)

This is the heaviest path - a new pane with its own container. Three files must
change together, or the button shows but the iframe 404s. Walk through each:

### 1. docker-compose.yml

Add a new service. Gate it behind a `profiles: [<name>]` so it only starts when
the user runs `make up PROFILES=<name>` (matches the code-server/vnc pattern -
they're opt-in). Mount the shared `workspace` volume if the service needs to
read `/home/gem`, expose its internal port, and join `sandbox-net`:

```yaml
my-service:
  build:
    context: .
    dockerfile: my-service/Dockerfile
  image: sandbox-my-service
  restart: unless-stopped
  profiles:
    - my-service
  expose:
    - "<port>"
  volumes:
    - workspace:/home/gem        # only if it needs the shared files
  networks:
    - sandbox-net
```

`expose:` (not `ports:`) - the gateway reverse-proxies; the port is internal to
`sandbox-net`, not published to the host.

### 2. gateway/Caddyfile

Add a `handle_path /<prefix>/*` block **before** the catch-all `handle` (caddy
evaluates `handle`/`handle_path` in source order - a later catch-all would
shadow an earlier route). The block strips the prefix and proxies to the
container on `sandbox-net`:

```caddyfile
:8080 {
    basicauth { {$SANDBOX_USER} {$SANDBOX_PASSWORD_HASH} }
    handle_path /my-service/* {
        reverse_proxy my-service:<port>
    }
    handle_path /code-server/* { reverse_proxy code-server:8200 }
    handle_path /vnc/*         { header { Cache-Control no-store } reverse_proxy vnc:6080 }
    handle { reverse_proxy app:8088 }
}
```

The subpath-strip pattern works because the target app emits **relative** asset
URLs (the browser resolves them against `/my-service/` and caddy strips the
prefix on the way back). If the app emits **absolute** URLs that assume `/`, it
won't work behind a subpath and you need the app's own base-path config (like
code-server lacks, hence the strip workaround) or a dedicated root (harder here
since caddy's basicauth wraps everything under `:8080`).

### 3. app/services.toml

Add the `[[service]]` so the manifest surfaces a button:

```toml
[[service]]
id = "my-service"
type = "web"
target = "my-service:<port>"
url = "/my-service/"
label = "My Service"
```

`target` drives the TCP liveness probe (button hides when the container is
down, e.g. its profile isn't enabled). `url` is what the iframe opens.

Then **rebuild the app image** (services.toml is compiled in):
`make up` (which does `compose up -d --build`) or `docker compose build app`.

### 4. Enable + run + verify

```bash
make up PROFILES=my-service        # starts the new container
docker exec aio-app-1 sh -c 'curl -sS -o /dev/null -w "%{http_code}" http://my-service:<port>/'
```

The button should now show in the WebUI (manifest `enabled=true` because the
container is reachable on `sandbox-net`). The gateway serves it at
`http://<host>:8080/my-service/` behind the basicauth.

## Compose profiles

`docker-compose.yml` gates optional services behind `profiles:`:

- `code-server` - VS Code in the browser (profile `code-server`, port 8200).
- `vnc` - Chromium in the browser (profile `vnc`, port 6080, `shm_size 2gb`).
- `build` - the `sandbox-base` image build target, never a runtime container.

`make up` builds + starts the always-on services (gateway + app). To start
optional services: `make up PROFILES=code-server` or
`make up PROFILES=code-server,vnc`. `make down` takes the same `PROFILES=` so
profile services are torn down with their counterparts.

`NOBUILD=1 make up` skips the base build entirely (for offline machines that
`docker load` pre-built images). Don't use it when you've changed a scenario -
the changed fragment only lands via `make build-base` -> `Dockerfile.base`.

## Why vnc is NOT a scenario

`vnc/Dockerfile` is `FROM debian:bookworm-slim`, not `FROM sandbox-base`.
It's a pure browser surface (Chromium + noVNC) and doesn't need the dev
tooling the scenarios bake. Decoupling it means adding a rust/go/python-dev
scenario doesn't stale-date or rebuild the vnc image. So scenarios only affect
`sandbox-base` (and the `app`/`code-server` images derived from it). If a
request is "add X to the vnc container", that's a `vnc/Dockerfile` edit, not
a scenario - and it's unusual; vnc is meant to stay thin.
