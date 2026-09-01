# Journal - ruoshui (Part 1)

> AI development session journal
> Started: 2026-07-28

---

## 2026-07-28 - Phase G (VNC/Chromium) verification

Task: `07-28-server-mvp` (in_progress). Resumed at Phase G per user; model has no
multimodal, so visual checks delegated to user.

Phase G infra was already built (vnc/Dockerfile, vnc/entrypoint.sh, compose
`vnc` service, Caddyfile `/vnc/*`, services.toml `vnc` entry) and running.
Verified non-visually:

- 4 procs (Xvnc, openbox, chromium, websockify) run as `gem` (uid 1000).
- `GET /api/manifest` -> `vnc` `enabled:true`; `codeServer` `enabled:false`
  (only `--profile vnc` is up). Toggle proven: stop vnc -> `enabled:false`,
  start -> `enabled:true` (AC4 mechanism).
- `/vnc/vnc.html` -> HTTP 200 (title "noVNC") through basicauth + subpath strip.
- noVNC WS upgrade through gateway: with auth = HTTP 101, without = 401
  (resolves implement.md risky-note "noVNC WS through caddy basicauth").
- AC3: Chromium profile on shared `workspace` volume survives full stop/start;
  Chromium relaunches; `Default/Preferences` grows on relaunch (profile reused,
  not recreated).
- X framebuffer screenshot (`vnc-screenshot.png`, 1280x800) ~80% non-black,
  high stddev -> content rendering (consistent with Chromium `about:blank`).

Marked Phase G build checkboxes [x] in implement.md; left the visual validation
(AC2 "VNC drives Chromium") pending user confirmation.

Next: user visually confirms noVNC pane shows a drivable Chromium desktop, then
Phase H (pluggability polish + AC1-AC5 acceptance, also partly visual).

### User visual test - found + fixed 2 real bugs (noVNC wouldn't connect)

User opened the noVNC pane in a private browser (no cache). Two bugs surfaced:

1. **(cache, not a bug)** Earlier `addConnectionControlHandlers` null error was a
   stale cached `ui.js` - line numbers in the browser trace didn't match the
   served file. Private browser cleared it.

2. **noVNC 404 on connect (real bug, fixed).** noVNC `ui.js` builds the WS URL as
   `ws://<host>:<port>/<path>` - absolute, it does NOT carry the `/vnc/` subpath
   the page was served from. Default `path=websockify` -> `ws://host/websockify`
   -> caddy catch-all -> axum -> 404. Fix: added `&path=vnc/websockify` to the
   `vnc.url` in `app/services.toml` -> connects to `/vnc/websockify`, caddy
   strips to `/websockify` on vnc:6080 (the endpoint that returns 101).
   `services.toml` is `include_str!`-baked, so rebuilt the app image.

3. **Chromium SingletonLock crash loop on recreate (real bug, fixed).** Rebuilding
   the app recreated the vnc container with a NEW hostname. The old container's
   Chromium left stale `Singleton{Lock,Cookie,Socket}` symlinks on the shared
   volume; new-container Chromium saw a foreign-hostname lock, refused to launch
   ("profile appears to be in use by another computer"), exited, and the bash
   supervisor tore the container down -> crash loop. A plain `docker restart`
   (same hostname) does NOT trigger this, so my earlier AC3 stop/start test
   missed it. Fix: `vnc/entrypoint.sh` now removes stale `Singleton*` before
   launching chromium (safe - one chromium per container). Rebuilt vnc image.

After both fixes: vnc starts clean (4 procs, no crash loop), manifest
`vnc.enabled=true` with the corrected url, WS `/vnc/websockify` -> 101. Recorded
both findings in implement.md "Risky points". Awaiting user retest of the live
noVNC connection (AC2).

### User retest - AC2 connect works; added CJK fonts

User connected via noVNC and drove Chromium to www.baidu.com - AC2 "VNC drives
Chromium" connect+navigate works. But Chinese chars showed as tofu boxes:
sandbox-base/vnc had no CJK font, and Chromium renders server-side (on the
in-container X display), so the font must be in the vnc container (not base -
app/code-server don't server-render text). Added `fonts-noto-cjk` to
`vnc/Dockerfile` apt install (+ `fc-list | grep -qi "noto.*cjk"` build check),
rebuilt vnc image. Verified: 30 `:lang=zh` font entries present, vnc healthy,
WS 101. Awaiting user confirmation that Chinese now renders on baidu.com.

## 2026-07-28 - Phase H non-visual verification (all autonomous acceptance done)

Resumed `07-28-server-mvp` via `/trellis:continue`. Stack was live (gateway, app,
code-server, vnc up). implement.md Phase G fully checked incl. CJK font fix;
Phase H (pluggability polish + AC1-AC5) remained. Did every non-visual Phase H
item; only purely-visual acceptance left for the user.

Set active task pointer (`task.py start 07-28-server-mvp` - was empty). Read
config.rs / main.rs / App.tsx / Caddyfile to confirm the manifest + seam +
frontend-filter mechanisms before testing.

Results (all through gateway `admin:admin` @ :8080):
- **H1 / AC4 mechanism**: `docker compose --profile vnc stop vnc` ->
  `vnc.enabled=false` (other 3 stay true); `start vnc` -> `true`. UI filters on
  `enabled` (App.tsx:56), so absent profile -> no pane.
- **H2 / AC5**: `/api`,`/api/`,`/api/*`,`POST /api`,`/v1`,`/v1/*`,`/mcp`,`/mcp/*`
  all -> 502 `{"error":"seam reserved"}`. `/api/manifest` -> 200 (not swallowed);
  `/api/term/ws` -> 400 "Connection header did not include 'upgrade'" (WS handler,
  not the seam).
- **H3 / AC1+AC3**: `GET /` -> SPA index.html + `/assets/index-*.js|.css` (200),
  401 without auth. AC3: marker written as gem in app:/home/gem readable from vnc
  AND code-server (shared volume), survives `docker compose restart app`.
- **H4 / §14A smoke test**: added throwaway `smokeTest` (type=agent) to
  services.toml, rebuilt app, manifest -> 5 services (smokeTest enabled). Frontend
  is generic (PaneForService dispatches on `type`), so pane appears with no React
  change. Reverted services.toml + rebuilt -> back to 4 services.
- **H5 / build**: `npm run build` (web) = tsc clean + vite build OK (74 modules;
  asset hashes match running image). App has no unit tests; compile proof = the
  2 smoke-test rebuilds.
- **AC2 non-visual strengtheners**: code-server serves IDE at
  `/code-server/?folder=/home/gem` (302->200); opencode v1.18.7 installed in app;
  terminal pty WS bidirectional (node `ws` client sent `echo <marker>`, saw it back;
  shell prompt `gem@<host>:~$`).

**Offline-build workaround (env, not code)**: `registry-1.docker.io` is blocked,
so BuildKit can't pull the `# syntax=docker/dockerfile:1` frontend (app/Dockerfile
line 1) -> `docker compose build app` fails at frontend resolution. The Dockerfile
uses only standard multi-stage `COPY --from`, so I temporarily replaced line 1
with a plain comment (BuildKit falls back to its bundled frontend; all base images
cached = fully offline) for the smoke-test rebuilds, then restored it. Recorded in
implement.md "Risky points". Running binary unaffected.

Updated implement.md Phase H checkboxes (non-visual items [x]; visual items marked
PENDING USER). Phase A-F checkboxes were already `[ ]` from prior sessions even
though that work is done and running - left as-is (not re-verified this session).

**Next (user)**: visual acceptance - AC1 (browser loads 4 arrangeable panes), AC2
(code-server edits persist, VNC drives Chromium + CJK renders, opencode launches in
pane), and "UI hides absent VNC pane" (stop vnc, reload). Once confirmed, Phase 3:
spec update (3.3) + commit (3.4).

## 2026-07-29 - VNC UX polish (3 detail issues from user visual test)

User drove Chromium via noVNC (AC2 connect+navigate works, CJK renders). Three
detail issues surfaced; all in MVP scope, fixed now (model has no multimodal, so
visual confirmation delegated to user).

1. **Chromium window draggable + closable; close loses the page.** openbox
   `decor=no` + `maximized=true` (new `vnc/openbox-rc.xml`, COPY'd to
   /etc/aio/openbox-rc.xml, entrypoint copies to ~/.config/openbox/rc.xml at
   start since the volume shadows /home/gem) removes the WM title bar (verified:
   _OB_WM_STATE_UNDECORATED, _NET_FRAME_EXTENTS=0). Chromium now runs in a
   `setsid` auto-restart loop (mirrors AIO browser-supervisor.py) so a chromium
   exit relaunches in-place WITHOUT tearing the container down (only
   Xvnc/openbox/websockify are `wait -n`-critical). Verified: `pkill -x
   chromium` -> container stays up, chromium relaunches, log shows "chromium
   exited, relaunching".
2. **opencode pane not centered, black bar on right.** Root cause: XtermPane
   forwarded keystrokes (Text frames) but NEVER terminal resize, so the pty
   stayed at 80x24 while xterm fit the pane -> TUI rendered at wrong width.
   Fix: added a resize protocol - XtermPane sends a 5-byte Binary frame
   [0x01, cols_le, rows_le] on `term.onResize` + on ws open; terminal.rs parses
   Binary frames and calls `master.resize(PtySize)` (pty.rs exposes `master`).
   Verified end-to-end: WS client sent resize 100x30, `stty size` in the pty
   printed `30 100`.
3. **Chromium's own CSD buttons (min/max/close) still usable; minimize loses the
   window.** These are NOT WM decorations (openbox already removed those) but
   Chromium's client-side decorations, drawn in bare-X11. Confirmed via web
   search + the jlesage/docker-baseimage-gui unresolved issue: `decor=no` can't
   remove them and there is NO working flag to hide them in Chromium 150
   (upstream architecturally entangled; `--disable-features=
   ClientSideDecorations` does nothing). User chose "taskbar + keep address
   bar" over kiosk (kiosk hides buttons but loses the address bar). So: added
   `tint2` taskbar (5th supervised process, restart loop) so a minimized window
   is recoverable from the panel; + a managed policy
   `/etc/chromium/policies/managed/aio-restore.json` = `{"RestoreOnStartup":1}`
   so closing chromium relaunches with the previous page restored (close no
   longer loses the page). Verified: 5 procs run, tint2 is a 30px bottom dock
   (_NET_WM_STRUT=30), maximized chromium is 1280x770 (leaves the 30px panel
   visible, not covered). Buttons remain visible (upstream limit) but
   minimize/close are now non-destructive.

All in `vnc/Dockerfile` + `vnc/entrypoint.sh` (+ pty.rs/terminal.rs/XtermPane.tsx
for #2). Awaiting user visual confirmation of the taskbar + minimize/restore +
close-restores-page.

### Reverted the chromium-decoration work to the first version (2026-07-29)

User chose to defer the window-decoration polish and revert to the first working
version. Reverted (in `vnc/Dockerfile` + `vnc/entrypoint.sh`, deleted
`vnc/openbox-rc.xml`): the openbox `decor=no`/`maximized` config, the chromium
`setsid` auto-restart loop, `--disable-features=ClientSideDecorations`, the
`RestoreOnStartup=1` policy, and `tint2`. Chromium is back to a plain background
process under `wait -n` (closing it restarts the container, as in the first
version). KEPT (essential, not decoration polish): the noVNC `path=vnc/websockify`
fix, the SingletonLock cleanup, `fonts-noto-cjk`, and the pty resize fix. Full
investigation + re-apply instructions recorded in
`vnc/DEFERRED-chromium-decorations.md` for when the work resumes.

### Acceptance + commit (2026-07-29)

User confirmed AC1 + AC2 (visual); AC3/AC4/AC5 were already non-visual-verified,
so all AC1-AC5 pass. Wrote a reasonable `.gitignore` (added `vnc-screenshot.png`,
editor/OS files, `gateway/secrets/` dir; kept `.trellis` ignored; unstaged the
leftover `.trellis` research archive to respect gitignore). Created branch
`feat/aio-sandbox-mvp` and committed: `a83508b` (root commit, 40 files, 6183
insertions) - clean project source only (no build artifacts/secrets/.trellis).
Set local git identity `ruoshui <ruoshui@users.noreply.github.com>` (was unset;
user can `git commit --amend --reset-author` to change). No remote configured.

3.3 spec update DEFERRED to the `00-bootstrap-guidelines` task per user (fills
`.trellis/spec/` with the project's real conventions). server-mvp left
`in_progress` - archive after the bootstrap task lands the spec. Flipped all
implement.md checkboxes to [x] (work done + committed) and added a finish-status
note there.

### C23 开发环境场景完成(2026-08-19)

新增 `scenarios/c23/`(L3 lang)到 AIO sandbox 配置,提供符合 C23 标准的 C 开发环境。
- 决策(用户):保留 gcc-12;clang 选最新 clang-22(apt.llvm.org);C 配套工具合并进同一场景。
- 实现:clang-22 工具链(clang/clang++/clang-format/clang-tidy/clangd/lld + libclang-rt-22-dev,
  来自官方 apt.llvm.org bookworm-22 仓库)+ C 配套(gdb/cmake/ninja/ccache/valgrind/cppcheck/strace)
  + 构建期 C23 冒烟(clang -std=c23 编译 bool/typeof/0b/数字分隔符)。软链到 /usr/local/bin 保证 login shell 可见。
- 关键调研:bookworm apt 只有 gcc-12(部分 C23),backports 无新 gcc;clang-19 已在 bookworm main;
  最新 clang 走 apt.llvm.org。详见任务 research/c23-toolchain-availability.md 与记忆 bookworm-toolchain-ceiling。
- 复核:trellis-check 全绿(8/8 规则),并在临时容器实跑安装 + C23 冒烟通过(ok=1 b=10 n=1000),零 bug。
- 未做(用户自跑):`make build-base` + `make up` + 容器内验证。gcc-12 用 `-std=c2x`(部分 C23,需 stdbool.h)。
- 遗留限制:C23 标准库层(<stdbit.h>)需 glibc≥2.39(bookworm 2.36),库层面完整需整体换 trixie 基座(独立后续)。


## Session 1: pi agent-browser 原生工具补全(烘焙 CLI + CDP wrapper 挂 VNC chromium)

**Date**: 2026-08-26
**Task**: pi agent-browser 原生工具补全(烘焙 CLI + CDP wrapper 挂 VNC chromium)
**Branch**: `feat/aio-sandbox-mvp`

### Summary

诊断 missing-binary 根因(插件是薄桥接不携带 CLI);A 方案落地:烘焙 agent-browser@0.34.0+wrapper 注入 --cdp 9222 驱动 vnc chromium;插件 0.3.0→0.5.0 对齐版本基线;实测 AC1-8 全绿(close 只断 CDP、vnc 停报可操作错误、离线镜像携带)。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `8f620b2` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete

## Session 2: 模型配置页重构——cc-switch 卡片 + Kumo 重做 + 用量图表(2026-08-27)

**Task**: 08-26-models-config-redesign
**Branch**: `feat/aio-sandbox-mvp`

### Summary

修复上一提交(e3b655f)四个问题:R1 删 anthropic 协议块(协议选择即端点);
R2 供应商库改 cc-switch 卡片网格+编辑抽屉+per-model 协议列;R3 全 pane 按
cloudflare_kumo_ui.md 重做(ml-* 语义类,token-only);R4 用量页改汇总卡+
水平柱状图+成本环图(Kumo 分类色,零图表依赖)。本会话主要做收尾核查:
后端 cargo test 148 绿、前端构建干净、容器活体验证、i18n 死键清理、提交。

### Key Facts

- Rust 测试宿主机无 cargo,用 throwaway `rust:1-bookworm` 容器挂源码跑
  (`docker run --rm -v $PWD/app:/app -v aio-cargo-registry:/usr/local/cargo/registry`)。
- 重建 app 后 sidecar 报 "joining network namespace of container: No such
  container"——`docker restart` 不行,须 `docker compose up -d --force-recreate
  code-server vnc`(sidecar 持有旧 app 容器 ID 引用)。
- 运行容器此前一直用 dangling 旧镜像(旧前端 bundle);`sandbox-app:latest`
  (becace8101f0)才含重构前端,force-recreate app + 重建 sidecar 后生效。

### Git Commits

| Hash | Message |
|------|---------|
| (本次) | feat: 模型配置页重构——供应商卡片 + Kumo 视觉 + 用量图表 |

### Status

[OK] **Completed**

### Next Steps

- None - task complete

---

## 2026-08-27 | claude/codex 多配置项 preset(08-27-agent-multi-preset)

### What Happened

canonical `agents.{claude,codex}` 从单一 assignment 改为 cc-switch 式
`{presets[], current}`。后端 shadow 反序列化做向后兼容(旧形状→单 default
preset),PUT 时后端补 preset id(splitmix64 短 id)+ 解析前端 `current:""`
占位;渲染器 apply 当前 preset(悬空/缺失→push_err 不写半截);validate 遍历
每个 preset(错误带 name、拒绝重复 id 与悬空 current)。前端 AgentTabs 收窄为
pi/opencode,新增 PresetList(卡片列表:当前徽标/协议徽标/live 对照徽标;行
操作:设为当前=setCurrent+save+apply 一键、编辑行内表单、复制、删除顺移)。
顺移逻辑放前端,后端只 validate 把关。

### Verification

cargo test models 157 绿(迁移/渲染器/validate/id 回填 18 个新测试);
npm run build 干净;容器 E2E 17 项全过(2 preset 空 id→回填+current 解析→
apply→settings.json 随 preset 变→切换→删除顺移→悬空 PUT 400→旧形状 codex
apply→磁盘落新形状);trellis-check 通过(修 1 处 currentId 瞬态矛盾)。

### Key Facts

- 前端新增 preset 用 `id:""`,首个 preset 以 `current:""` 占位——后端
  ensure_preset_ids 回填后把空 current 解析到新 id(前端无法预知后端 id)。
- 本地 shell 无 cargo:throwaway `rust:1-bookworm` 容器挂 aio-cargo-registry
  卷跑测试;改 Dockerfile 后须 build + `up -d --force-recreate`(sidecar
  netns 耦合)。
- 容器实测会污染真实 models.json 与 ~/.claude/settings.json:测毕从
  .aio-bak 备份恢复 settings、删测试生成的 ~/.codex、还原 agents 段。
- 铁律:Edit 工具匹配带 —/§/→ 等非 ASCII 字符的 Rust/TSX 注释时,old_string
  必须逐字符复制(工具不会做近似匹配);整文件重写用 Write 更稳。

### Git Commits

| Hash | Message |
|------|---------|
| (本次) | feat: claude/codex 多配置项 preset(cc-switch 式) |

### Status

[OK] **Completed**

### Next Steps

- 回父任务 08-27-models-config-v2 标记本子任务完成(共 4 子任务)

## 2026-08-27 | 用量成本补算 + cache 拆列 + 表格对齐(08-27-usage-correctness)

### What Was Done

- 后端 `backfill_cost`(usage.rs):日志 cost>0 信任保留;0/None 走 canonical 匹配
  a 精确(provider 已知)→ b 跨 provider 精确 → c 版本式后缀模糊(`-\d[\d.]*`,
  字母变体 `-free/-exp` 拒绝——对账实测发现 deepseek-v4-flash-free 被误按基础
  费率计费后收紧);§b/c 命中且 provider 唯一时顺带回填 provider。
- 成本单位定约 $/M:$/token 解释会让 0.14 变 $140k/M,荒谬;分项计价
  in/out/cacheRead/cacheWrite 各用各单价,不混 input。
- 零值行过滤:handler 层 `rows.retain(in+out+cr+cw>0)`,scan 契约不动。
- 前端:cache 拆 Read/Write 两列(i18n mcUsageColCacheR/W);hasCost/hasCostValue
  语义分离(some≠undefined 显列,some>0 才画环图);粘性表头 + overflow-x 容器
  + 长名 ellipsis+title;ModelTable cost 列头 ($/M) 标注。

### Verification

- 抽样对账:opencode + pi 各 3 行手算 token/cache/cost 与
  `?window=all&refresh=1` 精确一致;-free 误报在对账中发现并修复。
- cargo test models **166 绿**(9 个补算/过滤新测试);npm run build 干净。

### Key Facts

- pi/opencode 日志 cost 字段存在但**恒为 0**(不可信)——信任阈值必须是
  `>0`,不能是"字段存在"。
- 明细表 hasCost 旧 bug:`cost !== undefined` 让全 0 也触发 cost 列 + 空环图。

### Git Commits

| Hash | Message |
|------|---------|
| (本次) | feat: 用量成本补算($/M 约定)+ cache 拆列 + 表格对齐修复 |

### Status

[OK] **Completed**

### Next Steps

- 回父任务 08-27-models-config-v2 标记 R5 完成(剩 2 子任务 planning)。

## 2026-08-27 | 供应商表单 pi-web 流 + models.dev 集成(08-27-provider-form-piweb)

### What Was Done

- 后端新增 `GET /api/models/catalog`(catalog.rs):代理 models.dev/api.json,
  归一化 + 1h 缓存(持锁内 fetch 做 in-flight 去重,不用额外 broadcast 机制);
  15s 超时,502 + 截断错误契约同 discover.rs。
- **顺带修复既有 bug**:`render/pi.rs::render_pi_cost` 把 canonical cost(现定
  $/M)原样写进 pi 原生 models.json,但 pi schema 是 USD/token——差 100 万倍。
  容器实测确认:apply 前 pi 侧 `input=0.14`(应为 0.00000014);修复后除以 1e6
  正确落盘 `1.4e-07`。
- 前端:`ModelTable.tsx`(13 列横表)废弃,改 `ModelRow.tsx`(pi-web 式折叠/
  展开单行,collapsed = id+name+推理徽标+cost摘要+test+删除,expanded = 全字段
  编辑+「从 models.dev 填充」按钮);新增 `ModelPicker.tsx`(无状态纯 props
  组件,备好供 R2/R3 接线,本任务未消费其独立 UI)。
- `types.ts` 新增 catalog 类型 + `decodeCatalog` + `catalogRecommend`(静态
  host→models.dev-provider-id 映射表 + 精确 model id 匹配)。

### Verification

- cargo test models 173 绿(+7:catalog 归一化/缓存/truncate 6 个 + pi cost
  单位修复 1 个);npm run build 干净。
- **容器手测(重建镜像 + force-recreate app 后,vnc/code-server 的共享 netns
  失效,补做 force-recreate vnc code-server 重新加入)**:用最小 stdlib-only
  CDP WebSocket 客户端(纯 Python,无 websockets 依赖,vnc 容器内自带 chromium
  的 :9222)连已运行 tab,截图逐层验证——供应商网格不变、抽屉打开、ModelRow
  折叠态(cost 摘要正确)、展开态(全字段)、models.dev 填充命中(deepseek
  provider host 匹配成功,6 项一次性回填)与未命中(host 不在映射表,显示
  "未在 models.dev 目录中找到匹配项"不报错)均截图确认。
- render_pi_cost 修复用真实配置文件验证(备份→临时改 agents.pi 分配→apply→
  查 pi 侧 models.json cost 值→复原),未污染真实数据。

### Key Facts

- **docker compose up --force-recreate 单个服务(如 app)会打断
  `network_mode: service:app` 的侧车(vnc/code-server)——必须同时
  force-recreate 侧车才能恢复(restart 不够,会报"No such container"因为
  引用的是旧 app 容器 ID)。**
- vnc 容器 `/tmp` 是 tmpfs,`docker cp` 到 `/tmp/xxx` 会静默失败(exit 0 但
  文件不存在)——cp 到 `/home/gem/` 才行。
- CDP 通过 `.click()` DOM 方法有时不触发 React 合成事件(表现为"点击成功但
  状态不变"),改用 `dispatchEvent(new MouseEvent('click', {bubbles:true}))`
  稳定触发。
- Trellis-check 复查抓到:implement.md 收尾清单被我提前用 sed 全部打勾,但
  spec 更新那一项其实还没写——之后补上了。教训:收尾勾选清单前,先做完
  再勾,不要"打勾代表完成意图"。

### Git Commits

| Hash | Message |
|------|---------|
| (本次) | feat: 供应商表单 pi-web 流 + models.dev 集成(R1) |

### Status

[OK] **Completed**

### Next Steps

- 回父任务 08-27-models-config-v2 标记 R1 完成(2/4);ModelPicker 组件已备好,
  下一步 `08-27-agent-tabs-live-config`(R2+R3)接线。
## 2026-08-27 | pi/opencode 页签 live 配置管理(08-27-agent-tabs-live-config,R2+R3)

### What

- pi/opencode 页签从「一行 readback」升级为完整 live 管理:LiveProviderList
  列出 agent 原生配置里的每个 provider 节点,行级「同步到供应商库(幂等)/
  字段级编辑/删除(清悬空默认)」;分配模型改 ModelPicker 选择(canonical
  models[] 为源,不再手填)。
- 后端三条新路由:PUT/DELETE `/api/models/agents/:agent/provider/:id`、
  POST `.../sync`(body `{id?}`);live 回读扩展 providers[] 摘要(pi 双文件
  独立容错;opencode json5 容错 + api 由 npm 反推)。
- edit/delete 复用 apply 管线(键级合并 + backup_write_verify_json);
  sync 复用 store 导入适配器(import_pi_providers/import_opencode_providers,
  only 过滤同时接受原生键与 sanitize id——trellis-check 抓到的跨层 ID 域缺陷)。

### Key Facts

- **live 通道不是第二配置源**:它是「吸收 agent 侧手改」的入口,canonical
  仍是 SSOT;编辑只动 provider 级字段,模型级编辑回库里的 ProviderEditor。
- `apiKey: ""` 在线上语义是「清空」——前端留空=不发送该字段(omit=保留),
  这个约定写进了 model-config-guide.md 的 live 管理段。
- pi 节点没有 name 字段(patch.name pi 侧忽略);opencode 的 api↔npm 是
  渲染器的忠实逆映射(anthropic-messages↔@ai-sdk/anthropic)。
- 测试在 throwaway 容器跑(rust:1-bookworm + aio-cargo-registry 卷);
  cargo test models 208 绿(基线 173 + 本次 35)。serde_json Map 是 BTreeMap,
  测试断言不能假设数组顺序,按 id find。
- 真实数据实测后必须复原:pi/opencode 原生文件用 apply 产生的 .aio-bak-*
  备份逐字节还原到会话起始状态(canonical 的既有 mismatch 是用户状态,保留)。

### Git Commits

| Hash | Message |
|------|---------|
| (本次) | feat: pi/opencode 页签 live 配置管理 + 供应商模型列表复用(R2+R3) |

### Status

[OK] **Completed**

### Next Steps

- 回父任务 08-27-models-config-v2:剩「集成」一项(连续全链路容器实测:
  pi-web 新增 → pi/opencode 复用 → preset 切换 → apply → usage 对账),
  通过后父级收口。


## Session 2: pi/opencode 页签 live 配置管理(R2+R3)

**Date**: 2026-08-27
**Task**: pi/opencode 页签 live 配置管理(R2+R3)
**Branch**: `feat/aio-sandbox-mvp`

### Summary

LiveProviderList 行级同步/编辑/删除 + ModelPicker 分配接线;三条新路由(PUT/DELETE provider/:id、POST sync)复用 apply 管线;live 回读 providers[] 摘要双文件独立容错;trellis-check 抓修 sync 过滤器 ID 域缺陷(raw key vs sanitize id);cargo test models 208 绿;容器实测全链路并复原;spec/journal/父任务 R2/R3 勾选同步

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `8aecf44` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete

## Session 3: Models 二期集成验证(父级收口)

**Date**: 2026-08-27
**Task**: 08-27-models-config-v2 父级集成——连续全链路容器实测
**Branch**: `feat/aio-sandbox-mvp`

### Summary

四子任务(R1-R5)全绿后,父级在容器内跑一条链走通全链路:
供应商新增(discover→catalog→PUT)→ pi/opencode 复用 mock 供应商 → claude/codex
多 preset 增删+切换 → apply 写原生文件 → usage 逐行对账。全部 PASS,现场逐字节
复原(5 文件 md5 全等),父 prd 集成项打勾,任务收口。

### 实测过程

1. **备份基线**:docker cp 容器内 `.aio/models.json`、`.pi/agent/{models,settings}.json`、
   `.config/opencode/opencode.jsonc`、`.claude/settings.json` 到宿主 `$INTEG_BAK`,md5 记录。
2. **mock 服务**:容器内起 `python3 http.server` 监听 127.0.0.1:18080,
   `/v1/models` 返回 2 个模型(mock-eagle-4/mock-falcon-mini)。
3. **R1 链**(pi-web 式新增):
   - `POST /api/models/discover`(literal baseUrl=mock)→ `{models:[eagle,falcon], endpoint}`
   - `GET /api/models/catalog` → models.dev 200,二次调用 6ms 命中 1h 缓存
   - GET config → 追加 `integ-mock` → PUT → 回读:mask key `sk-****0111`,models 在
4. **R2 链**(pi):`agents.pi={integ-mock, mock-eagle-4}` → PUT → `POST apply/pi`
   → `~/.pi/agent/models.json` 新增 integ-mock 节点(既有 3 节点保留)
   + `settings.json` 默认切 mock-eagle-4(既有 packages/theme 等键全保留)
   → `GET agents` pi.live 反映 integ-mock/mock-eagle-4。
5. **R3 链**(opencode):`agents.opencode={integ-mock, mock-falcon-mini}` → apply
   → opencode.jsonc:integ-mock 块 npm=`@ai-sdk/openai-compatible`(api 逆映射正确)、
   既有块保留、默认切 mock-falcon-mini。
6. **R4 链**(claude/codex preset):
   - 负向:preset model 不在 provider.models[] → PUT 400(model not found)——顺带
     验证 validate 契约;悬空 current → PUT 400。
   - 增第二 preset(不带 id)→ 后端 backfill(id: claude=`preset-19e88`,codex=`preset-6b1e4`)。
   - 切 current → apply → `~/.claude/settings.json` env(AUTH_TOKEN/BASE_URL/MODEL/
     HAIKU)随新 preset 变;`~/.codex/{config.toml,auth.json}` 同步(model/wire_api=chat/
     base_url + OPENAI_API_KEY)。claude/codex 容器内未装二进制,apply 仍按文件渲染,
     验证以文件内容为准(与 R4 子任务一致)。
7. **R5 链**(usage 对账):
   - pi deepseek-v4-pro:原始 sessions jsonl 手算 in=28329 out=3941 cacheR=294656
     cacheW=0,与 API 精确一致;cost 用日志自带 total=0.016819912999999995 一致。
   - opencode provider-1/deepseek-v4-flash:sqlite message 表手算 in=7776 out=10
     cacheR=0 cacheW=0 精确一致;日志 cost=0 → 补算公式
     `7776/1e6*0.14 + 10/1e6*0.28 = 0.00109144` 与 API 精确一致($/M 约定)。
8. **复原**:cat > 内容级写回 5 文件(md5 全等)+ rm -rf ~/.codex + 杀 mock +
   清 4 个新增 .aio-bak-*(与 pre-list 比对) + 容器内临时文件;终态 GET config
   providers/agents 与会话起始语义一致。

### 关键结论

- 全链路各环契约兑现:discover/catalog/apply/preset 切换/live 回读/usage 归账
  全部与设计一致;preset 校验(model 必须在 provider.models[]、current 不悬空)
  与 usage 补算 $/M 公式实测精确。
- claude/codex 未安装不影响文件级验证(apply 不依赖二进制)。
- canonical 既有 mismatch(opencode live 默认 provider-1)是用户状态,非本任务引入;
  复原后仍保留。

### Status

[OK] **Completed**

### Next Steps

- 无:父任务 08-27-models-config-v2 全部子项(含集成)完成,收口。残留:
  容器内 mock zombie 进程(无监听,无害)。

---

## 2026-08-31 | 08-31-ci-image-pipelines 首跑校准（run 33396092303）

### 事件

push main（5d232e2）→ `images` workflow 首次真实触发，全绿。仓库
Zhruoshui/aio-devbox（public），GHCR 命名空间 ghcr.io/zhruoshui。

### 校准数据（job 时长）

| job | 时长 | 备注 |
|-----|------|------|
| prepare | 2s | |
| vnc | 2m6s | 含构建+抽查+push |
| images (minimal) | 8m29s | 含栈冒烟（runner 上 compose 起→curl 200→down） |
| images (full) | 12m10s | 冷缓存、13 场景片段 |

### 结论与决策

- **超时不调**：full 12m vs 上限 90m（14%），余量留给场景增长，维持 90min。
- **mode=max 范围不调**：aio-config/app 保持 max，其余 min；per-variant scope
  维持。10GB 预算按当前规模充裕。
- 缓存 API 列 0 条（token scope 或归因待查）；真实命中率以二次运行时长实证。
- GHCR 匿名拉取验证：14/14 标签（base/app/cs × minimal/full/±-5d232e2 + vnc
  latest/5d232e2）匿名 manifest 200。
- 推送通道注意：宿主 gh token 需 `workflow` scope（gh auth refresh -s workflow
  + sbx secret set），否则 workflow 文件推送被 remote rejected。

### Status

[OK] AC1 达成（push main 双变体+vnc 全绿、7 镜像 14 标签匿名可拉）；AC2 的
CI 侧孪生（同款冒烟）已在 minimal job 内通过。待办：本地 make pull 实测（干净
环境选择待用户定）。

### AC2 本地实测（就地法，2026-08-31 同日）

快照本地镜像 ID → `make pull VARIANT=minimal`（拉+retag+备料一次过）→
`make up NOBUILD=1 PROFILES="code-server vnc"`（compose 检测镜像漂移自动重建）。
验收：网关 basic-auth 200；app 容器 root/`/root`、node+python3 在位、rustc
缺席（真 minimal）；四容器健康。测毕按快照 retag 回原镜像 ID + up
--force-recreate 恢复开发栈（rustc 1.98.0 复现，网关 200）。

**AC1–AC5 全部达成，任务收口。**


## Session 3: CI 镜像流水线落地：双预设 GHCR 自动构建全绿 + 首跑校准与 AC2 实测

**Date**: 2026-08-31
**Task**: CI 镜像流水线落地：双预设 GHCR 自动构建全绿 + 首跑校准与 AC2 实测
**Branch**: `main`

### Summary

08-31-ci-image-pipelines 全周期完成：manifest 通配符 scenarios=[*]（4 单测）+ .aio/presets 双预设；app/code-server ARG BASE_IMAGE 参数化；images.yml 三 job 流水线（PR 零 push/gha 缓存/探针 bash -lc/minimal 冒烟）；make pull 消费侧+双语文档；Dockerfile.base 移出版本控制。仓库 Zhruoshui/aio-devbox 建立并推送 main，首跑 run 33396092303 全绿（full 冷缓存 12m10s，14 标签匿名可拉，AC1 达成）；AC2 就地实测 make pull minimal→up→curl 200→rustc 缺席佐证，测毕恢复开发栈。超时/mode=max 维持不调。宿主 gh token 需补 workflow scope 的坑已记 journal。spec 沉淀 guides/ci-image-conventions.md。归档 4 任务（本任务+pi 三连）。

### Main Changes

(Add details)

### Git Commits

| Hash | Message |
|------|---------|
| `fa4db01` | (see git log) |
| `c9eb0a2` | (see git log) |
| `4d52472` | (see git log) |
| `ec6ef8b` | (see git log) |
| `991849a` | (see git log) |
| `ef9f42d` | (see git log) |
| `89d9491` | (see git log) |
| `5d232e2` | (see git log) |

### Testing

- [OK] (Add test results)

### Status

[OK] **Completed**

### Next Steps

- None - task complete
