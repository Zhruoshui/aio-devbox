# >>> scenario: zoxide >>>
# L2 shell 层:zoxide —— 智能 cd 替代(按访问频率跳转)
#
#   二进制名即 `zoxide`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG ZOXIDE_VERSION={{version}}
RUN mise use -g "zoxide@${ZOXIDE_VERSION}" \
 && mise ls zoxide \
 && bash -lc 'command -v zoxide >/dev/null || { echo "MISSING(login): zoxide" >&2; exit 1; }' \
 && bash -c 'command -v zoxide >/dev/null || { echo "MISSING(non-login): zoxide" >&2; exit 1; }' \
 && bash -lc 'zoxide --version'
# <<< scenario: zoxide <<<
