# >>> scenario: jq >>>
# L2 shell 层:jq —— JSON 命令行处理器
#
#   二进制名即 `jq`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG JQ_VERSION={{version}}
RUN mise use -g "jq@${JQ_VERSION}" \
 && mise ls jq \
 && bash -lc 'command -v jq >/dev/null || { echo "MISSING(login): jq" >&2; exit 1; }' \
 && bash -c 'command -v jq >/dev/null || { echo "MISSING(non-login): jq" >&2; exit 1; }' \
 && bash -lc 'jq --version'
# <<< scenario: jq <<<
