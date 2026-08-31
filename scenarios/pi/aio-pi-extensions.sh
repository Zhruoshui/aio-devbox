#!/usr/bin/env bash
# aio-pi-extensions — register the image-baked pi packages (idempotent, OFFLINE).
#
# Package CONTENT is baked into the image at /opt/pi-extensions (see
# scenarios/pi/pi-packages/package.json — its dependencies list IS the pi
# scenario's "subset config"). pi loads packages listed in
# ~/.pi/agent/settings.json, and ~/.pi == /root/.pi lives on the shared
# aio_workspace volume — masked at build time, so the *registration* must
# happen at runtime, once per volume. This script does exactly that using
# local-path package entries: `pi install /abs/path` adds the package to
# settings WITHOUT copying and WITHOUT network, so it also works on offline
# machines that got the images via `docker load` + `make load`.
#
# It also migrates pre-baked `npm:<name>` entries (the old runtime-install
# scheme, which needed npm network) to the baked local paths, so the two
# schemes never double-load the same package.
#
# Run once in any terminal pane (or: docker exec -u 1000:1000 aio-app-1
# bash -lc aio-pi-extensions). Safe to re-run: already-registered paths are
# skipped.
set -euo pipefail

# Zero-network guarantee for the pi invocations below: PI_OFFLINE disables
# startup/update/telemetry network ops; local-path install/remove are local
# file + settings operations. (Keep PI_SKIP_VERSION_CHECK/PI_TELEMETRY as
# belt-and-braces for pi versions predating PI_OFFLINE.)
export PI_OFFLINE=1
export PI_SKIP_VERSION_CHECK=1
export PI_TELEMETRY=0

BAKED_DIR="/opt/pi-extensions"
MANIFEST="${BAKED_DIR}/package.json"
SETTINGS="${PI_CODING_AGENT_DIR:-${HOME:-/root}/.pi/agent}/settings.json"

command -v pi >/dev/null 2>&1 || {
  echo "aio-pi-extensions: 'pi' not on PATH (scenario not baked?)" >&2
  exit 1
}
[ -f "$MANIFEST" ] || {
  echo "aio-pi-extensions: $MANIFEST missing (image built without the pi-packages bake?)" >&2
  exit 1
}

# pi rewrites absolute install paths to settings-RELATIVE ones (e.g.
# "../../../../opt/pi-extensions/node_modules/<name>", resolved against the
# settings file - pi list prints the resolved absolute path). So idempotency
# checks must match the unambiguous `node_modules/<name>` suffix, not the
# full absolute path.
registered() { local name="$1"; [[ -f "$SETTINGS" ]] && grep -qF "node_modules/${name}\"" "$SETTINGS"; }
npm_entry()  { local name="$1"; [[ -f "$SETTINGS" ]] && grep -qF "\"npm:${name}\"" "$SETTINGS"; }

# The baked package list = the subset config (single source of truth, same
# file the build installed from).
mapfile -t PACKAGES < <(node -e '
  const m = require(process.argv[1]);
  for (const name of Object.keys(m.dependencies || {})) console.log(name);
' "$MANIFEST")

echo "baked pi packages: ${PACKAGES[*]}"
for name in "${PACKAGES[@]}"; do
  path="${BAKED_DIR}/node_modules/${name}"
  if registered "$name"; then
    echo "already registered: $name"
    continue
  fi
  if npm_entry "$name"; then
    echo "migrating npm:${name} -> baked path"
    pi remove "npm:${name}"
  fi
  echo "registering: $path"
  pi install "$path"
done
echo "done - verify with: pi list"
