# pi agent-browser 原生工具补全:烘焙 CLI + 浏览器后端

## Goal

让 pi 的 `agent_browser` 原生工具真正可用:插件(pi-agent-browser-native)已烘焙并登记,
但它包装的 `agent-browser` CLI 不在容器里,导致所有调用以 `missing-binary` 失败。
本任务补上 CLI,并让它驱动 VNC 桌面里的 chromium(A 方案:CDP 挂现有 VNC 浏览器)。

## 背景(诊断结论,2026-08-26)

- `pi-agent-browser-native@0.3.0` 是薄桥接,设计上**不携带** agent-browser 本体
  (其 REQUIREMENTS.md 明确 "agent-browser is an external dependency")。
- `agent-browser`(vercel-labs)npm 包只是分发壳:node 启动器 + 各平台原生 Rust 二进制;
  daemon 纯 Rust 不依赖 node。engines `node>=24` 仅产生 EBADENGINE 警告(默认非严格),
  启动器在 node 22.23.2 上可运行 → **不需要升级 node**。
- AIO 的 vnc chromium 目前未开 `--remote-debugging-port`,AI 无法驱动;
  netns 共享已落地,app 容器的 `localhost:9222` 天然直通 vnc 容器,只差启动参数。
- 上游 agent-infra sandbox 的成熟做法:chromium 开 CDP 9222 + PATH wrapper 统一注入
  `--cdp 9222`(参考文件已在任务 research 中提取)。

## Requirements

- R1 **CLI 烘焙**:`agent-browser` 烘进 app 基座镜像,系统路径(/usr/local),
  不被 aio_workspace 卷遮盖;随 `make save/load` 离线分发,运行时零网络。
- R2 **端到端可用**:pi 会话内 `agent_browser` 工具调用(open/snapshot/click/
  screenshot/read 等)成功。
- R3 **A 方案浏览器后端**:agent-browser 经共享 netns 的 localhost CDP(9222)驱动
  vnc chromium;浏览过程在 noVNC 画面可见;与人工浏览共享同一 user-data-dir/cookie
  (登录一次,两边都认)。
- R4 **vnc 未启用的清晰降级**:vnc 场景停用/未起时,工具失败且报可操作的错误
  (提示开 vnc 或走 agent-browser-real),不是裸的连接栈错。
- R5 **doctor 可用**:`pi-agent-browser-doctor` 进 PATH,可一键自检。
- R6 **零回归**:现有 5 个 pi 扩展登记不受影响;pi 其余功能、vnc 人工浏览不受影响。
- R7 **逃逸口**:用户显式传 `--cdp` 时不重复注入;`agent-browser-real` 始终可直调
  (绕过 wrapper 的自管/其他 CDP 用法)。

## Constraints

- node 保持 22.23.2(engines 警告已知且无害;升 24 是被否决的备选)。
- 遵循仓库烘焙惯例:系统路径、COPY 脚本放最后的小层、`make build` 全量重建。
- CDP 仅绑 127.0.0.1(Chrome 默认),只在共享 netns 内可达;gateway 不得暴露 9222。
- 镜像体积增量受控:烘焙后裁掉非本平台二进制(全平台包 ~88MB → 目标 +~20MB 级)。

## Acceptance Criteria

- [ ] AC1 `docker exec aio-app-1 agent-browser --version` 输出版本号(wrapper 链路通)。
- [ ] AC2 `pi-agent-browser-doctor` 全绿(无 missing-binary)。
- [ ] AC3 vnc 运行时,app 容器内 `curl -s localhost:9222/json/version` 返回 chromium 信息。
- [ ] AC4 pi 内 `agent_browser` 调 `{"args":["open","https://example.com","--load-state","networkidle"]}` 成功,且 noVNC 画面里能看到该页面(人工核对或截图)。
- [ ] AC5 cookie 共享:vnc 里人工访问某站后,agent 侧 `get url`/`read` 能看到同一会话状态(或反向)。
- [ ] AC6 离线分发:`make save` 的镜像含 agent-browser;`make load` 后 AC1/AC3/AC4 复测通过(本机可模拟 load 回放)。
- [ ] AC7 停掉 vnc(`docker stop aio-vnc-1`)后工具调用失败,错误信息包含可操作指引。
- [ ] AC8 `pi list` 仍为 5 个包、无重复加载;`aio-pi-extensions` 重跑幂等。

## Notes

- 浏览器后端已决策:A 方案(CDP 挂 VNC chromium);B(自管 headless +400MB)与
  C(双后端)被否决——用户本就启用 vnc,可见+共享登录态价值最高、零体积代价。
- **版本决策修订(2026-08-26)**:实施时发现插件内置版本基线——
  `pi-agent-browser-native@0.3.0` 期望 `agent-browser@0.33.2`,而当时拟钉的 0.35.0
  会让 `pi-agent-browser-doctor` 报版本漂移(AC2 不绿)。用户选 Option B:
  插件 `0.3.0`→`0.5.0`,`agent-browser` 钉 `0.34.0`(0.5.0 的基线)。
  原注"0.35.0 符合插件只跟最新上游策略"不成立(0.3.0 插件落后上游两个 minor)。
