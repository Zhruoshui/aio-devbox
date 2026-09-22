# >>> scenario: starship >>>
# L2 shell 层:starship —— 跨 shell 提示符(prompt)定制
#
#   二进制名即 `starship`(mise 装的即规范名;无 Debian 的 batcat/fdfind 改名问题)。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG STARSHIP_VERSION={{version}}
RUN mise use -g "starship@${STARSHIP_VERSION}" \
 && mise ls starship \
 && bash -lc 'command -v starship >/dev/null || { echo "MISSING(login): starship" >&2; exit 1; }' \
 && bash -c 'command -v starship >/dev/null || { echo "MISSING(non-login): starship" >&2; exit 1; }' \
 && bash -lc 'starship --version'
# <<< scenario: starship <<<
