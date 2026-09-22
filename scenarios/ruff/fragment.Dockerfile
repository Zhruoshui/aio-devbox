# >>> scenario: ruff >>>
# L3 lang 层:ruff(Python linter + formatter),mise 的 aqua:astral-sh/ruff 后端。
#
# `mise use -g` 是读改写语义,与其他 mise 工具 fragment 在顺序层上幂等可组合。
ARG RUFF_VERSION={{version}}
RUN mise use -g "ruff@${RUFF_VERSION}" \
 && mise ls ruff \
 && bash -lc 'command -v ruff >/dev/null || { echo "MISSING(login): ruff" >&2; exit 1; }' \
 && bash -c 'command -v ruff >/dev/null || { echo "MISSING(non-login): ruff" >&2; exit 1; }' \
 && bash -lc 'ruff --version'
# <<< scenario: ruff <<<
