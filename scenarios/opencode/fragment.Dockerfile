# >>> scenario: opencode >>>
# L4 app 层:opencode AI coding agent,mise 的 aqua:anomalyco/opencode 后端。
#
# 二进制名 `opencode` 与 scenario id 同名。app 面板的 opencode 按钮(type=agent)
# 的 `enabled` 是 command_exists(opencode) 的 login-shell PATH 探测,未装即
# 自动隐藏(不产生死 pane)——故本 fragment 装好即按钮自现。
ARG OPENCODE_VERSION={{version}}
RUN mise use -g "opencode@${OPENCODE_VERSION}" \
 && mise ls opencode \
 && bash -lc 'command -v opencode >/dev/null || { echo "MISSING(login): opencode" >&2; exit 1; }' \
 && bash -c 'command -v opencode >/dev/null || { echo "MISSING(non-login): opencode" >&2; exit 1; }' \
 && bash -lc 'opencode --version'
# <<< scenario: opencode <<<
