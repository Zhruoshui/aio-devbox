# 设计:pi agent-browser 补全(A 方案:CDP 挂 VNC chromium)

## 总体数据流

```
pi 会话 (app 容器 aio-app-1)
  │ agent_browser 工具调用 (插件 pi-agent-browser-native)
  ▼
spawn PATH 上的 agent-browser
  = /usr/local/bin/agent-browser        ← 本任务新增的 wrapper(shell)
      │ 已传 --cdp? → 原样直调
      │ 9222 探活失败且非白名单子命令? → 可操作错误(R4)
      ▼
  exec /usr/local/bin/agent-browser-real --cdp 9222 "$@"
  = npm 全局 shim(node 启动器→原生 Rust 二进制,daemon 常驻)
      │ CDP WebSocket
      ▼ (共享 netns:localhost 直通,无跨容器 hop)
  vnc 容器 chromium,--remote-debugging-port=9222 (仅 127.0.0.1)
      │ X11
      ▼
  Xvnc :99 → websockify :6080 → noVNC 画面(人可旁观,AC4)
```

## 变更清单(4 处)

### 1. `vnc/entrypoint.sh` — chromium 开 CDP(1 行 + 注释)

chromium 启动参数增加 `--remote-debugging-port=9222`(Chrome 默认只绑 127.0.0.1;
netns 共享下仅 app/code-server/vnc 三容器可达,gateway 不发布该端口)。

- 注释说明消费方:pi 的 agent-browser 经 wrapper `--cdp 9222` 接入;
  与人工浏览共享 `--user-data-dir=/home/gem/.config/chromium`(AC5 的基础)。
- 端口值用 shell 变量 `CDP_PORT="${BROWSER_CDP_PORT:-9222}"`,与 wrapper 侧
  默认值一致,留环境变量逃生口(两侧都用默认 9222,不额外加 compose env)。

### 2. `scenarios/pi/fragment.Dockerfile` — 烘焙 CLI(ARG + RUN + COPY)

在现有 pi 烘焙层之后追加(沿用该 fragment 的注释风格与分层惯例):

```dockerfile
ARG AGENT_BROWSER_VERSION=0.34.0
RUN npm install -g "agent-browser@${AGENT_BROWSER_VERSION}" \
 && agent-browser --version \
 # 裁掉非本平台二进制(全平台 ~88MB → 仅留 linux-x64 ~14MB)
 && AB_DIR="$(npm root -g)/agent-browser" \
 && find "$AB_DIR/bin" -type f ! -name 'agent-browser-linux-x64' ! -name '*.js' -delete \
 # npm shim 让位给 wrapper(上游同款命名)
 && mv /usr/local/bin/agent-browser /usr/local/bin/agent-browser-real \
 # doctor/config 进 PATH(绝对源,相对目标在 .bin 内部自洽)
 && ln -sf "$AB_DIR/node_modules/../../.bin/pi-agent-browser-doctor" /usr/local/bin/ || true
```

要点:
- **不设 `--engine-strict`**:engines>=24 只应产生 EBADENGINE 警告(prd Constraints)。
- **不用 `--ignore-scripts`**(与 pi 本体不同):postinstall 把全局 shim patch 成
  直调原生二进制(零 node 开销);`agent-browser --version` 作为安装自证。
  若 postinstall 在某环境失效,shim 仍是 node 启动器,链路不断(见 wrapper 兜底)。
- doctor/config 链接:源用 `/opt/pi-extensions/node_modules/.bin/pi-agent-browser-doctor`
  (与 pi 扩展同 bake,绝对路径,不随 npm root -g 变化)。上面伪码仅示意,落地以
  /opt 路径为准。
- 裁剪 `find` 模式需保留 `bin/agent-browser.js`(启动器)。

### 3. `scenarios/pi/agent-browser-wrapper.sh` — 新文件 + COPY(最后小层)

上游 `/tmp/re/gem/agent-browser-wrapper.sh` 的精简改编(上游全文已存 research/):

- 解析真身:`agent-browser-real` 优先;兜底 `$(npm root -g)/agent-browser/bin/agent-browser.js`
  (node 启动器,postinstall 失效时不断链)。
- `$*` 已含 `--cdp` → `exec REAL "$@"`(R7 逃逸口)。
- CDP 探活:`curl -s --max-time 1 localhost:${CDP_PORT}/json/version`;
  不通且子命令属于浏览器类(open/click/snapshot/…,即非白名单
  `version|help|doctor|install|upgrade|config|completions`)→ stderr 打印可操作指引:
  "VNC chromium CDP (localhost:9222) 不可达:确认 vnc 场景已启用且 aio-vnc-1 在运行
  (make config 勾选 vnc / docker start aio-vnc-1);或直调 agent-browser-real 走自管模式",
  exit 1(R4)。
- 通 → `exec REAL --cdp "${CDP_PORT}" "$@"`。

COPY 放 fragment 末尾(该 fragment 既有的"脚本最后、独立小层"惯例)。

### 4. 跨场景耦合注释

- pi fragment 注释块写明:agent-browser 的浏览器后端由 vnc 场景提供(CDP 9222),
  vnc 未启用时工具不可用(报错指引见 wrapper)。
- vnc entrypoint 注释写明反向引用。两侧互指,防止未来单侧改动悄悄断链。

## 决策与理由

| 决策 | 选择 | 理由(含被否决项) |
|---|---|---|
| 版本对齐 | 插件 0.3.0→0.5.0,agent-browser 钉 0.34.0 | 插件带版本基线(0.3.0 期望 0.33.2、0.5.0 期望 0.34.0);doctor 会报漂移,须与基线对齐(用户选 Option B,见 prd Notes) |
| node 版本 | 保持 22.23.2 | 启动器纯 spawn/fs/path API,node 22 可跑;升 24 牵动 code-server/app web-builder 全链路,收益为零 |
| CLI 注入方式 | PATH wrapper(非插件级 connect) | 对插件与终端用户双透明;上游生产镜像验证过的模式;插件级 `{"args":["connect","9222"]}` 要求 agent 每会话记得 connect,易漏 |
| 自管浏览器 | 不烘焙 | +~400MB 离线分发体积;VNC 不可见;与"共享登录态"目标冲突(B/C 方案否决) |
| shim 改名 | `agent-browser-real`(上游同款) | 语义直白,与上游运维经验对齐 |
| daemon 状态 | 不管 | 上游 self-managed(.pid+.sock 于 $HOME=共享卷);容器重建后 daemon 惰性重启,按上游行为,AC4 复测覆盖 |

## 风险与对策

- **engines>=24 警告**:接受;若未来 npm 默认 strict,显式加 `--engine-strict=false`。
- **`close`/`quit` 语义**:插件文档明确 CDP 附加时 close 仅断开会话、不动宿主浏览器;
  E2E 时实测确认(AC4 附带)。若 quit 杀掉 chromium → vnc 容器 restart 策略自愈,
  不另加防护(MVP 简洁优先,实测有问题再补)。
- **9222 探活增加每次调用 ~毫秒级延迟**:仅 curl 本地回环,可忽略;
  daemon 已连状态下 agent-browser 自身会话复用不受影响。
- **镜像体积**:烘焙净增 ~15-20MB(裁剪后);`make save` tar 相应增大,可接受。
- **chromium 151 HTTPS-first**:结构性已解(netns localhost 直通,见 memory);
  agent-browser open 外部 https 站点不受影响。

## 构建/发布/回滚

- 发布:`make build`(gen→Dockerfile.base 重组含新 pi fragment→build-base→compose build)
  + `docker compose up -d --force-recreate app vnc`。
  注意 memory 坑:`compose up --build` 不重建运行中服务,必须显式 `--force-recreate`。
- 回滚:revert 提交后同命令重建;两层变更(vnc entrypoint / pi fragment)可独立回滚。
- 离线分发:`make save` 自然携带新层;`make load` 侧无新依赖,`aio-pi-extensions`
  登记流程不变(AC8)。
