# >>> scenario: bat >>>
# L2 shell 层:bat —— 带语法高亮的 cat 替代(含 Git 集成)
#
#   二进制名即 `bat`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG BAT_VERSION={{version}}
RUN mise use -g "bat@${BAT_VERSION}" \
 && mise ls bat \
 && bash -lc 'command -v bat >/dev/null || { echo "MISSING(login): bat" >&2; exit 1; }' \
 && bash -c 'command -v bat >/dev/null || { echo "MISSING(non-login): bat" >&2; exit 1; }' \
 && bash -lc 'bat --version'
# <<< scenario: bat <<<
