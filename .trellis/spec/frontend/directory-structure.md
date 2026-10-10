# Directory Structure

## mgr-web/ — 唯一前端 SPA(unified UI, 09-09-sandbox-mgr-unified)

09-09 D1/D2: golden-layout 工作区自 web/ 整体迁入,mgr-web 成为**唯一**
用户界面(工作区 + 管理页)。页面切换是 App 内内存 state(可辨识联合
view),无路由库;golden-layout popout 子窗口以 lone workspace 渲染
(`IS_POPOUT_CHILD`,无 admin shell)。

```
mgr-web/src/
├── types.ts            # mgr API payload 单一 owner ↔ mgr/src/routes.rs
├── api.ts              # typed fetch 边界(as 只许在这里出现)
├── i18n.ts             # zh-CN/en flat table(t() 强类型 keyof Strings,
│                       #   漏 key = tsc 报错,即双语门)
├── gl-kumo.css         # golden-layout Kumo 主题覆盖
├── styles.css          # Kumo token 层([data-mode] 深浅色不变) + 页面
│                       #   局部样式(仅页面私有类;通用组件类已上收)
├── components.css      # 设计师组件层(09-11 原型重构,移植原型 mgr-web.css):
│                       #   rail/panel/sdot/badge/btn/segmented/tabs/chip/
│                       #   card/overlay/dialog/menu/pop/drawer/table/tree…
│                       #   只消费 token,无硬编码色;由 main.tsx 在
│                       #   styles.css 之后 import
├── icons.tsx           # 24px stroke 图标集(PATHS/IconName 单一 owner,
│                       #   图标名与原型 mgr-shell.js 对齐)
├── components/
│   ├── Dialogs.tsx           # 共享对话框(确认/输入/prompt 等,组件层)
│   ├── SourceCodeEditor.tsx  # 轻量 textarea 源码编辑器(沙箱离线无 prism;
│   │                         #   JSON/TOML 双向绑定 + 解析错误提示)
│   └── TemplatePickerModal.tsx # 从 mgr /api/models/templates 拉公共模板
│                             #   (apiKey 永不外传)填表单
├── App.tsx             # shell: 48px rail 图标栏(品牌+五导航+主题/语言,
│                       #   激活态 accent 指示) + 工作区侧面板(aside.panel,
│                       #   键 mgr.panelHidden);管理页包 .page(1200px 居中
│                       #   + .page-head);mgr.sidebarCollapsed 已退役(D5);
│                       #   wsFocus = goWorkspace 聚焦
└── pages/
    ├── WorkspacePage.tsx    # golden-layout 工作区(默认落地页): 布局键
    │                        # mgr.layout / seq 池 / popout / 拖拽遮罩
    ├── SandboxListPage.tsx  # 卡片列表(segmented 筛选+搜索/sbx 卡片/删除
    │                        #   .dialog);model-profile 快捷指派已删(cc-switch)
    ├── CreatePage.tsx / AdoptPage.tsx / EditPage.tsx (共用 EnvPicker;
    │                        EditPage 无标签栏直出 env 向导——模型配置标签
    │                        已迁工作区 pane, 10-10-models-workspace-pane)
    ├── ServicesPicker.tsx   # 创建/编辑的服务开关区(四开关,pi-web 联动)
    ├── JobView.tsx / ImagesPage.tsx(+ImageDetailDialog) / UsagePage.tsx
    ├── UserToolsPage.tsx    # 用户工具编辑器
    ├── models/             # ModelsPage(cc-switch 供应商库): ProviderGrid
    │                        #   恒渲染(空库=仅新增幽灵卡片)+ ModelRow 卡片
    │                        #   + ProviderEditorModal 中心弹窗;
    │                        #   SandboxModelConfigTab(由 workspace/panes/
    │                        #   ModelsPane 挂载,四 agent 子标签:
    │                        #   SwitchAgentPanel(claude/codex 快照切换) +
    │                        #   IncrementalAgentPanel(pi/opencode 读-改-合并));
    │                        #   sandboxTypes.ts = 沙箱 /api/models/* 边界解码
    │                        #   单一 owner
    └── workspace/
        ├── paneUrl.ts      # per-sandbox URL 唯一工厂
        ├── SandboxTree.tsx # 面板头+.ws-search 筛选+tree-toggle 行结构+
        │                   #   服务叶子+注册按钮行;mgr.treeCollapsed 独立键
        ├── NodeMenu.tsx    # 「更多」fixed .menu(role=menu,Escape/外点关)
        ├── RegisterDialog.tsx  # 注册自定义按钮 .dialog
        ├── panes/          # XtermPane / IframePane / CodeServerPane /
        │                   #   ModelsPane(page 类型, 挂 SandboxModelConfigTab;
        │                   #   per-sandbox 绑定, componentState.sandbox)
        └── types.ts        # manifest + RegisterButtonInput 契约 +
                            #   injectSyntheticServices(modelsConfig 条目
                            #   客户端合成的单一 owner)
```

约定:

- **types.ts 是 mgr-api 契约的唯一前端 owner**（沙箱 agent 配置的边界
  解码在 `pages/models/sandboxTypes.ts`——它是 app `/api/sbx/:name/api/
  models/*` 代理的单一 owner；PUT model_profile 已随 profile 系统删除）。
- **panes/ 一类 service type 一个通用 pane**: XtermPane(agent/终端)、
  IframePane(web/VNC/pi-web)、CodeServerPane(按需拉起 + probe 轮询,
  D4)、ModelsPane(page/modelsConfig, 10-10)。新 service 是 manifest/按钮
  条目,**不加** React 组件,除非出现新 type;page 类型例外——条目由
  mgr-web 客户端合成(见 types.ts),**不进** services.toml。
- **一切沙箱面经 mgr 代理**(paneUrl.ts 是唯一 URL 工厂): 终端 WS =
  `/api/sbx/<name>/api/term/ws?cmd=...`(same-origin),iframe 类经子域名。
  见 backend/sandbox-mgr-ops.md 契约 10。
- **cc-switch 模型配置**(10-04-model-config-cc-switch): mgr 只有共享供应
  商库(模板源,`/api/models/providers` + `/api/models/templates`);每个
  沙箱独立持有自己的 agent 配置(`/api/sbx/<name>/api/models/config`),
  不存在 profile 作用域查询参数,也没有跨沙箱指派 UI。
- always_on 场景在 EnvPicker 锁定显示(不可取消),且其 id **永远不进**
  `env.scenarios`(后端会拒,canonical env 契约)。
- **S1 服务区(09-10-mgr-create-services)**: pi/pi-web 已从 always_on
  翻转成**可选场景**(`scenarios/{pi,pi-web}/scenario.toml` 均
  `always_on = false`),且在 EnvPicker 隐藏、改由 `ServicesPicker` 服务
  开关区接管(四开关 code-server/vnc/pi/pi-web,pi-web 联动 pi+vnc)。
  现存 always_on 场景仅 node/python——继续在 EnvPicker 锁定显示;服务
  区四开关与后端 `normalize_services` 是同一条校验链(前端联动、后端
  400 兜底),后端 400 文案中文。
- **CSS 分层(09-11 原型重构)**: 通用组件类(rail/panel/btn/badge/
  segmented/dialog/menu/pop/drawer/table/tree 等)归 `components.css`
  (源自 docs/Web-Prototype/ 设计基准,与原型 mgr-web.css 同源);token
  归 `styles.css` 顶部,页面私有类归 `styles.css` 对应页节。**组件层只
  消费 token,禁硬编码色**;页面组件优先复用组件层类,新增页面局部类
  需先确认组件层没有等价物。设计基准原型(5 HTML + mgr-web.css +
  mgr-shell.js)保留在 `docs/Web-Prototype/`。
- **管理页脚手架(S7)**: 所有非工作区页面统一 `.page` 容器(1200px
  居中)+ `.page-head`(h1 + `.sub` + `.page-actions` 放状态 badge/
  spinner/主操作);表单控件一律带 `.input` 类,校验态走
  `.field.invalid` + `.err`(components.css 标准),不再写裸
  `.field input` 覆盖。工作区主区全宽(golden-layout 需要),不套
  `.page`。
- **弹层契约**: `NodeMenu`(.menu)/删除确认(.overlay+.dialog)/供应商编
  辑与模板选择(.dialog 中心弹窗)统一:role 属性(menu/menuitem、dialog、
  alertdialog)、Escape 关闭、外点/scrim 点击关闭;fixed 定位弹层的锚点
  坐标由调用方计算(NodeMenu 模式)。
- 构建门: `npm run build` = `tsc --noEmit && vite build`;镜像经
  mgr/Dockerfile web-builder 阶段(node:20),由 mgr-api 静态服务。

## web/ — 已退役(墓碑)

09-09 D1: per-sandbox workbench SPA **已废弃**——golden-layout 工作区、
pane 组件、侧栏已迁入 mgr-web(见上节);沙箱内自有模型 UI 已消失(10-04
起模型配置 = mgr 供应商库模板 + 每沙箱 cc-switch 配置,入口是 mgr-web
工作区的 modelsConfig 合成 pane,10-10)。目录物理删除与 app 静态目录换
redirect 页在 Phase 6 落地。**不要再往 web/ 加任何东西**;迁移遗漏的
能力进 mgr-web 对应模块。terminal pty/resize 协议契约随 XtermPane 迁移,
见 [xterm-pane.md](./xterm-pane.md)。
