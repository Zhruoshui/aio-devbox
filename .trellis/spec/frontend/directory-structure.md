# Directory Structure

## mgr-web/ — 唯一前端 SPA(unified UI, 09-09-sandbox-mgr-unified)

09-09 D1/D2: golden-layout 工作区自 web/ 整体迁入,mgr-web 成为**唯一**
用户界面(工作区 + 管理页)。页面切换是 App 内内存 state(可辨识联合
view),无路由库;golden-layout popout 子窗口以 lone workspace 渲染
(`IS_POPOUT_CHILD`,无 admin shell)。

```
mgr-web/src/
├── types.ts            # mgr API payload 单一 owner ↔ mgr/src/routes.rs
├── api.ts              # typed fetch 辀界(as 只许在这里出现)
├── i18n.ts             # zh-CN/en flat table
├── gl-kumo.css         # golden-layout Kumo 主题覆盖
├── styles.css          # Kumo token 层([data-mode] 深浅色不变)
├── App.tsx             # shell: 侧边导航(工作区/沙箱/镜像/模型/用量) +
│                       # 主题/语言(键前缀 mgr.*); wsFocus = goWorkspace 聚焦
└── pages/
    ├── WorkspacePage.tsx    # golden-layout 工作区(默认落地页): 布局键
    │                        # mgr.layout / seq 池 / popout / 拖拽遮罩
    ├── SandboxListPage.tsx  # 卡片列表(实时状态/指派 profile/启停/删除)
    ├── CreatePage.tsx / AdoptPage.tsx / EditPage.tsx (共用 EnvPicker;
    │                        Edit 含 model-profile 指派 select, D8)
    ├── JobView.tsx / ImagesPage.tsx / UsagePage.tsx
    ├── models/             # ModelsPage: profile 选择栏(D8) + 五个 tab
    │                        # (providers/pi/opencode/claude/codex)
    └── workspace/          # SandboxTree(沙箱树+展开懒加载 manifest +
        ├── paneUrl.ts      #   注册按钮) / RegisterDialog / types
        ├── panes/          # XtermPane / IframePane / CodeServerPane
        │                   #   (per-sandbox 绑定, componentState.sandbox)
        └── types.ts        # manifest + RegisterButtonInput 契约
```

约定:

- **types.ts 是 mgr-api 契约的唯一前端 owner**(PUT limits 三态、
  model_profile 字段、PUT model_profile 纯 kv 写语义注释就写在这里,
  见 backend/api-contracts.md mgr 节)。
- **panes/ 一类 service type 一个通用 pane**: XtermPane(agent/终端)、
  IframePane(web/VNC/pi-web)、CodeServerPane(按需拉起 + probe 轮询,
  D4)。新 service 是 manifest/按钮条目,**不加** React 组件,除非出现新
  type。
- **一切沙箱面经 mgr 代理**(paneUrl.ts 是唯一 URL 工厂): 终端 WS =
  `/api/sbx/<name>/api/term/ws?cmd=...`(same-origin),iframe 类经子域名。
  见 backend/sandbox-mgr-ops.md 契约 10。
- **profile 作用域**(D8): ModelsPage 全部 fetch 带 `?profile=`;EditPage
  指派 select 三态(未变不发/id 指派/显式空解绑,对齐 limits 三态)。
- always_on 场景在 EnvPicker 锁定显示(不可取消),且其 id **永远不进**
  `env.scenarios`(后端会拒,canonical env 契约)。
- **S1 服务区(09-10-mgr-create-services)**: pi/pi-web 已从 always_on
  翻转成**可选场景**(`scenarios/{pi,pi-web}/scenario.toml` 均
  `always_on = false`),且在 EnvPicker 隐藏、改由 `ServicesPicker` 服务
  开关区接管(四开关 code-server/vnc/pi/pi-web,pi-web 联动 pi+vnc)。
  现存 always_on 场景仅 node/python——继续在 EnvPicker 锁定显示;服务
  区四开关与后端 `normalize_services` 是同一条校验链(前端联动、后端
  400 兜底),后端 400 文案中文。
- 构建门: `npm run build` = `tsc --noEmit && vite build`;镜像经
  mgr/Dockerfile web-builder 阶段(node:20),由 mgr-api 静态服务。

## web/ — 已退役(墓碑)

09-09 D1: per-sandbox workbench SPA **已废弃**——golden-layout 工作区、
pane 组件、侧栏已迁入 mgr-web(见上节);沙箱内模型 UI 已消失(模型配置
上收 mgr per-profile, 契约 7)。目录物理删除与 app 静态目录换 redirect 页
在 Phase 6 落地。**不要再往 web/ 加任何东西**;迁移遗漏的能力进 mgr-web
对应模块。terminal pty/resize 协议契约随 XtermPane 迁移,见
[xterm-pane.md](./xterm-pane.md)。
