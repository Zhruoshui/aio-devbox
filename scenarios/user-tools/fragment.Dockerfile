# >>> scenario: user-tools >>>
# L2 shell 层:user-tools —— 用户自定义工具层(非 mise 工具的逃生通道)
#
# 这个 fragment 是给**最终用户**自由编辑的,不是给场景作者照搬的模板。
# 装非 mise 能管理的工具(apt 系统包、源码编译、自定义 tarball)。
#
# 重建范围实测(2026-09-22):BuildKit 缓存链是**位置链**——改本层会让
# 本层及**之后所有层**重建(实测本层位于 L2 中间时,后续 ~27 层重建,约 50s)。
# 前面的层(head + 排在本场景之前的场景)缓存不受影响。保持本层精简、
# 少改动,能把重建代价降到最低。
#
# ─────────────────────────────────────────────────────────────────────────────
# 三条硬规则(违反任意一条,工具在容器里就"消失了"):
#
#   1. 安装到系统路径:
#        - 单二进制  → /usr/local/bin
#        - 目录树    → /opt/<tool>(再按需软链到 /usr/local/bin)
#      **绝不写 /root** —— 共享工作区卷挂 /root,镜像层内容会被遮盖。
#
#   2. 双通道自检(必须同时通过):
#        bash -lc  '<tool> --version'   # WebUI 终端面板是 login shell
#        bash  -c  '<tool> --version'   # code-server 默认非 login shell
#      只过一条 = 只在某个终端可见,另一条看不见。两条都要。
#
#   3. 每次修改后:
#        make gen                      # 重新拼接 Dockerfile.base
#        docker compose build app      # 重建本层及之后所有层(BuildKit 位置链)
#        docker compose up -d --force-recreate app
#      **不要** `make build-base` —— 那会无条件重跑 build-base 阶段。
#
# 进阶(可选):
#   - 用 `ARG <TOOL>_VERSION=x.y.z` 固定版本,可复现构建,升级改一行
#   - 多工具可写多个独立 RUN,每个工具一个层,单独重建粒度更细
#   - apt 装完务必 `rm -rf /var/lib/apt/lists/*`(base head 已清空,见 c23 场景)
#
# ─────────────────────────────────────────────────────────────────────────────
# 完整示例(取消注释即可用):
#
# ARG HTOP_VERSION=3.2.2
# RUN apt-get update \
#  && apt-get install -y --no-install-recommends htop \
#  && rm -rf /var/lib/apt/lists/* \
#  && htop --version \
#  && bash -lc 'command -v htop >/dev/null || { echo "MISSING(login): htop" >&2; exit 1; }' \
#  && bash -c  'command -v htop >/dev/null || { echo "MISSING(non-login): htop" >&2; exit 1; }'

# 当前为空 —— 用户自行添加上方示例或自定义安装步骤
RUN echo "user-tools: no custom tools yet"
# <<< scenario: user-tools <<<
