# >>> scenario: fzf >>>
# L2 shell 层:fzf —— 命令行模糊查找器(补全/历史搜索/文件选择)
#
#   二进制名即 `fzf`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG FZF_VERSION={{version}}
RUN mise use -g "fzf@${FZF_VERSION}" \
 && mise ls fzf \
 && bash -lc 'command -v fzf >/dev/null || { echo "MISSING(login): fzf" >&2; exit 1; }' \
 && bash -c 'command -v fzf >/dev/null || { echo "MISSING(non-login): fzf" >&2; exit 1; }' \
 && bash -lc 'fzf --version'
# <<< scenario: fzf <<<
