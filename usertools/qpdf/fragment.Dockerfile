# --- tool: qpdf ---
# install: apt
# path: /opt
RUN apt-get update \
 && apt-get install -y --no-install-recommends qpdf \
 && rm -rf /var/lib/apt/lists/* \
 && qpdf --version && bash -lc 'command -v qpdf >/dev/null || { echo "MISSING(login): qpdf" >&2; exit 1; }' \
 && bash -c  'command -v qpdf >/dev/null || { echo "MISSING(non-login): qpdf" >&2; exit 1; }'
# --- /tool ---
