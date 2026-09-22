# >>> scenario: eza >>>
# L2 shell 层:eza —— 现代 ls 替代(图标/树形/Git 状态)
#
#   二进制名即 `eza`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG EZA_VERSION={{version}}
RUN mise use -g "eza@${EZA_VERSION}" \
 && mise ls eza \
 && bash -lc 'command -v eza >/dev/null || { echo "MISSING(login): eza" >&2; exit 1; }' \
 && bash -c 'command -v eza >/dev/null || { echo "MISSING(non-login): eza" >&2; exit 1; }' \
 && bash -lc 'eza --version'
# <<< scenario: eza <<<
