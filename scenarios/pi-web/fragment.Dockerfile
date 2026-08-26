# >>> scenario: pi-web >>>
# L4:pi 的本地浏览器 UI(agegr/pi-web,Next.js 服务,https://github.com/agegr/pi-web)。
# 拍板"npm 安装上即可",不建 L5 compose 服务:仅 npm 全局装得 /usr/local/bin/pi-web
# (系统路径)。服务由 app/entrypoint.sh 在 app 容器启动时自动拉起(0.0.0.0:30141,
# 崩溃自动重启,日志 ~/.aio/pi-web.log);面板按钮在 app/services.toml 为 type=web
# iframe 内嵌 —— pi-web 是 Next.js 应用,HTML 引用根绝对资源(/_next/...)且自带
# /api/* 路由,无法走 caddy 子路径代理(code-server/vnc 模式),故 compose 给 app
# 直接发布 30141,iframe 指 http://{host}:30141/({host} 由 IframePane 替换为浏览器
# 实际访问的主机名)。宿主机浏览器访问同样经该端口(compose 已发布;sbx 层需一次性
# `sbx ports <sandbox> --publish 30141:30141/tcp`)。engines 要求 node>=22.19
# (enabled.toml 已置 22.23.2)。数据读 ~/.pi/agent,与 pi 共享配置/会话。
ARG PI_WEB_VERSION=0.8.9
RUN npm install -g "@agegr/pi-web@${PI_WEB_VERSION}" \
 && command -v pi-web \
 && ls -l "$(command -v pi-web)"

# PI_WEB_ALLOWED_HOSTS=app:让 pi-web 接受 sandbox-net 服务名 `app`
# (VNC/Chromium 容器用 http://app:30141 访问)。pi-web 默认只信任 loopback +
# IP 字面量(request-security.ts);宿主浏览器经发布端口访问时 Host 是
# localhost/IP 字面量,默认即信任。在 profile.d 导出该变量,终端里手动启动的
# pi-web 也会带上;app/entrypoint.sh 的自启实例不跑 login shell(profile.d 不被
# source),由 entrypoint 显式 export 同名变量。
RUN printf 'export PI_WEB_ALLOWED_HOSTS=app\n' > /etc/profile.d/pi-web.sh \
 && chmod 644 /etc/profile.d/pi-web.sh
# <<< scenario: pi-web <<<
