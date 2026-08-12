# >>> scenario: opencode >>>
# L4 示例:opencode AI agent CLI。原烘在 Dockerfile.base.head,按四层模型
# (AI agent 归 L4)迁出到此。GitHub release 单二进制,装 /usr/local/bin(系统
# 路径,不被共享卷 aio_workspace 遮盖)。opencode.ai 官方 installer 被沙箱网络
# 策略封锁,故走 GitHub release(同 head 的 node 模式)。
#
# Web 面板耦合(已知 MVP 限制,见 prd 发现 6/7):app/services.toml 把 opencode
# 注册为 type=agent 服务(enable=ENABLE_OPENCODE, cmd=opencode),面板可见性由
# compose 的 ENABLE_OPENCODE env(硬编码 true)控制,与"是否烘进镜像"解耦。
# 故若取消勾选本场景:opencode 不烘,但面板仍显示 -> 点开 `opencode: command
# not found` 后关闭(死面板,非崩溃)。待未来"自动检测 L4 AI -> Web 动态按钮"
# 特性修(运行时 command_exists 探测,面板跟随勾选状态)。

ARG OPENCODE_VERSION=v1.18.7
RUN mkdir -p /tmp/oc \
 && curl -fsSL "https://github.com/sst/opencode/releases/download/${OPENCODE_VERSION}/opencode-linux-x64.tar.gz" \
        -o /tmp/oc.tar.gz \
 && tar -xzf /tmp/oc.tar.gz -C /tmp/oc \
 && install -m 0755 /tmp/oc/opencode /usr/local/bin/opencode \
 && rm -rf /tmp/oc /tmp/oc.tar.gz \
 && opencode --version
# <<< scenario: opencode <<<
