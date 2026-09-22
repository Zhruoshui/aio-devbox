# >>> scenario: fd >>>
# L2 shell 层:fd —— 用户友好的 find 替代
#
#   二进制名即 `fd`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG FD_VERSION={{version}}
RUN mise use -g "fd@${FD_VERSION}" \
 && mise ls fd \
 && bash -lc 'command -v fd >/dev/null || { echo "MISSING(login): fd" >&2; exit 1; }' \
 && bash -c 'command -v fd >/dev/null || { echo "MISSING(non-login): fd" >&2; exit 1; }' \
 && bash -lc 'fd --version'
# <<< scenario: fd <<<
