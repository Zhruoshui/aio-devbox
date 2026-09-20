# Design — mgr-web 多主题配色 + xterm 适配

## 1. 架构:CSS 变量块 + 双属性驱动(不引入 CSS-in-JS / 主题库)

维持"token 层唯一真源"的现有契约,扩展而非替换:

```css
:root { /* Kumo light(默认亮色) */ }
[data-theme="dark"], [data-mode="dark"] { /* Kumo dark(现行为,兼容保留) */ }
[data-theme="tokyo-night"]       { /* 全 token 覆盖 */ }
[data-theme="nord"]              { ... }
[data-theme="catppuccin-mocha"]  { ... }
[data-theme="catppuccin-latte"]  { ... }
```

- `<html data-theme="<scheme>" data-mode="<light|dark>">` 双属性:`data-theme` 驱动具体方案;`data-mode` 保留给 `color-scheme`(UA 原生控件)及现有 `[data-mode="dark"]` 选择器兼容。每个 scheme 的 mode 是静态属性(如 tokyo-night 恒为 dark)。
- 切换 = 改一个属性,CSS 级联自动重着色,xterm 热切换沿用 XtermPane 现有 modeObserver(MutationObserver 已监听 data-mode;改为同时监听 data-theme)。
- 主题元数据(注册表)放 `src/themes.ts`:`{ key, labelI18nKey, mode, swatch: string[5] }`,UI 选择器与 App 状态消费;颜色值本体只在 CSS,TS 不存色值(单一真源)。

**为什么不动态注入 style**:CSS 块参与正常级联、无 FOUC、可被 DevTools 直接调试,与现有 `[data-theme="dark"]` 模式一致。

## 2. 主题选择器 UI

顶栏现有亮暗 `icon-btn` 升级为下拉(palette 图标 + 菜单):

- 复用 `.segmented`/`.chip` 的弹层样式惯例(新增 `.theme-menu` 小样式组,走 token)。
- 每项:色板缩略(5 个小色点:accent/fg/surface 采样)+ 名称(i18n)。
- 选中项打勾;点击即切,无"应用"按钮。
- localStorage `THEME_KEY` 语义从 `"light"|"dark"` 变为 scheme key;旧值("light"/"dark")映射为 `kumo-light`/`kumo-dark` 保证向后兼容(AC 要求未识别值回退 dark)。

## 3. xterm ANSI 16 色

### 3.1 token 约定

新增 16 个 ANSI token(每主题 16 值,hex,来自各主题官方 terminal 色板):

```css
--term-ansi-black / red / green / yellow / blue / magenta / cyan / white
--term-ansi-bright-black / ... / bright-white
```

Kumo light/dark 也补齐(现状用 xterm 默认色板,与 Kumo 视觉脱节):从 Kumo 色板派生一套(参照主流亮暗 ANSI 惯例)。

### 3.2 readTermTheme 扩展

```ts
const ANSI_KEYS = ["black","red","green","yellow","blue","magenta","cyan","white"] as const;
// ITheme 16 字段 + cursor/cursorAccent/selectionBackground
```

全部从 computed style 读,拼 bright 变体名(`--term-ansi-bright-black` → `brightBlack`)。**用 hex 存 ANSI token**:xterm WebGL 渲染器对 oklch/color-mix 的解析不可靠,官方色板本身即 hex,不引入转换。

### 3.3 热切换

XtermPane 现有 modeObserver 改为监听 `data-theme`(属性过滤器加 data-mode,data-theme 双保险),回调里 `term.options.theme = readTermTheme()` 不变。

## 4. 图表调色板(每主题一套 --chart-1..6)

- 来源:各主题官方色板中选 6 个高区分度色(Tokyo Night 的 blue/purple/cyan/orange/green/red 系;Nord 的 frost 系 + aurora 系;Catppuccin 的 accent 系)。
- **流程铁律**:候选色跑 dataviz `validate_palette.js --mode <light|dark> --surface <主题 surface hex>`;FAIL 者同色相压暗/提亮(snap-to-passing),复验至 PASS。对比度 WARN 允许(页面自带文字标签 + 明细表 relief)。
- 暗色主题按 dark 模式亮度带(L 0.48–0.67),Latte 按 light 带(L 0.43–0.77)。
- 结果以注释形式记录每个主题的校验结论(CSS 块头注释),与现有 Kumo 注释风格一致。

## 5. 边界与风险

| 风险 | 缓解 |
|---|---|
| gl-kumo.css / components.css 硬编码色 | 实现第一步先 grep 排查全部硬编码 hex/rgb,逐个 token 化(AC4) |
| `color-mix(in oklch, var(--accent)...)` 在新 accent 值下的表现 | 各主题 token 直接给最终色值,避免复杂 color-mix 派生;个别必要的派生在主题块内重定义 |
| WebGL 渲染器对 CSS 颜色串解析 | ANSI 全 hex;--term-bg/fg 若现值含 oklch,新主题直接 hex,Kumo 两套保持现值(已验证可用) |
| localStorage 旧值 | "light"→kumo-light、"dark"→kumo-dark 映射 |
| 图表在低对比主题下可读性 | dataviz 校验硬门 + WARN 需 relief(已有) |

## 6. 文件影响面

| 文件 | 变更 |
|---|---|
| `mgr-web/src/styles.css` | +4 主题 token 块(含 ANSI 16 色 + chart 6 色);Kumo 亮/暗补 ANSI token;.theme-menu 样式 |
| `mgr-web/src/themes.ts` | 新建:主题注册表 |
| `mgr-web/src/App.tsx` | Theme 状态改 scheme key;选择器下拉;data-theme 写入 |
| `mgr-web/src/i18n.ts` | 主题名称词条 |
| `mgr-web/src/pages/workspace/panes/XtermPane.tsx` | readTermTheme ANSI 扩展;observer 属性过滤 |
| `mgr-web/src/components.css` / `gl-kumo.css` | 仅硬编码色 token 化(如有) |
