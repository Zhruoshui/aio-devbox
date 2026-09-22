# >>> scenario: shell-tools >>>
# L2 shell 层:常用基线工具(apt 系统路径)。
#
# 为什么与 mise 侧分开:
#   L2 的另外 10 个工具(fzf/ripgrep/bat/fd/eza/zoxide/delta/starship/jq/yq)
#   都是 mise 托管的「现代替代品」,有版本维度、可独立升降级。本场景装的则是
#   **各发行版通用基线**:wget/tree/less/file/zip/vim —— 脚本、Makefile、CI、
#   文档里默认它们存在,没有版本选择需求。两者互补,不重叠。
#   (installer = "apt" 使 L2 出现混用,EnvPicker/TUI 因此渲染两级子标题 ——
#    见 scenario.rs::has_multiple_installers,这是预期效果。)
#
# 安装到系统路径(/usr/bin、/usr/local/bin),不被共享卷 aio_workspace 遮盖。
# apt lists 在 head 末尾被 `rm -rf /var/lib/apt/lists/*` 清空,这里需重新 update。
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        wget tree less file zip vim \
 && rm -rf /var/lib/apt/lists/* \
# 双通道自检:WebUI 终端面板走 login shell(bash -l),code-server 默认非 login。
# 两条都要过,只过一条意味着某个终端里工具"消失"。
 && for t in wget tree less file zip vim; do \
      bash -lc "command -v $t >/dev/null || { echo \"MISSING(login): $t\" >&2; exit 1; }"; \
      bash -c  "command -v $t >/dev/null || { echo \"MISSING(non-login): $t\" >&2; exit 1; }"; \
    done \
# 冒烟:每个工具跑一次版本输出,确认不只是个可执行的空壳。
 && wget --version | head -n1 \
 && tree --version \
 && less --version | head -n1 \
 && file --version | head -n1 \
 && zip --version | head -n3 | tail -n1 \
 && vim --version | head -n1
# <<< scenario: shell-tools <<<
