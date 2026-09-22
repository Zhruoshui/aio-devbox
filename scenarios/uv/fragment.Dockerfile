# >>> scenario: uv >>>
# L3 lang 层:uv(Python 包与项目管理器),mise 的 aqua:astral-sh/uv 后端。
#
# `mise use -g` 是读改写语义,与其他 mise 工具 fragment 在顺序层上幂等可组合。
ARG UV_VERSION={{version}}
RUN mise use -g "uv@${UV_VERSION}" \
 && mise ls uv \
 && bash -lc 'for t in uv uvx; do command -v "$t" >/dev/null || { echo "MISSING(login): $t" >&2; exit 1; }; done' \
 && bash -c 'command -v uv >/dev/null || { echo "MISSING(non-login): uv" >&2; exit 1; }' \
 && bash -lc 'uv --version && uvx --version'
# <<< scenario: uv <<<
