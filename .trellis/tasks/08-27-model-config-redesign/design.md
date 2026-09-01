# 设计 —— 模型配置页按设计稿重设计

## 目标与边界

纯前端视觉/结构重设计，把 `web/src/panes/models/` 按 `docs/open-design/screens_model-config.html`
（Kumo 单页设计稿）的视觉语言重构。**不改** `/api/models/*` 后端契约、**不增删**功能、
**不动** golden-layout 外壳（App.tsx / Sidebar / Statusbar）。设计稿是整页，实际是 pane，
故按用户已确认的两个决定适配：
- **不复刻** crumb 面包屑栏与语言切换（全局已有）。
- 抽屉 = **pane 内 scrim + absolute 抽屉**（`position:absolute`，遮罩只盖 `.pane-models`）。

## Token 与命名策略

- 应用的 `:root` / `[data-mode="dark"]` token 与设计稿完全同源（Kumo），直接复用，
  不新增 token；仅按需补齐设计稿用到的语义 helper（如 `--chart-cat-*` 已在 charts.tsx
  硬编码同值，保持）。
- 样式一律放 `web/src/styles.css` 的 models 区块，沿用现有 `ml-*` 前缀类名（与全库一致），
  但视觉值从设计稿对应规则抄写。设计稿的通用类名（`.card/.drawer/.field/.btn`）与 app
  全局冲突时**不**引入裸类，映射到 `ml-*`。
- 组件层面沿用现有文件结构（ProviderGrid/ProviderEditor/ModelRow/AgentTabs/PresetList/
  UsageTab/ModelsPane），不改数据流，只改 render 结构 + className + 少量本地 state。

## 设计稿 → React 映射

### 1. 主 Tab（`.ml-tabs`）
设计 `.tabs-primary`：圆角容器（radius-lg）+ padding 3px + gap 2px + bg surface-warm；
`.tab-item` 高 34px、radius-md、muted；选中 = surface + elev-ring + fg + 500。
改动：`.ml-tabs` 容器加 padding/border-radius/bg；`.ml-tab` 高 34px。
保留现有 `flex-shrink:0`（Tab 固定、body 滚动）。

### 2. 供应商页标题（`.ml-sec-head`）
设计 `.sec-head`：h2（text-xl 600）+ p 副标题（text-sm muted）+ 右侧 `.sec-actions`
（「从 pi 导入」btn-secondary +「新增供应商」btn-primary 带 plus 图标）。
改动：ModelsPane providers 分支把 `.ml-toolbar` 换成 `.ml-sec-head` 结构；
空态沿用 `.ml-empty`（标题/副标题/双按钮）。

### 3. 供应商卡片（`.ml-card`）
设计 `.provider-card`：
- 协议徽章独占一行（badge 上、名称下）；名称 `.pc-name` text-base 600。
- 工具（编辑/删除 icon-btn）右上，hover/focus-within 浮现。
- `.pc-url` mono muted 单行省略。
- `.pc-meta`：`{n} 个模型` + `·` + 脱敏 key（`maskKey`：`sk-xxxx •••• xxxx`，
  空则 `—`）。后端已返回脱敏 key，前端仅需格式化（6 头 + •••• + 4 尾）或直接展示。
- `.pc-chips`：顶部分隔线（border-top border-soft）+ padding-top；空显示「未被任何 agent 绑定」。
- hover：bg 微调（surface 92% + fg）。
改动：ProviderGrid 的 head 结构（badge 上/名在下）、meta 加 `·` 分隔、chips 加 border-top。
grid 改 `repeat(auto-fill, minmax(300px, 1fr))` + gap space-4。

### 4. 抽屉 + scrim（`.ml-drawer` / `.ml-scrim`）
设计 `.scrim`（fixed inset0，bg 60% 遮罩，opacity 过渡）+ `.drawer`（fixed 右全高，
width min(460px,100vw)，translateX(100%)→0）。
pane 版：`.pane-models` 加 `position:relative`；`.ml-scrim` absolute inset0 z 低于抽屉、
`.open` 淡入；`.ml-drawer` 改 absolute（top/right/bottom 0、宽 min(460px,94%)）、
`.open` 时 translateX 过渡（替换当前 keyframe 动画）。Escape 关闭 + scrim 点击关闭。
- 分组 `.ml-dgroup`：h3（text-sm 600 muted）小节标题；高级设置用 `.ml-dgroup-toggle`
  （chevron-down 旋转收起）。
- 底部 `.ml-drawer-savebar`：左「删除」**btn-danger-text** + spacer + 取消 btn-secondary + 保存 btn-primary；
  新增取消按钮（关闭抽屉）；保存 disabled 逻辑不变。

### 5. 模型行（`.ml-model-row`）
- 折叠头：chevron + mono id（保留当前可编辑 input，但样式向设计 mono 文本靠拢：
  透明底、无边框→focus 显示框）+ 名称（muted）+ **推理 info 徽章**（accent 底 + dot）+
  成本 `$i / $o`（mono）+ 动作（`test-pill` + trash icon-btn）。
- **test-pill**：24px 胶囊按钮；play 图标；testing = 左部 10px spinner +「检测中…」；
  ok = 成功色底 + `✓ {ms}ms`；fail = 危险色底 + `✕ {错误}`。
- 展开体改设计排布：`field-row`（显示名 + 协议覆盖 2 列并排）、`field-row-3`
  （上下文窗口 / 最大输出 / 推理 checkbox 内联）、`field-row-4`（in/out/cacheR/cacheW）、
  底部「从 models.dev 填充」ghost 按钮。
- 现有 add/delete/fill/cost 回调全保留。

### 6. Discover 弹窗
设计 `.modal`：head（h2「从端点拉取模型」+ mono endpoint）+ 搜索框（带放大镜 icon）
+ 列表行（checkbox + id + 名称 + 「已存在」tag）+ foot（全选/清空 + `N 已选` +
取消 btn-secondary +「添加所选」btn-primary）。
改动：`.ml-discover` 头部补 endpoint 副标题、搜索框加 icon、底部加取消按钮与已选计数文案。

### 7. 增量 agent 页（pi/opencode，`.ml-agent`）
- `.ml-agent-head`：**agent 名称（text-2xl 600）** + 已安装徽章 + 一致/不一致徽章。
- `.ml-paradigm-strip`：surface-warm 圆角条 + cube 图标 + 增量式说明文案。
- 表单卡片化：`.ml-form-card`（surface + ring + raised + padding space-5）包裹
  Live 回读卡 / 分配卡（provider 下拉 + ModelPicker）/ 保存条 / 结果面板 / 原生配置列表。
- `.ml-savebar`：surface-warm 圆角条；dirty 圆点 + 「未保存」；msg；spacer；
  保存 btn-secondary + 应用 btn-primary。
- 原生配置行 `.ml-native-row`：名称 + url + synced 徽章 + 同步/编辑/删除。

### 8. 切换式 agent 预设（claude/codex，`.ml-preset-list`）
- agent-head（同增量）+ paradigm-strip（切换式文案）+ `.sec-head`（预设标题 + 新增预设按钮）。
- `.ml-preset-card`：current 用 **accent 1.5px 描边**（`0 0 0 1.5px var(--accent)`）替代徽章；
  preset-top = 协议徽章 + 名称（+ current 徽章）+ meta（`provider → model` mono）+
  动作（「设为当前」btn-primary btn-sm、编辑/复制 icon-btn、删除 danger icon-btn）。
- 无 current 时 `.warn-strip` 警告条。
- 编辑内联 `.ml-preset-form`（现有表单字段保留）；取消/保存按钮。

### 9. 用量页（`.ml-usage`）
- `.ml-usage-toolbar`：h2「用量统计」+ `.ml-window-switch`（胶囊分段 today/7d/all）+
  刷新 **icon-btn**（替换带文字按钮）+ gen-at mono 右对齐。
- `.stat-grid`：4 张 stat 卡（label text-xs + 值 mono text-2xl 600）——现有 `.ml-stats`
  已是此形态，仅微调。
- `.chart-row`：`1.4fr 1fr` 两栏（条形卡 + 环形卡）——现有 `.ml-charts` auto-fit，
  改为固定两栏（<900px 单栏）。条形行对齐设计（label 130px + track + val 74px）。
- 明细表保持（cacheR/cacheW 分列 + 合计），现有已达标，不改。

## i18n 新增 key（zh/en 双份）

| key | zh | en |
|---|---|---|
| mcProvidersSub | 统一管理各 agent 使用的 API 供应商、密钥与模型目录。 | Manage API providers, keys, and model catalogs shared across agents. |
| maParadigmIncremental | 增量式 agent —— 单一「供应商 + 模型」绑定，通过下拉直接选择。 | Incremental agent — one provider + model binding, picked from dropdowns. |
| maParadigmSwitcher | 切换式 agent —— 维护多个命名预设，恰好一个当前生效（cc-switch 风格）。 | Switcher agent — multiple named presets, exactly one current (cc-switch style). |
| mcProvidersHeading（若不复用 mcProviders） | 供应商库 | Providers |
| mcUsageHeading（若不复用 mcUsage） | 用量统计 | Usage |
| mcSelectedCount | 已选 | selected |

## 兼容性与风险

- 纯 CSS/JSX 改动；`tsc --noEmit && vite build` 是构建门（spec frontend）。
- 现有功能全部保留：live sync/edit/delete、catalog fill、discover、preset CRUD、
  用量聚合、协议兼容过滤、ModelPicker。
- 抽屉改 scrim+translateX 后，需回归：编辑→保存→关闭、Esc、scrim 点击、删除确认。
- 卡片/抽屉在窄 pane（<500px）下：grid 单列、drawer 全宽（`width:100vw`? 不——
  pane 版 width:min(460px,100%)，窄 pane 自然贴合）。
- 回滚：styles.css 与各组件均为独立提交内改动，git revert 单提交即可回滚。

## 不做

- 不引设计稿整页 HTML 的 crumb/lang-toggle 结构。
- 不引入新依赖（图表仍零依赖）。
- 不改后端（含 masked key 格式）。
