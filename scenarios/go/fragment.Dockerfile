# >>> scenario: go >>>
# L3 lang 层:Go 工具链,由 mise 的 core:go 后端安装。
#
# `mise use -g` 是读改写语义(读现有 config → 合并 [tools] → 写回),因此本
# fragment 与其他 mise 工具 fragment 在 Docker 顺序层上幂等可组合,无需聚合。
ARG GO_VERSION={{version}}
RUN mise use -g "go@${GO_VERSION}" \
 && mise ls go \
 && bash -lc 'for t in go gofmt; do command -v "$t" >/dev/null || { echo "MISSING(login): $t" >&2; exit 1; }; done' \
 && bash -c 'command -v go >/dev/null || { echo "MISSING(non-login): go" >&2; exit 1; }' \
 && bash -lc 'go version && gofmt -h >/dev/null 2>&1; echo "gofmt ok"'
# <<< scenario: go <<<
