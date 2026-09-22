# >>> scenario: ripgrep >>>
# L2 shell 层:ripgrep —— 极快的递归搜索工具(grep 替代)
#
#   ⚠️ 该工具的二进制名是 `rg`(不是 `ripgrep`),自检必须用 `rg`。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG RIPGREP_VERSION={{version}}
RUN mise use -g "ripgrep@${RIPGREP_VERSION}" \
 && mise ls ripgrep \
 && bash -lc 'command -v rg >/dev/null || { echo "MISSING(login): rg" >&2; exit 1; }' \
 && bash -c 'command -v rg >/dev/null || { echo "MISSING(non-login): rg" >&2; exit 1; }' \
 && bash -lc 'rg --version'
# <<< scenario: ripgrep <<<
