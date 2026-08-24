# >>> scenario: pi-web >>>
# L4:pi 的本地浏览器 UI(agegr/pi-web,Next.js 服务,https://github.com/agegr/pi-web)。
# 拍板"npm 安装上即可"+ 侧边栏按钮:不建 L5 compose 服务(那需要 compose 服务 +
# Caddyfile + services.toml type=web 三件套),仅 npm 全局装得 /usr/local/bin/pi-web
# (系统路径)。面板按钮在 app/services.toml(cmd="pi-web --no-open --hostname 0.0.0.0
# --port 30141")于 pty 面板启动服务;宿主机浏览器访问需先发布端口:
# `sbx ports <sandbox> --publish 30141:30141/tcp`。engines 要求 node>=22.19
# (enabled.toml 已置 22.23.2)。数据读 ~/.pi/agent,与 pi 共享配置/会话。
ARG PI_WEB_VERSION=0.8.9
RUN npm install -g "@agegr/pi-web@${PI_WEB_VERSION}" \
 && command -v pi-web \
 && ls -l "$(command -v pi-web)"

# PI_WEB_ALLOWED_HOSTS=app:让 pi-web 接受 sandbox-net 服务名 `app`
# (VNC/Chromium 容器用 http://app:30141 访问)。pi-web 默认只信任 loopback +
# IP 字面量(request-security.ts);在 profile.d 导出该变量,按钮(bash -lc)与
# 终端里手动启动的 pi-web 都会带上。注意不要写进 services.toml 的 cmd:
# command_exists 探测首个 token,`VAR=x pi-web ...` 会去找名为 `VAR=x` 的二进制。
RUN printf 'export PI_WEB_ALLOWED_HOSTS=app\n' > /etc/profile.d/pi-web.sh \
 && chmod 644 /etc/profile.d/pi-web.sh
# <<< scenario: pi-web <<<
