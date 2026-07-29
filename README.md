# AIO-style Dev Sandbox

A self-hosted remote development environment: `docker compose up` starts a caddy
gateway plus pluggable service containers, and a web UI presents a workspace of
panes (code-server, VNC/Chromium, terminal, opencode). Personal use.

This repository is being built phase by phase (see
`.trellis/tasks/07-28-server-mvp/implement.md`). **Phase A** delivers the base
image, the compose skeleton, the caddy gateway (basicauth), and a minimal axum
app returning `ok` on `/`.

## Architecture (Phase A)

```
host :8080 -> caddy gateway (basicauth) --reverse_proxy--> app (axum) :8088
                                                            USER gem (uid 1000)
                                                            volume: /home/gem
```

- `sandbox-base` (Dockerfile.base): Debian slim + node 20 + python3 + git +
  opencode + user `gem` (uid 1000). Shared base image for `app`, `code-server`,
  and (later) `vnc`.
- `app` (app/Dockerfile, multi-stage): axum server, `FROM sandbox-base`.
- `gateway` (caddy:2): basicauth + reverse proxy to `app:8088`.

## Build order

`app/Dockerfile` is `FROM sandbox-base`, so `sandbox-base` must be built and
tagged **before** `app`. The Makefile handles this:

```sh
make build-base   # docker build -t sandbox-base -f Dockerfile.base .
make build        # build-base, then docker compose build (builds app)
make up           # build-base, ensure auth hash, then docker compose up -d --build
```

`docker compose --profile build build base` also tags `sandbox-base` (the `base`
service is gated behind the `build` profile so it never starts at runtime).

## Auth

The gateway uses caddy `basicauth` (user `admin`, password `admin` by default).
The bcrypt hash contains `$` characters, which this docker-compose build
corrupts when passed through `env_file`/`environment` (it interpolates `$VAR`
patterns inside env values). The hash is therefore generated to
`gateway/secrets/hash` (gitignored) and delivered to caddy via
`gateway/entrypoint.sh`, which exports it before exec'ing caddy. The Caddyfile
still uses the `{$SANDBOX_PASSWORD_HASH}` placeholder as designed.

```sh
make hash              # generate hash for password "admin" (default)
make hash PASS=secret  # custom password
```

## Run (Phase A)

```sh
make up
curl -u admin:admin http://localhost:8080/   # -> ok (200)
curl http://localhost:8080/                  # -> 401
make down
```

## Layout

```
Dockerfile.base        sandbox-base image (shared dev-env layer)
app/                   axum app (Cargo.toml, src/main.rs, Dockerfile)
gateway/               Caddyfile + entrypoint.sh (+ secrets/hash, generated)
docker-compose.yml     gateway + app (+ base under build profile)
Makefile               build-base / build / up / hash / down / clean
.env / .env.example    SANDBOX_USER (hash is generated, not env-delivered)
```
