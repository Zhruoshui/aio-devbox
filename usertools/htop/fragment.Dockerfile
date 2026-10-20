# --- tool: htop ---
# install: apt
RUN apt-get update \
 && apt-get install -y --no-install-recommends htop \
 && rm -rf /var/lib/apt/lists/* \
 && bash -lc 'command -v htop >/dev/null || { echo "MISSING(login): htop" >&2; exit 1; }' \
 && bash -c  'command -v htop >/dev/null || { echo "MISSING(non-login): htop" >&2; exit 1; }'
# --- /tool ---
