# PRD — mgr-web 多主题配色方案(Tokyo Night / Nord / Catppuccin)+ xterm 终端色适配

## 背景

mgr-web 目前只有亮/暗二态主题(Kumo 设计语言,`data-mode` + localStorage 持久化,顶栏按钮切换)。用户希望引入社区流行配色方案,同时让 xterm 终端面板的色彩(尤其 ANSI 16 色)随主题联动。

现状关键事实:

- 主题机制:`App.tsx` 中 `theme: Theme = "light" | "dark"`,`document.documentElement.dataset.mode = theme`,`THEME_KEY` 存 localStorage。
- 终端:`XtermPane.readTermTheme()` 只解析 `--term-bg/fg/selection` 三个变量到 xterm `ITheme`;**ANSI 16 色(black/red/green/yellow/blue/magenta/cyan/white + bright 变体)从未设置**,始终用 xterm 默认色板——主题切换后终端彩色输出(ls、prompt、git diff)不会跟随。
- 图表:`--chart-1..6` 调色板按亮/暗两种 surface 用 dataviz 校验器验证过(亮度带、色度地板、相邻 CVD ΔE、对 surface 对比度),是主题联动的一部分。
- 其余 UI 颜色全部经语义 token(`--bg/--surface/--fg/--muted/--accent/--border/...`),token 层之下无手写色值,这是多主题化的基础,无需改动组件层。

## 需求

> **09-20 范围修订**(用户确认最终主题集):删除 Kumo 亮/暗,最终 8 套主题全部来自 **Omarchy 官方主题库**(basecamp/omarchy `themes/<name>/colors.toml`,已存档至 `research/omarchy-palettes/`),保持主题体系可扩展(新增主题 = 一个 CSS 块 + 注册表一行)。

### R1 主题目录(最终)

| 主题 key | Omarchy 主题 | 明暗 |
|---|---|---|
| `tokyo-night` | Tokyo Night | 暗(**默认主题**) |
| `catppuccin` | Catppuccin(Mocha 基底) | 暗 |
| `ethereal` | Ethereal | 暗 |
| `nord` | Nord | 暗 |
| `vantablack` | Vantablack | 暗 |
| `catppuccin-latte` | Catppuccin Latte | 亮 |
| `white` | White | 亮 |
| `flexoki-light` | Flexoki Light | 亮 |

架构调整:`:root` 只保留模式无关原语(字体/间距/圆角/动效/阴影形状),**所有配色 token 下沉到各 `[data-theme]` 块**(每块完备,含 Kumo 此前的 oklch/color-mix 值全部移除);默认态由 index.html 静态写 `data-theme="tokyo-night"` 保证首帧正确。旧 localStorage 值(light/dark/kumo-*)统一映射到默认主题。

每套主题提供完整 token 覆盖:`--bg/--surface/--surface-warm/--fg/--fg-2/--muted/--meta/--border/--border-soft/--accent/--accent-on/--accent-hover/--accent-active/--success/--warn/--danger/--elev-raised/--focus-ring/--scrollbar-thumb(--hover)/--term-bg/--term-fg/--term-selection/--chart-1..6`,外加 **ANSI 16 色 token**(见 R3,映射规则采用 Omarchy 官方 `omarchy-theme-color` 的语义→ANSI 对照)。

### R2 主题选择器

- 顶栏现有亮暗切换按钮升级为主题选择入口(下拉或弹层,含色板缩略预览)。
- 选择持久化到 localStorage;首次进入维持现有默认(dark)。
- 主题切换零刷新:`data-mode` 机制扩展为 `data-mode`(明暗,供 color-scheme 等) + `data-theme`(具体方案 key)双属性驱动,XtermPane 已有 MutationObserver 热切换链路沿用。

### R3 xterm ANSI 16 色适配

- 新增 ANSI 16 色 CSS token,各主题给出官方色板映射(Tokyo Night / Nord / Catppuccin 官方 terminal 色板)。
- `readTermTheme()` 扩展解析全部 16 色 + cursor/selectionBackground,写入 xterm `ITheme` 的 `black/red/.../brightWhite`。
- 主题热切换时终端即时重着色(现有 modeObserver 链路)。

### R4 图表调色板适配

- 每套主题基于其 `--surface`,从该主题 Omarchy 官方色板(red/yellow/green/cyan/blue/magenta/orange + bright 变体)中选取 6 色图表分类色,dataviz 校验器(六项检查)验证;不达标同色相 snap-to-passing。已过校验的 tokyo-night/nord/catppuccin(mocha)/catppuccin-latte 色板沿用;新增 ethereal/vantablack/white/flexoki-light 按同一流程。

### R5 兼容与回退

- 未识别/旧版 `THEME_KEY` 值(light/dark/kumo-light/kumo-dark/catppuccin-mocha)回退 `tokyo-night`。
- `gl-kumo.css`(golden-layout chrome)、`components.css` 若存在硬编码色需排查,统一改走 token。
- 亮色 color-scheme / 暗色 color-scheme 随主题明暗正确设置。

## 非目标

- 不做用户自定义主题编辑器(只做预置方案)。
- 不做后端/沙箱侧的主题推送(app 服务的 web 面板不在本次范围)。
- 不改变现有 Kumo 亮/暗的视觉表现。

## 验收标准

- AC1:顶栏可切换 8 种主题(Omarchy 集),刷新后保持,无页面刷新闪白;默认 tokyo-night。
- AC2:每个主题下 Usage 页四类图表(条形/环形/趋势/汇总卡)颜色协调、可辨识;dataviz 校验器对每主题 `--chart-N` 在对应 surface 上无 FAIL(对比度 WARN 需有 relief:页面自带文字标签+明细表,视为满足)。
- AC3:终端内运行 `ls --color` / `git diff` 等彩色输出,8 主题的 ANSI 16 色与 Omarchy 桌面终端一致(同一 colors.toml 来源),切换主题即时重着色,无需重连 pty。
- AC4:所有现有页面(沙箱列表/编辑/镜像/Usage/工作区)在 8 主题下无未 token 化的硬编码色残留(目检 + grep 排查)。
- AC5:`npm run build` 通过;TS 无新错误。
- AC6(扩展性):新增第 9 套主题只需 ①styles.css 加一个 `[data-theme]` 块 ②themes.ts 注册表加一行 ③i18n 加词条——无需改任何组件代码;且选择器在任意主题数量与视口高度下完整可用(菜单内部滚动,不依赖缩放页面)。
