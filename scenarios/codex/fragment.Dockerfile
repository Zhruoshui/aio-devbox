# >>> scenario: codex >>>
# L4 app 层:OpenAI Codex CLI,mise 的 aqua:openai/codex 后端。
#
# 二进制名 `codex` 与 scenario id 同名(mise registry 另有 npm: 后端,此处用 aqua)。
#
# ⚠️ 许可提示:Codex CLI 是闭源商业工具,烘进镜像后随 `make save` 分发
#   前请自行确认其许可条款。
ARG CODEX_VERSION={{version}}
RUN mise use -g "codex@${CODEX_VERSION}" \
 && mise ls codex \
 && bash -lc 'command -v codex >/dev/null || { echo "MISSING(login): codex" >&2; exit 1; }' \
 && bash -c 'command -v codex >/dev/null || { echo "MISSING(non-login): codex" >&2; exit 1; }' \
 && bash -lc 'codex --version'
# <<< scenario: codex <<<
