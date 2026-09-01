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
