# >>> scenario: claude-code >>>
# L4 app 层:Claude Code,mise 的 aqua:anthropics/claude-code 后端。
#
# ⚠️ 两个命名注意:
#   1) scenario id 是 `claude-code`(目录名须与 id 一致),但 mise 装出的
#      二进制名是 **`claude`** —— 自检必须用 `claude`。
#   2) mise registry 同时提供 `http:claude` 后端;这里用 aqua(与其余 agent
#      一致,带 checksum 校验)。
#
# ⚠️ 许可提示:Claude Code 是闭源商业工具,烘进镜像后随 `make save` 分发
#   前请自行确认其许可条款。
ARG CLAUDE_CODE_VERSION={{version}}
RUN mise use -g "claude-code@${CLAUDE_CODE_VERSION}" \
 && mise ls claude-code \
 && bash -lc 'command -v claude >/dev/null || { echo "MISSING(login): claude" >&2; exit 1; }' \
 && bash -c 'command -v claude >/dev/null || { echo "MISSING(non-login): claude" >&2; exit 1; }' \
 && bash -lc 'claude --version'
# <<< scenario: claude-code <<<
