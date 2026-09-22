# Theming (09-20-theme-schemes)

> mgr-web 的多主题体系。8 套官方 Omarchy 主题,`[data-theme]` CSS 块驱动,零 CSS-in-JS。

## 架构契约

**单一真源拆分**:颜色值只存在于 `styles.css` 的 `[data-theme="<key>"]` 块;`themes.ts` 只有身份元数据(key/labelKey/mode/swatch);组件层永远不出现色值(swatch 缩略点是唯一例外——它们本身就是主题的预览)。

**双属性驱动**:`<html data-theme="<scheme-key>" data-mode="<light|dark>">`。
- `data-theme` 选中具体方案块;`data-mode` 是方案的静态派生属性(color-scheme 与 legacy 选择器兼容)。
- `App.tsx` 的一个 effect 同时写两者并持久化 `localStorage["mgr.theme"]`。
- `XtermPane` 的 MutationObserver 监听 `["data-mode","data-theme"]` 热重着色(不重连 pty);`charts.tsx::useThemeFlip` 同样监听双属性强制 Recharts 重绘(CSS 变量在 SVG fill 中需要一次 paint-cycle 才重新解析)。

**`:root` 禁区**:`:root` 只允许模式无关原语(字体/字号/行高/间距/容器/圆角/motion/阴影形状),不允许**定死配色值的** token(oklch 色值/配色 hex)。没有"默认配色层"——`index.html` 静态写 `<html data-theme="tokyo-night" data-mode="dark">` 保证无 JS 首帧正确。

> **合法例外:引用主题 token 的「环/阴影形状」**。`--elev-ring: 0 0 0 1px var(--border)`
> 与 `--control-ring: 0 0 0 1px color-mix(in oklch, var(--muted) 80%, transparent)`
> 都住在 `:root`,但**不构成配色层**:它们只定义环的形状/宽度,颜色由 `var(--border)`
> / `var(--muted)` 在**每主题**解析,8 套主题各得其所。
> 判据:形状可留 `:root`,**字面色值必须下沉到主题块**。
> 这样一条 `:root` 定义能服务全部主题——`--elev-ring` 有 22 处引用,
> 逐主题复制 8 份既冗余又极易漏改。

**悬空 `var()` 是系统性风险(2026-09-22 事故)**:未定义的 CSS 自定义属性会让
**整条声明**在 computed-value 阶段失效(落到初始值),且**没有任何构建期报错**。
09-20 主题重构把 `--elev-ring` 的定义删了、留下 22 处引用 → 全站卡片/输入框/按钮/
勾选框一次性失去边框。勾选框最致命:`.check` 与它的容器 `.rows` 同为
`background: var(--surface)`(flexoki-light 下连 `--bg` 都等于 `--surface`),
**这个 1px 环是二者唯一的区分手段**,环没了方框就完全不可见。

因此改 token 后必须跑悬空引用审计,差集必须为 0:

```bash
cd mgr-web && grep -rhoE "var\(--[a-zA-Z0-9_-]+" src/*.css | sed 's/var(//' | sort -u > /tmp/u
grep -rhoE "^\s*--[a-zA-Z0-9_-]+:" src/*.css | sed 's/^\s*//;s/:$//' | sort -u > /tmp/d
comm -23 /tmp/u /tmp/d    # 必须为空
```

交互控件(`.check`/`.radio`)的环另有 a11y 下限:环是该控件的唯一静止态标识,
须满足 WCAG 1.4.11 的 3:1 非文本对比度。`--control-ring` 的 80% 是**实测**出来的
最小达标 alpha(8 主题扫描,最差 vantablack 3.08;降到 75% 即 2.85 不达标)。
调这个值前重跑一次全主题扫描。

## 每主题块必须完备(45 token)

23 语义/终端 token(`--bg/--surface/--surface-warm/--fg/--fg-2/--muted/--meta/--border/--border-soft/--accent/--accent-on/--accent-hover/--accent-active/--success/--warn/--danger/--elev-raised/--focus-ring/--scrollbar-thumb(--hover)/--term-bg/--term-fg/--term-selection`)+ 16 ANSI token + `--chart-1..6` + `color-scheme`。块之间不允许交叉引用或"继承默认值"——每块独立完整,新增主题不能依赖其他块的值。

## 色值来源与映射(Omarchy 官方)

主题色板一律取自 basecamp/omarchy `themes/<name>/colors.toml`(任务 `research/omarchy-palettes/` 有存档),语义→token 映射:

| token | colors.toml 来源 |
|---|---|
| `--bg`/`--surface`/`--surface-warm` | background / lighter_background / 色阶邻档 |
| `--fg`/`--fg-2`/`--muted` | foreground / light_foreground / muted(对比 < 4.5:1 时换更深层并在注释注明) |
| `--accent` | accent |
| `--success/--warn/--danger` | green / yellow / red(文本用途需 ≥4.5:1,可同色相压深) |
| `--term-bg/--term-fg/--term-selection` | background / foreground / selection |
| ANSI 16 | Omarchy `omarchy-theme-color` 官方映射:**color0=background, color7=foreground, color8=muted, color15=bright_foreground, 1-6=red/green/yellow/blue/magenta/cyan, 9-14=bright_***(toml 缺省时 Omarchy 以混白 20% 推导) |

ANSI 与 `--term-*` 一律最终 hex(xterm WebGL 对 oklch/color-mix 解析不可靠)。

## 图表色板铁律

`--chart-1..6` 必须用 dataviz 校验器(bundled skill `dataviz/scripts/validate_palette.js`)对**本主题 surface** 跑六项检查:暗主题 `--mode dark`(L 带 0.48-0.67),亮主题 `--mode light`(0.43-0.77)。FAIL 同色相 snap-to-passing 复验;对比度 WARN 允许(Usage 页有文字标签+明细表 relief)。校验输出存档到任务 research/,CSS 块头注释记录结论。官方 pastel 原色几乎必 FAIL(色度/亮度带),微调是预期流程不是偏离。

## 新增主题(AC6 程序)

1. `styles.css` 加一个完备 `[data-theme]` 块(45 token,色值来自 Omarchy colors.toml,图表色过校验);
2. `themes.ts` `THEMES` 加一项(key/mode/swatch 5 色 hex 需与块值一致);
3. `i18n.ts` 双语词条;
4. **同步 `index.html` 预渲染脚本的 SCHEME_MODES 映射**(内联脚本无法 import 注册表,是手工镜像,两处必须逐项一致);
5. 组件零改动。选择器菜单自动适应任意数量(定位钳位按 `THEMES.length` 估算 + `.theme-menu` max-height 滚动)——不要在任何菜单里硬编码项数/高度。

## 常见错误

- **在 `:root` 或组件里写配色值** → 破坏多主题;唯一合法位置是主题块。(引用主题
  token 的环/阴影形状是例外,见上)
- **删/改 token 后不跑悬空引用审计** → 22 处引用集体失效且零报错(09-20 事故:
  `--elev-ring` 被删,勾选框方框直接看不见)。见「悬空 `var()` 是系统性风险」。
- **index.html SCHEME_MODES 与 themes.ts 失同步** → 刷新闪错主题(预渲染脚本是注册表的手工镜像)。
- **swatch hex 与块 token 漂移** → 选择器预览失真(无编译期保护,靠 review)。
- **菜单/弹层按固定项数定位** → 主题变多即溢出视口(已修过一次:8 主题时第 8 项要缩放页面才能点到)。
- **chart 色不跑校验直接上** → CVD/对比度退化不可目测发现。

## 回退兼容

`themes.ts::resolveThemeKey`:已退役 key(`"light"/"dark"/"kumo-*"/"catppuccin-mocha"`)与未知/空值一律回落 `DEFAULT_THEME`(tokyo-night)。退役主题不需要在 CSS 留块。
