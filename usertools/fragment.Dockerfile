# >>> scenario: user-tools >>>
# L2 shell 层: user-tools —— 用户自定义工具层（非 mise 工具的逃生通道）。
#
# 本文件由 sandbox-mgr 的「自定义工具」页维护。每个工具是一个
# `# --- tool: <name> ---` / `# --- /tool ---` 标记块（字段写在块内的注释行里）。
# 也可以直接用「直接编辑 Dockerfile」模式手写——一旦内容无法解析为条目，
# 表单模式会自动禁用。
#
# 三条硬规则（违反任意一条，工具在容器里就“消失了”）：
#
#   1. 安装到系统路径：单二进制 → /usr/local/bin；目录树 → /opt/<tool>。
#      **绝不写 /root** —— 共享工作区卷挂 /root，镜像层内容会被遮盖。
#
#   2. 双通道自检（生成的块自带这两条）：
#        bash -lc '<tool> --version'    # WebUI 终端面板是 login shell
#        bash  -c '<tool> --version'    # code-server 默认非 login shell
#
#   3. 每次修改后：
#        make gen && docker compose build app && docker compose up -d --force-recreate app
#      **不要** `make build-base` —— 那会无条件重跑 build-base 阶段。


# --- tool: qpdf ---
# install: apt
# path: /opt
RUN apt-get update \
 && apt-get install -y --no-install-recommends qpdf \
 && rm -rf /var/lib/apt/lists/* \
 && qpdf --version && bash -lc 'command -v qpdf >/dev/null || { echo "MISSING(login): qpdf" >&2; exit 1; }' \
 && bash -c  'command -v qpdf >/dev/null || { echo "MISSING(non-login): qpdf" >&2; exit 1; }'
# --- /tool ---

# <<< scenario: user-tools <<<
