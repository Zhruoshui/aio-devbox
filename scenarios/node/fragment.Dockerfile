# >>> scenario: node >>>
# L1 always_on:Node 运行时。nodejs.org 官方 tarball 装 /usr/local(系统路径,
# 躲过共享卷 aio_workspace 对 /root 的遮盖;在 PATH 上,code-server 与 app
# web-builder 直接可用,无需 source)。版本由 TUI 下拉选,gen 把 {{version}}
# 注入 ARG。NodeSource 被沙箱网络策略封锁,故走 nodejs.org(同 head 原 node 安装)。
ARG NODE_VERSION={{version}}
RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
        -o /tmp/node.tar.xz \
 && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
 && rm /tmp/node.tar.xz \
 && node --version && npm --version
# <<< scenario: node <<<
