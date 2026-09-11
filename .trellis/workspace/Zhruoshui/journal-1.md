# Journal - Zhruoshui (Part 1)

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

## Session 4: sandbox-mgr Phase 5——存量纳管 + 全栈去认证 + 收尾

**Date**: 2026-09-09
**Task**: 09-08-sandbox-mgr-tui (Phase 5/5)
**Branch**: `feat/sandbox-mgr`

### Summary

Phase 5 全部完成: 导入向导(adopt 流程后端+mgr-web 页)、存量栈去认证(D9)、
README/wiki 双语安全边界与多沙箱使用文档、A9/A7 全量容器级验证、PR #14。
两轨并行 trellis-implement(文件集不相交): 轨 A 导入向导、轨 B 去认证+文档;
trellis-check PASS 含 6 处自修(关键: usage.rs 原跳过 adopted 沙箱,但 adopt
恰好把 app 容器以 sbx-<name>-piweb 别名接入了 aio-mgr-net——与 fetch_one
URL 完全一致,移除过滤,adopted 沙箱进入 usage 汇总)。

### Main Changes

- mgr/src/routes.rs: POST /api/sandboxes/adopt(路径解析/运行检查/容器发现/
  别名双连/登记/regenerate) + sandbox_json/start/stop/restart 的 adopted
  分支(start 后 fresh ps 重连别名——外部栈 down/up 丢网络成员身份) +
  unadopt 同步去登记(不删容器,best-effort 断连)
- mgr/src/docker.rs: network_connect_alias(already-exists → disconnect+
  reconnect 幂等)/network_disconnect/compose_*_file 无 -p 外部栈变体
  (project 由文件目录推导)+parse_ps_output 抽取
- mgr-web: AdoptPage 向导 + 列表导入按钮 + adopted 删除文案(无卷
  checkbox)+ 18 i18n 键
- 去认证: gateway/Caddyfile 删 basicauth、删 entrypoint.sh/secrets、
  Makefile hash/ensure-hash/save/load、CI 冒烟 -u admin:admin、
  .env.example/.gitignore
- spec: sandbox-mgr-ops.md 契约 8(别名三方一致+外部 compose 无 -p+
  注销空 200 语义)、契约 9(全栈无认证安全边界+残留清理清单)

### Key Findings

- Docker 29.6.1: 已连网络的容器重复 connect 报 already exists(设计内的
  disconnect+reconnect 路径),但**已停止容器**静默 exit 0 且丢弃新别名
  ——adopted start 在 up 后连接,不受影响
- 注销后子域名返回**空 200**(caddy 无 catch-all 时未知 Host 默认行为),
  不是 404——判定域名失效用响应体字节数
- 外部 compose 不带 -p 时 project 由文件目录推导,容器内重命名挂载下
  (如 /repo)会推导错——mgr 容器化形态安全(compose.yml PATH IDENTITY
  挂载自身宿主路径)
- 本沙箱 shell 在外层沙箱(非栈容器内),curl 经代理层拦截——验证一律
  --noproxy '*' 或 docker exec 进容器

### Git Commits

| Hash | Message |
|------|---------|
| `98ac764` | docs(spec): 契约 8/9 + 勾选 Phase 5 回滚点 |
| (前一提交) | feat(mgr): 存量纳管导入向导 + 全栈去认证 + README/wiki (Phase 5) |

### Testing

- [OK] cargo test --workspace: 326 绿(app 214/config 23/aio-mgr 50/
  aio-models 39,较 Phase 4 +10)
- [OK] A9: tmp-a9 临时外部栈 adopt→别名双连→列表→子域名 200→启停重启→
  PUT 拒改→unadopt 容器存活/别名清/Caddyfile 清/域名 0 字节
- [OK] A7 全量: a7full 创建→子域名 200→stop/start/restart→DELETE volumes=1
  →容器/卷/instance 目录/路由/域名/列表全清
- [OK] make up 回归: 四容器健康、无认证 200、code-server/vnc 子路径通
- [OK] mgr-web build(tsc+vite)、README 双语锚点核对、CI 冒烟自洽

### Status

[OK] Phase 5 完成,PR #14 已开。留宿主机: 浏览器 A1/A2/A9 人工目视验收
(mgr-web 导入向导交互、adopted 卡片徽标、删除确认交互)。

## Session 5: S1 创建向导重构——服务四开关 + 场景四层分组

**Date**: 2026-09-10
**Task**: 09-10-mgr-create-services (S1,父 09-10-mgr-web-ux-batch2 D1/D2)

### Summary

S1 完成: create/edit API + 前端向导新增服务四开关(code-server/vnc/pi/
pi-web),场景选择按 L1-L4 四层分组 + description;jobs/composegen 按开关
条件化构建。关键设计修正: pi/pi-web 原本是 always_on(issue #8 必装基线),
父 PRD D1 授权翻转为可选场景,否则默认全开创建直接 400。AC1-AC5 全过:
向导形态/徽章列表(puppeteer 实机)、无服务组合 compose+镜像省略、pi-web
依赖联动+400、旧沙箱 sbx-111 回归、361 测试全绿。

### Main Changes

- mgr/src/db.rs: sandboxes.services_json 列(code_server/vnc 两布尔 + 迁移
  守卫);NULL→全开读取;Services 逐字段 serde default(部分 JSON 不全 false)
- mgr/src/routes.rs: normalize_services 折叠 pi/pi-web 进 env.scenarios
  (单源真值),pi_web 强制依赖 pi+vnc(两者缺一 400,R1/AC3);PUT 用当前行
  形状重折叠(installed_services_of,NULL→四键全开)保 pre-S1 行不静默剥
  服务;service_start 未装→400
- mgr/src/{jobs,docker,composegen}.rs: code_server=false 跳过 cs 镜像与
  compose 段;vnc=false 不进 compose/up profiles;UP_PROFILES const →
  up_profiles(include_vnc);修 code_server_block {short} 字面量 bug
  (原 const 永不替换 → docker invalid reference format)
- scenarios/{pi,pi-web}: always_on true→false;config/src 注释同步
  (现存 always_on 仅 node/python)
- mgr-web: ServicesPicker 新组件(四开关+pi-web 联动+只读徽章)、EnvPicker
  四层分组+description、列表/编辑服务徽章、12 个 i18n 键

### Review Gate (trellis-check)

step5 后 review: 18 文件对 5 spec,10 处问题全修。P0: ①向导复选框失效
(标签当 key 传 set);②PUT 静默掉 pi/pi-web(pre-S1 行推导成未装→重建丢
服务,AC4 回归);③列表/详情 pre-S1 显 false;④pi_web 校验不对称
(只查 vnc 不查 pi)→ !b.pi || !b.vnc。修复后单测 358→361。实机复核:
sbx-111 installed_services 四键全开、向导复选框翻转/pi-web 联动。

### Git Commits

| Hash | Message |
|------|---------|
| `e519a91` | feat(mgr): 创建向导服务四开关 + 场景四层分组 (S1) |
| `87eaae9` | docs(spec): S1 服务开关契约落 spec 三份 |

### Testing

- [OK] cargo test --workspace: 361 绿(aio-mgr 215 含 9 个 S1 单测)
- [OK] mgr-web: tsc --noEmit 0 错 + vite build
- [OK] 实机(重建部署后): AC1 向导/徽章结构 + 复选框交互 + pi-web 联动;
  AC2 svcoff/svcfresh compose 无服务段+无 cs 镜像;AC3 400;AC4 sbx-111
  列表/installed_services 全开
- [留宿主机] 浏览器目视: 新建向导整体观感、列表徽章视觉

### Status

[OK] S1 完成归档。旁支 S3(images-manage)/模型指派/usage 图表等仍在规划。

## Session 6: S2 模型配置重构——agent 指派三层结构 (D4)

### Summary

cc-switch 心智重构落地:全局供应商库(不变)× profile × agent 卡片式指派 →
沙箱指派 profile + agent 子集,只渲染勾选的 agent。三层结构本已存在,
本次补「agent 子集」维度 + 卡片化 UI + 渲染过滤。

### Main Changes

- **mgr 数据模型**: `assignments` 值从裸字符串升级为 `StoredAssignment
  {profile, agents}`;反序列化兼容旧 `{"<sbx>": "<profile-id>"}`(= agents
  None 全指派,AC4);`agents` 语义 None=全指派/[]=零指派(拉取 404 → 保持
  本地,AC3)/[names]=精确子集。`VALID_AGENTS` 白名单(未知 agent 400)。
  `set_assignment/assignment/read_assignments` 适配;`assigned_profile`
  降为测试锚点。
- **mgr API**: `PUT /:name/model_profile` body 增 `agents`(整份替换语义);
  `sandbox_json` 增 `model_agents`;`GET /api/models/sync` payload 增
  `agents`,零指派 → 404。
- **app 同步**: `SyncPayload` 增 `#[serde(default)] agents`(旧 mgr 兼容);
  差异判定扩展 config+子集(子集变了也重渲染,`last_agents` 存 loop 状态);
  `apply_selected_agents` 过滤渲染——四个 renderer 零改动(R4);
  `apply_all_agents` 变 None 路径别称(测试锚点)。Some([]) 零指派在
  mgr 端就 404,app 端保持本地。
- **前端**: 新组件 `AgentAssignControl`(四 agent 复选,null=全选);EditPage
  指派含 agent 勾选;SandboxListPage profile chip → 快捷指派 popover(子集
  摘要 `pi+2`);Models 页 pi/opencode tab 供应商卡片墙(点卡=激活,active
  高亮)。types/api 增 `model_agents`。
- **spec**: api-contracts model_profile 契约 + sandbox-mgr-ops 契约 7 全量
  更新 agents 子集语义与回滚注意(旧代码读新形状 kv 失败)。

### Key Findings

- 界面 429 配额:子代理 trellis-implement ×3 全部 API 限额失败 → 主会话
  内联实现,安全兜底。
- 会话前工作区已有 app 端 mgr_sync/mod.rs 的 S2 改动(apply_selected_agents
  等),经核验完整且 223 测试全绿——只需补 mgr 端、前端集成与 spec。
- 前端 PUT 的「整份替换」陷阱:agents 省略会触发后端 serde default 放大
  为全指派——EditPage 始终携带 origAgents。
- `apply_all_agents` 无生产引用了 → 标 `#[cfg(test)]` 消除 dead_code。

### Git Commits

| Hash | Message |
|------|---------|
| `20d6831` | feat(mgr): S2 模型配置 agent 指派三层结构 (09-10) |

### Testing

- [OK] cargo test -p aio-mgr: 88 绿(含 4 个 S2 新测:旧 payload 迁移/白名单/
  sync agents+Some([])404/subset helper)
- [OK] cargo test -p aio-app: 223 绿(含既有 mgr_sync 子集/渲染过滤测试)
- [OK] mgr-web: tsc --noEmit 0 错 + vite build
- [留宿主机] 实机 AC2/AC3 链路(沙箱 A 指派 + 仅 pi/opencode → 60s 内
  ~/.pi 更新、~/.claude 不动;零指派 → 本地完全不动)

### Next Steps

- S1→S3 数据链路(S3 images-manage 依赖组合清单字段)等旁支仍在规划。
- 模型指派 UI 的浏览器目视复核留宿主机。
### Status

[OK] S2 实现+检查+spec 更新完成,已提交 `20d6831`(task 仍 in_progress,
待实机 AC 后归档)。

## Session 7: S3 镜像页增强——组合说明 + 删除 + 一键清理 (D3)

### Summary

images 表从只读列表升级为可管理:每镜像显示组合清单+体积,支持删除未引用
镜像组(异步 job)与一键清理(含构建缓存)。

### Main Changes

- **db**: images 表幂等增 combo 列(pragma 迁移,仿 services_json 范式);
  upsert_image 5 参(combo,ON CONFLICT COALESCE 不覆盖旧描述);
  list_images 返回 combo;新增 delete_image_row
- **envhash**: describe_combo(env, services)——场景+版本+服务开关可读描述。
  关键发现:db::Services 仅含 cs/vnc,**pi/pi-web 是 scenario**(S1
  normalize_services 折叠进 env.scenarios),须从 scenarios 推导
- **docker**: image_rmi(is_owned_image_tag 白名单,仅 sandbox- 前缀,防误删
  宿主镜像)/image_size/builder_prune 三原语
- **jobs**: spawn_image_delete(预检 refcount>0=409 + job 内重查兜底 + 三 tag
  顺序 rmi + 全成功才删行 + 失败保行 R5 不半删 + 不碰全局共享 vnc);
  spawn_image_cleanup(refcount=0 逐行删组 + builder prune + 回收 bytes 汇总)
- **routes**: GET /api/images 增 combo+size_bytes(实时 inspect 失败 null);
  POST /:env_hash/delete 与 /cleanup 走 202+job(迭代 Jobs 任务);
  env_hash 64-hex 校验
- **前端**: ImagesPage 加组合/体积列、行删(refcount>0 disabled+title)、
  一键清理、内联 job 轮询、confirm;Image/Job 类型扩

### Key Findings

- 先写了 describe_combo 5 布尔签名后撞上 db::Services 只有 cs/vnc 的事实
  → 改为 &Services + scenarios 推导,测试断言同步修正。
- list_images 返回类型变化连累 routes 消费处一一适配。
- cleanup 的回收 bytes 统计放进 run_image_delete 返回值(rmi 前逐个 size),
  避免 rmi 后 inspect 失败。
- ImagesPage 删除用内联 job 轮询(不整页跳 JobView,页内保留进度)。

### Git Commits

| Hash | Message |
|------|---------|
| `ba68d3d` | feat(mgr): S3 镜像页组合说明 + 删除 + 一键清理 (09-10) |

### Testing

- [OK] cargo test -p aio-mgr: 92 绿(新增 combo 迁移/describe_combo/docker
  白名单/delete_image_row 等)
- [OK] cargo test -p aio-app: 223 绿(未受影响)
- [OK] mgr-web: tsc --noEmit 0 错 + vite build
- [留宿主机] make mgr-up 重建后 AC1-AC4 目视:组合+体积显示、禁用态原因、
  docker images 真删、清理分列报告

### Next Steps

- S4 usage 图表/S5 侧栏折叠仍在规划。
- S3 实机 AC 复核留宿主机(S2 亦同)。
### Status

[OK] S3 实现+检查+spec 更新完成,已提交 `ba68d3d`(task 仍 in_progress,
待实机 AC 后归档)。

## Session 8: S4 用量图表——分沙箱条形 + 按天趋势 (D5)

### Summary

用量页补两图+时间维度:合计视图加「分沙箱」横向条形(点条跳沙箱视图);
单沙箱视图加「近 14 天趋势」(token 双系列柱 + 成本折线);明细表加日期
列 + 按日筛选(独立于窗口 chip)。

### Main Changes

- **app usage.rs byDay**: 新增 `DayUsage`/`UsageScan` 结构、`day_label`/
  `build_14_day_series` 纯函数;四个扫描器返回 `UsageScan{rows, by_day}`,
  **日桶累加移到 window cutoff 之前**(S4 关键语义:byDay 与窗口解耦);
  handler 合并日桶后按 `now_day-13..=now_day` 裁剪(修掉初版把 `_now_secs`
  弃用、today/7d 窗口截断趋势、all 窗口吐全史的 bug);无数据日不发合成行
  (前端 gap-fill)。cache 增 by_day。
- **mgr usage.rs**: 新增 `assemble_totals(entries)` 纯函数——对非 error
  entry 的 `usage.rows` sum in/out/cost(cost 缺失→0),error entry 全 0;
  GET /api/usage 响应增 `totals`。不入缓存(由 30s 缓存的 entries 派生)。
- **前端**: types.ts 增 `DayUsage`/`SandboxTotal` + byDay/totals 解码
  (旧后端缺字段→undefined 降级);charts.tsx 增 `SandboxBars`(点击条跳沙箱、
  hasCost 着色区分)+ `DayTrend`(SVG 柱状双系列 + 成本 polyline,无成本不画,
  gap-fill 14 天);UsagePage 增沙箱条形(合计视图)、按天趋势(单沙箱)、
  日期列+筛选;切沙箱重置筛选。i18n 5 个新 key + CSS。

### Key Findings

- 初版 `build_14_day_series` 把 `now_secs` 命名为 `_now_secs` 弃用——裁剪
  逻辑根本没写;且四扫描器 day 桶累加都在 `if t < cutoff { continue }` 之后,
  违反 design §1.2「恒定 14 天、与窗口解耦」。修复 = 累加前置 + `[today-13,
  today]` 字符串裁剪(`YYYY-MM-DD` 字典序即日序)。
- **DayTrend 初版漏渲染 `{bars}`**(只 push 不画)——bar 数组构建了但 JSX
  从未输出,检查 prd 的「柱状双系列」时发现。已补。
- JSON 数字比较坑:mgr 测试 `assert_eq!(json["cost"], 0)` 时 `Number(0.0)`
  ≠ 整数 0,须 `as_f64()`。
- 前端日期跨度用浏览器 UTC 计算(`Date.UTC`),与后端 UTC 裁剪一致;跨时区
  时按用户视角呈现(可接受)。

### Git Commits

| Hash | Message |
|------|---------|
| `38dad12` | feat(mgr): S4 用量图表 byDay + 分沙箱条形 + 按天趋势 (09-10) |

### Testing

- [OK] cargo test -p aio-app: 226 绿(新增 day_label/build_14_day_series/
  pi_scan by_day 3 测)
- [OK] cargo test -p aio-mgr: 94 绿(新增 assemble_totals 2 测)
- [OK] mgr-web: tsc --noEmit 0 错 + vite build
- [留宿主机] make mgr-up 重建后 AC1-AC4 目视:条形跳转、趋势数据与明细一致、
  日期筛选、无成本降级

### Next Steps

- S5 侧栏折叠仍在规划(09-10-mgr-sidebar-collapse)。
- S4 实机 AC 复核留宿主机(S2/S3 亦同,一批复核)。
### Status

[OK] S4 实现+检查+spec 更新完成,已提交 `38dad12`(task 仍 in_progress,
待实机 AC 后归档)。

## Session 9: S5 侧栏折叠——图标栏 + hover flyout (D6)

### Summary

两处折叠最大化工作区:App 侧栏可折叠为 48px 图标栏;WorkspacePage 的
SandboxTree 可折叠为竖向首字母图标条 + hover flyout。两折叠独立记忆。

### Main Changes

- **App 侧栏折叠 (R1)**: `App.tsx` 增 `sidebarCollapsed` state,键
  `mgr.sidebarCollapsed`(localStorage);`.sidebar.collapsed` 宽 48px,隐藏
  `.sb-title/.launch-label/.sb-group-label`,折叠态隐藏 brand 图标、collapse
  按钮居中(chev-l/chev-r 翻转);展开态完全不变(AC4)。
- **SandboxTree 折叠 (R2/R3)**: `WorkspacePage` 增 `treeCollapsed` state,
  键 `mgr.treeCollapsed`(R4 两键互不影响);SandboxTree 增
  `collapsed`/`onCollapseToggle` props + 折叠分支:每沙箱一个首字母圆
  (`.ws-cavatar`,stopped 置灰),hover(`onMouseEnter`/`onMouseLeave` 挂
  `.ws-cnode`)弹 `.ws-flyout` 浮层(绝对定位贴右侧),flyout 内按钮组
  = 展开态 `buttonsOf` 同源(manifest 探测一致),stopped 沙箱按钮置灰 +
  start 按钮(flyout footer),register 按钮;点击 launch **不关闭** flyout
  (React mouseleave 看整个 DOM subtree,flyout 是 node child)。
- **R5 翻转**: `@media (max-width: 560px)` flyout 左开(`right: 100%`)。
- **CSS**: `.ws-cnode/.ws-cavatar/.ws-flyout/.ws-tree.collapsed` 全套,
  flyout `max-height: 70vh + overflow-y auto`;折叠 rail 顶 collapse 按钮
  (展开态居右、折叠态居中)。
- **i18n**: `collapse/expandSidebar` + `collapse/expandTree` 双语言。
- **spec**: directory-structure.md 补 App/sidebar 与 SandboxTree 折叠说明。

### Key Findings

- 折叠态 avatar 点击语义:最初 `onToggle`(展开树节点)在折叠态无意义,
  改为 `onCollapseToggle`(展开整个树)——折叠态下一格即整栏。
- flyout 点击保持的关键:React `onMouseLeave` 只在指针**离开整个子树**
  时触发;flyout 作为 `.ws-cnode` 的 DOM child,鼠标从 avatar 移入 flyout
  不触发关闭(连续开多个 pane)。
- 折叠态 `.sb-head` 有 brand + collapse 两元素会挤出 48px——折叠态
  隐藏 brand,让 collapse 按钮独占居中。
- vnc 容器在 `container:` netns,与 aio-mgr-net 的 mgr-api 异网,wget 亦缺,
  浏览器验证走不通 → 依 S2/S3/S4 惯例留宿主机目视。

### Git Commits

| Hash | Message |
|------|---------|
| `849cb9b` | feat(mgr): S5 侧栏折叠图标栏 + 沙箱树 flyout (09-10) |

### Testing

- [OK] mgr-web: tsc --noEmit 0 错 + vite build
- [静态审查] flyout hover/点击语义、stopped 置灰、两折叠键隔离、展开态
  回归(折叠分支与展开分支完全 parallel)
- [留宿主机] make mgr-up 重建后 AC1-AC5 目视:折叠/刷新保持、flyout
  hover+连续开 pane、stopped 启动入口、展开态回归、窄屏翻转

### Next Steps

- S1-S5 五子任务全部完成,待宿主机一批实机 AC 后归档(batch2 父任务
  跨子任务验收)。
- S5 实机复核留宿主机。
### Status

[OK] S5 实现+检查+spec 更新完成,已提交 `849cb9b`(task 仍 in_progress,
待实机 AC 后归档)。
