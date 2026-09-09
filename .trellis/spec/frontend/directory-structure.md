# Directory Structure

```
web/
├── index.html              # Vite entry HTML (mounts #root)
├── package.json            # type:module; scripts: dev/build/preview/typecheck
├── tsconfig.json           # strict, noUnusedLocals/Parameters
├── vite.config.ts
├── smoke-test.cjs          # manual end-to-end smoke test (puppeteer-core)
└── src/
    ├── main.tsx            # entry: createRoot(<App/>)
    ├── App.tsx             # workspace shell: fetch manifest, own golden-layout
    ├── layout.ts           # buildLayoutConfig(services) -> golden-layout LayoutConfig
    ├── types.ts            # Manifest, ServiceEntry (the manifest contract)
    ├── styles.css          # global + pane styles
    └── panes/
        ├── IframePane.tsx  # type=web  -> <iframe src={service.url}>
        └── XtermPane.tsx   # type=agent -> xterm.js over /api/term/ws
```

## Conventions

- **One generic pane per service `type`** in `panes/`. A new service is a
  `services.toml` entry - NO new React component unless it needs a new `type`.
  `PaneForService` (in `App.tsx`) dispatches on `service.type`.
- **`types.ts`** owns the manifest contract (`ServiceEntry`, `Manifest`) shared
  with the backend's `/api/manifest` response.
- **`layout.ts`** is the only place that knows golden-layout's config shape;
  `App.tsx` calls `buildLayoutConfig` and stays free of layout details.
- CSS is a single `styles.css` (global + pane classes like `.pane`,
  `.pane-iframe`, `.pane-xterm`); no CSS modules / styled-components yet.

## mgr-web/ — 第二个 SPA(sandbox-mgr 管理面)

09-08-sandbox-mgr-tui Phase 3。纯管理页面,**无 golden-layout**;页面切换是
App 内内存 state(可辨识联合 view),无路由库。复用 web/ 的三件套模式:

```
mgr-web/src/
├── types.ts            # mgr API payload 单一 owner ↔ mgr/src/routes.rs
├── api.ts              # typed fetch 边界(as 只许在这里出现)
├── i18n.ts             # zh-CN/en flat table(同 web/src/i18n.ts)
├── styles.css          # Kumo token 层自 web/src/styles.css 移植裁剪
│                       # ([data-mode] 深浅色覆盖不变)
├── App.tsx             # shell: 侧边导航 + 主题/语言切换(键前缀 mgr.*)
└── pages/              # SandboxList / Create / Edit(共用 EnvPicker) /
                        # JobView(1.5s 轮询) / Images
```

约定:
- **types.ts 是 mgr-api 契约的唯一前端 owner**(PUT limits 三态语义等注释
  就写在这里,见 backend/api-contracts.md mgr 节)。
- always_on 场景在 EnvPicker 锁定显示(不可取消),且其 id **永远不进**
  `env.scenarios`(后端会拒,canonical env 契约)。
- 构建门同 web/: `npm run build` = `tsc --noEmit && vite build`;镜像经
  mgr/Dockerfile web-builder 阶段(node:20),由 mgr-api 静态服务。
