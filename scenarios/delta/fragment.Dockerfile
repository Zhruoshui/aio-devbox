# >>> scenario: delta >>>
# L2 shell 层:delta —— Git diff 语法高亮分页器
#
#   二进制名即 `delta`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG DELTA_VERSION={{version}}
RUN mise use -g "delta@${DELTA_VERSION}" \
 && mise ls delta \
 && bash -lc 'command -v delta >/dev/null || { echo "MISSING(login): delta" >&2; exit 1; }' \
 && bash -c 'command -v delta >/dev/null || { echo "MISSING(non-login): delta" >&2; exit 1; }' \
 && bash -lc 'delta --version'
# <<< scenario: delta <<<
