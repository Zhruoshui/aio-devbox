# >>> scenario: pi >>>
# L4:pi coding agent(earendil-works/pi,https://github.com/earendil-works/pi)。
# 官方安装 = `npm install -g --ignore-scripts @earendil-works/pi-coding-agent`
# (quickstart:pi 不依赖 install scripts)。npm 全局 prefix=/usr/local(node 场景
# 烘在 /usr/local),故得 /usr/local/bin/pi(系统路径,不被共享卷 aio_workspace
# 遮盖;login/non-login shell 都在 PATH)。node 是 always_on L1,build 时已在
# PATH,可直接 npm -g。engines 要求 node>=22.19(enabled.toml 已置 22.23.2)。
#
# 扩展子集配置(pi-packages):pi 从 ~/.pi/agent/settings.json 的 packages 数组
# 加载扩展,而 ~/.pi=/root/.pi 被共享卷盖住 -> 登记必须发生在运行时。为了
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

# agent-browser CLI 烘焙(pi-agent-browser-native 的原生工具补全,prd R1):
# 烘到系统路径 /usr/local(不被共享卷 aio_workspace 遮盖;login/non-login shell
# 都在 PATH),随 make save/load 离线分发,运行时零网络。浏览器后端由 vnc 场景
# 的 chromium CDP(localhost:9222,共享 netns)提供——见 vnc/entrypoint.sh 与
# 下面的 agent-browser-wrapper.sh。vnc 未启用时浏览器类子命令报可操作错误
# (见 wrapper),version/doctor/config 等白名单子命令仍可用(R4)。
#
# agent-browser 包是分发壳(node 启动器 + 全平台原生 Rust 二进制 ~88MB);
# engines node>=24 在 node 22.23.2 上仅出 EBADENGINE 警告(默认非 strict),
# 启动器在 22 可跑——故不升 node,不带 --engine-strict。不用 --ignore-scripts
# (与 pi 本体不同):postinstall 把全局 shim 优化为直指原生二进制(零 node 开销),
# 且 postinstall 不下载(全平台二进制已在 tarball 内),构建无网络依赖。
ARG AGENT_BROWSER_VERSION=0.34.0
RUN npm install -g "agent-browser@${AGENT_BROWSER_VERSION}" \
 && agent-browser --version \
 # 裁掉非本平台原生二进制(全平台 ~88MB → 仅留 linux-x64 ~14MB + 启动器 .js)
 && AB_DIR="$(npm root -g)/agent-browser" \
 && find "$AB_DIR/bin" -type f ! -name 'agent-browser-linux-x64' ! -name '*.js' -delete \
 # npm 全局 shim 让位给 wrapper(上游同款命名);postinstall 已把该 shim 优化为
 # 直指原生二进制,改名后 agent-browser-real 即原生 Rust 二进制(零 node 开销)。
 # wrapper 兜底也覆盖 postinstall 未优化的情况(此时 shim 指向 .js 启动器)。
 && mv /usr/local/bin/agent-browser /usr/local/bin/agent-browser-real \
 # pi-agent-browser-native 的 doctor/config 进 PATH(源用 /opt/pi-extensions 下
 # npm install 生成的 .bin 软链;若不存在则直链 scripts/*.mjs 兜底)
 && DOC_SRC="/opt/pi-extensions/node_modules/.bin/pi-agent-browser-doctor" \
 && { [ -e "$DOC_SRC" ] || DOC_SRC="/opt/pi-extensions/node_modules/pi-agent-browser-native/scripts/doctor.mjs"; } \
 && ln -sf "$DOC_SRC" /usr/local/bin/pi-agent-browser-doctor \
 && CFG_SRC="/opt/pi-extensions/node_modules/.bin/pi-agent-browser-config" \
 && { [ -e "$CFG_SRC" ] || CFG_SRC="/opt/pi-extensions/node_modules/pi-agent-browser-native/scripts/config.mjs"; } \
 && ln -sf "$CFG_SRC" /usr/local/bin/pi-agent-browser-config
# Script COPY goes LAST (own tiny layer): editing the script must not
# invalidate the big npm install layers above.
COPY scenarios/pi/aio-pi-extensions.sh /usr/local/bin/aio-pi-extensions
RUN chmod 0755 /usr/local/bin/aio-pi-extensions \
 && bash -n /usr/local/bin/aio-pi-extensions
# agent-browser wrapper: COPY 最后(独立小层,编辑它不重建上面的大层)。装到
# /usr/local/bin/agent-browser 覆盖 npm shim 位(此时该名已被 mv 走,wrapper 即
# 该名唯一占用者)。所有 agent-browser 调用的必经路径,见脚本头注释。
COPY scenarios/pi/agent-browser-wrapper.sh /usr/local/bin/agent-browser
RUN chmod 0755 /usr/local/bin/agent-browser \
 && bash -n /usr/local/bin/agent-browser
# <<< scenario: pi <<<
