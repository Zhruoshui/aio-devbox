# >>> scenario: pi >>>
# L4:pi coding agent(earendil-works/pi,https://github.com/earendil-works/pi)。
# 官方安装 = `npm install -g --ignore-scripts @earendil-works/pi-coding-agent`
# (quickstart:pi 不依赖 install scripts)。npm 全局 prefix=/usr/local(node 场景
# 烘在 /usr/local),故得 /usr/local/bin/pi(系统路径,不被共享卷 aio_workspace
# 遮盖;login/non-login shell 都在 PATH)。node 是 always_on L1,build 时已在
# PATH,可直接 npm -g。engines 要求 node>=22.19(enabled.toml 已置 22.23.2)。
#
# 扩展子集配置(pi-packages):pi 从 ~/.pi/agent/settings.json 的 packages 数组
# 加载扩展,而 ~/.pi=/home/gem/.pi 被共享卷盖住 -> 登记必须发生在运行时。为了
# 让 docker save/load 的离线分发携带扩展本体,包实体烘在系统路径
# /opt/pi-extensions(scenarios/pi/pi-packages/package.json 的 dependencies 即
# 清单,单一事实源),运行时由 aio-pi-extensions 用本地路径(pi install /abs/path
# 不拷贝、零网络)登记进卷上的 settings.json —— 离线机器 `make load` 后跑一次
# 脚本即可,不需要 npm 网络。同 code-server /opt/cs-extensions 的烘焙模式。

ARG PI_VERSION=0.84.2
COPY scenarios/pi/pi-packages/package.json /opt/pi-extensions/package.json
RUN npm install -g --ignore-scripts "@earendil-works/pi-coding-agent@${PI_VERSION}" \
 && pi --version \
 && cd /opt/pi-extensions \
 && npm install --omit=dev --no-audit --no-fund \
 && ls /opt/pi-extensions/node_modules
# Script COPY goes LAST (own tiny layer): editing the script must not
# invalidate the big npm install layers above.
COPY scenarios/pi/aio-pi-extensions.sh /usr/local/bin/aio-pi-extensions
RUN chmod 0755 /usr/local/bin/aio-pi-extensions \
 && bash -n /usr/local/bin/aio-pi-extensions
# <<< scenario: pi <<<
