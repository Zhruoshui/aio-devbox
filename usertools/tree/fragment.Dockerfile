# --- tool: tree ---
# install: apt
RUN apt-get update \
 && apt-get install -y --no-install-recommends tree \
 && rm -rf /var/lib/apt/lists/* \
 && bash -lc 'command -v tree >/dev/null || { echo "MISSING(login): tree" >&2; exit 1; }' \
 && bash -c  'command -v tree >/dev/null || { echo "MISSING(non-login): tree" >&2; exit 1; }'
# --- /tool ---
