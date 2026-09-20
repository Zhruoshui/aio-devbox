# Implement — 多主题配色 + xterm 适配(Omarchy 官方主题集修订版)

> 09-20 修订:第一阶段(kumo + 4 主题 + ANSI)已实现并通过检查;现按用户确认的最终主题集重整——**删除 Kumo 两套,补 Ethereal/Vantablack/White/Flexoki Light,全部 8 套对齐 Omarchy 官方 colors.toml**(存档 `research/omarchy-palettes/*.toml`)。

前置:design.md 已定架构(CSS 变量块 + data-theme/data-mode 双属性)。执行顺序按依赖排列;每步末尾有验证命令。

## 0. 重整:Kumo 退役 + 8 主题对齐(修订核心)

- [ ] styles.css:`:root` 只保留模式无关原语(字体/间距/圆角/动效/elev 形状/焦点环形状等),删除全部配色 token(oklch/color-mix 定义全部移除);8 个 `[data-theme]` 块各自完备(配色 + ANSI + chart + color-scheme);每块注释标注 Omarchy colors.toml 来源
- [ ] token 值映射(以 Omarchy 语义→我们的 token):
  bg=background / surface=lighter_background / surface-warm=dark_background(暗)或 background 附近提亮(亮,参考 colors.toml 实际值) / fg=foreground / fg-2=light_foreground / muted=muted 或 dark_foreground / border/border-soft 从 background/foreground 派生色阶 / accent=accent / success=green / warn=yellow / danger=red / term-bg=background / term-fg=foreground / term-selection=selection
- [ ] ANSI 16 色映射按 Omarchy 官方 omarchy-theme-color 规则:color0=background、color7=foreground、color8=muted、color15=bright_foreground、1-6=red/green/yellow/blue/magenta/cyan、9-14=bright_*(colors.toml 语义名直接对应)
- [ ] 图表色板:4 个已校验主题(tokyo-night/nord/catppuccin 即 mocha/catppuccin-latte)沿用现值;ethereal/vantablack/white/flexoki-light 走 dataviz 校验流程(暗 4/L 0.48-0.67,亮 2/L 0.43-0.77,surface 用各自 --surface 值),结果追加到 research/palette-validation.md
- [ ] themes.ts:THEMES 换 8 套(key: tokyo-night/catppuccin/ethereal/nord/vantablack/catppuccin-latte/white/flexoki-light),默认 tokyo-night;resolveThemeKey 旧值(light/dark/kumo-*/catppuccin-mocha)→ tokyo-night;swatch 用各主题 accent/fg/bg/chart-1/chart-2
- [ ] index.html:静态默认 data-theme="tokyo-night" data-mode="dark";预渲染脚本 8 键映射同步更新
- [ ] i18n:8 主题词条,删 kumo 两条
- [ ] charts.tsx:检查 KUMO_CATEGORICAL 命名可保留(仍是调色板常量名),值不变(var 引用)

## 1. 硬编码色排查(基线)

- [ ] `grep -n "#[0-9a-fA-F]\{3,8\}\b\|rgb(" mgr-web/src/components.css gl-kumo.css mgr-web/src/pages/**/*.tsx mgr-web/src/components/**/*.tsx`(排除 chart.tsx 的 var() 引用)
- [ ] 产出残留清单;逐个改走语义 token
- 验证:`npm run build`

## 2. 主题 token 块(styles.css)

- [ ] 各主题整理官方色板(Tokyo Night / Nord / Catppuccin Mocha / Latte),映射到语义 token 全集(design §1 列表)+ ANSI 16 色
- [ ] Kumo light/dark 块补 ANSI 16 色 token(派生自 Kumo 色板)
- [ ] 新增 `[data-theme="..."]` 四块;每块头注释记录色板来源
- 验证:浏览器/devtools 手动 `document.documentElement.dataset.theme = "nord"` 无未定义变量(getComputedStyle 抽查 --accent 等)

## 3. 图表色板校验(dataviz 铁律)

- [ ] 每主题选 6 候选色,跑 `node <dataviz>/scripts/validate_palette.js "<hex,...>" --mode <light|dark> --surface <surface hex>`
- [ ] FAIL → 同色相 snap-to-passing → 复验至全 PASS(对比 WARN 记录注释)
- [ ] 通过值写入各主题块 `--chart-1..6`,注释记录校验结论
- 验证:校验器输出存档到任务 research/palette-validation.md

## 4. themes.ts 注册表 + App.tsx 选择器

- [ ] `themes.ts`:`THEMES: ThemeDef[]`(key/i18n label/mode/swatch 5 色)导出;`resolveThemeKey(saved)` 兼容旧 "light"/"dark"
- [ ] App.tsx:state 改 scheme key;useEffect 写 `data-theme` + `data-mode`(mode 由 scheme 推导);顶栏按钮 → 下拉选择器(.theme-menu;色点 + 名称 + 勾选)
- [ ] i18n 词条(kumoLight/kumoDark/tokyoNight/nord/catppuccinMocha/catppuccinLatte)
- 验证:`npm run build`;浏览器切换 6 主题无刷新闪白,刷新后保持

## 5. xterm ANSI 适配(XtermPane.tsx)

- [ ] `readTermTheme()` 读全部 16 ANSI token(+ 现有 bg/fg/cursor/selection)
- [ ] modeObserver attributeFilter 加 `data-theme`
- 验证:终端 `ls --color=always`、`git diff`、`echo -e "\e[31mred\e[0m ..."` 全 16 色环;切主题即时重着色(不重连)

## 6. 全页面 6 主题目检 + 收尾

- [ ] 工作区(xterm 面板、布局 chrome)、沙箱列表/编辑/镜像/Usage 逐主题过一遍
- [ ] `npm run build` 最终门禁
- [ ] 重建 mgr 镜像部署验证(mgr 栈)

## 回滚点

每步一个 commit;主题块纯增量(Kumo 默认不动),回滚 = revert 对应 commit,无数据迁移。
