# Implement - claude/codex 多配置项(cc-switch 式 preset)

> 前置:读 `prd.md`、`design.md` §1-§7。改动顺序:后端 schema+迁移 -> 渲染器 ->
> 后端测试绿 -> 前端 types -> 前端 PresetList + handlers -> 视觉/i18n -> 容器实测 -> 收尾。

## 1. 后端 schema + 迁移(store.rs)

- [ ] `ClaudePresets`/`ClaudePreset` 替换 `ClaudeAssignment`;`CodexPresets`/`CodexPreset`
  替换 `CodexAssignment`;`AgentsConfig` 字段类型改 `Option<ClaudePresets>`/`Option<CodexPresets>`。
- [ ] `ClaudePresetsShadow`/`CodexPresetsShadow` + `#[serde(from = "…Shadow")]` + `From` impl
  (旧单 assignment -> 单 default preset;见 design §1)。
- [ ] preset id 生成:`gen_preset_id()`(kebab + 短随机);PUT 时为无 id 的新 preset 补 id,
  撞名重生成(同 agent 域内唯一)。
- [ ] `validate`:遍历 claude/codex 每个 preset 校验 provider/model 存在;错误带 preset name。
- [ ] 删除当前 preset 的顺移:`delete_preset` 语义在 PUT(整份 canonical)时由前端构造新
  presets/current;后端只 validate + 持久化(顺移逻辑放前端,后端单测 validate 不变)。
  **若发现后端更合适再调整,记录于本文件。**

## 2. 渲染器(render/claude.rs, render/codex.rs)

- [ ] `apply_claude`:取 current preset(见 design §3);找不到/None -> push_err,不写半截。
- [ ] `apply_codex` 同构。
- [ ] 文件头注释更新(assignment -> current preset)。

## 3. 后端测试

- [ ] store 迁移单测:旧形状/新形状/空/presets 空但旧 provider 在/缺 model。
- [ ] claude renderer:多 preset apply current;current 不存在 -> err;current None -> err。
- [ ] codex renderer 同构。
- [ ] validate:非当前 preset 引用未知 provider -> PUT 报错带 name。
- [ ] `cargo test models` 全绿(throwaway rust 容器跑:`docker run --rm -v $PWD/app:/app
  -v aio-cargo-registry:/usr/local/cargo/registry -w /app rust:1-bookworm cargo test`)。

## 4. 前端 types(types.ts)

- [ ] `ClaudePresets`/`ClaudePreset`/`CodexPresets`/`CodexPreset` 接口 + 解码器;`AgentAssignment`
  仍服务 pi/opencode;`CanonicalConfig.agents.claude|codex` 改类型。
- [ ] `incompatibleReason`/`liveReadbackText` 适配 preset(取 current 的 provider/model 判断)。

## 5. 前端 PresetList + handlers(AgentTabs.tsx / 新 PresetList.tsx)

- [ ] claude/codex 页签分叉渲染 `PresetList`:卡片(名称/模型/协议徽标/当前徽标)+ 行操作
  (设为当前[主]/编辑/复制/删除)。
- [ ] 新增/编辑表单:provider select(兼容过滤)-> 模型从该供应商 models[] 选(ModelPicker
  复用,无则手填降级)-> 覆盖项(claude 三档 + authField / codex effort + wireApi)。
- [ ] `ModelsPane.tsx` handler:`addPreset/updatePreset/deletePreset/duplicatePreset/setCurrentPreset`,
  全走现有 PUT canonical 通道;switch = setCurrent + save + apply。
- [ ] 复制:新 id 后端补、name 加「副本」后缀插源后;删除当前顺移到首项。
- [ ] agent dirty / saveMsg / applyResult 面板沿用。

## 6. 视觉 + i18n

- [ ] PresetList 走 Kumo 卡片样式(ml-card 衍生,不嵌套 LayerCard);当前徽标语义色;
  focus ring/disabled/loading 齐;reduced-motion 尊重。
- [ ] i18n 新键(zh+en):mcPreset/maCurrent/maSetCurrent/maDuplicate/maDeletePresetConfirm/
  maNewPreset/maDefaultPreset/maNoCurrentPreset 等。

## 7. 验证(质量门)

- [ ] `cargo test models` 全绿。
- [ ] `cd web && npm run build` 干净(tsc 严格 + vite)。
- [ ] 容器实测:app 用最新镜像重建+重建 sidecar(见 compose 注释)-> 新增 2 preset ->
  切换 current -> apply -> 查 `~/.claude/settings.json` / `~/.codex/config.toml` 内容随 preset 变;
  删除当前 preset -> 顺移;旧单 assignment models.json 可读(转单 preset)。
- [ ] 对照 prd AC 逐项打勾;`trellis-check` 复查。

## 8. 收尾

- [ ] 更新 `.trellis/spec/backend/model-config-guide.md`:agents schema 改 preset(claude/codex),
  渲染器取 current;前端 spec 同步 PresetList。
- [ ] journal-1.md 当日进展。
- [ ] commit(`feat: claude/codex 多配置项 preset`);归档子任务;回父任务标记子完成。
