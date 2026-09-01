# 模型配置页二期——pi-web 表单流 + cc-switch 多配置项 + 用量修复

> 父任务。子任务:provider-form-piweb / agent-tabs-live-config / agent-multi-preset /
> usage-correctness。承接已归档任务 08-26-models-config-redesign(R1-R4 已落地)。
> 参考:pi-web(github.com/agegr/pi-web)、cc-switch(github.com/farion1231/cc-switch)、
> models.dev(已验证沙箱可达 200/2.5s)。调研存档见
> `.trellis/tasks/archive/2026-08/08-26-unified-model-config/research/`。

## 用户需求（原话）

1. 新增供应商那里的信息填写改为跟 pi-web 填写模型一致：
   - 界面窗口布局
   - 模型配置填写：首先是供应商（填写完成信息后可以拉取模型列表来选择接入的模型），
     然后是所选择接入的模型的配置，可以拉取 models.dev 的模型信息一键填写，或者手动
     填写等，以及高级设置
2. 对于 pi agent 来说，配置模型页面需要能看到现有的模型配置，同时做管理，以及在这里
   复用统一供应商，需要调出该供应商的模型列表提供选择
3. 对于 opencode，跟 pi 类似
4. 对于 codex 和 claude，由于一次只能配置一系列的模型，比如 claude-opus、claude-sonnet 等，
   所以需要像 cc-switch 一样提供多个配置项，能在该页面自由选择（有点类似供应商页）
5. 用量统计这里效果不错，但一定确保数据是正确的，尤其缓存等信息，还有就是 web 页面中
   出现了字段不对齐等问题，需要修复

## 需求到子任务的映射

| 需求 | 子任务 | 一句话 |
| --- | --- | --- |
| R1 pi-web 表单流 + models.dev | `provider-form-piweb` | 供应商新增/编辑改 pi-web 布局:基本信息→拉取模型→勾选接入→逐模型配置(models.dev 一键填充/手动/高级) |
| R2 pi agent 现有配置管理 + 复用供应商 | `agent-tabs-live-config`(pi 侧) | pi 页签显示 `~/.pi/agent/models.json` 现有配置并管理;分配时调出供应商模型列表选择 |
| R3 opencode 跟 pi 类似 | `agent-tabs-live-config`(opencode 侧) | opencode 页签显示 `~/.config/opencode/opencode.jsonc` 现有配置并管理;同款供应商模型列表选择 |
| R4 claude/codex 多配置项 | `agent-multi-preset` | canonical `agents.{claude,codex}` 改为 `{presets[], current}`;UI cc-switch 式卡片列表 + 一键切换当前项;渲染器写当前 preset |
| R5 用量数据正确性 + 对齐 | `usage-correctness` | 核查各 agent 源 cacheRead/cacheWrite/cost 归账;修复明细表字段不对齐 |

`agent-tabs-live-config` 合并承担 R2+R3(pi 与 opencode 改造模式同构,共用「live 回读 + 供应商模型列表选择」组件)。

## 跨子任务验收（父级把关）

- [x] R1:新增供应商走 pi-web 式流程(基本信息→fetch 模型→勾选→逐模型配置);models.dev
  元数据填充可用(沙箱可达时增强,不可达时降级为手动,不报错)。
  (08-27-provider-form-piweb 已落地,commit 85dff57;ModelRow 折叠/展开替代
  旧 13 列横表;GET /api/models/catalog 1h 缓存;顺带修复 pi cost 单位 bug;
  ModelPicker 组件已备好供 R2/R3;容器截图实测通过)
- [x] R2:pi 页签展示 pi 现有 models.json 的 provider/model(不只一行 readback);可在此编辑
  /删除现有项;分配时从所选供应商的 `models[]` 列表选择,而非手填。
  (08-27-agent-tabs-live-config:LiveProviderList 行级同步/编辑/删除 + ModelPicker
  接线;live 回读 providers[] 摘要双文件独立容错;容器实测对账一致)
- [x] R3:opencode 页签同 R2(目标 `~/.config/opencode/opencode.jsonc`)。
  (同上任务:json5 容错读 + api 由 npm 反推 + 编辑逆映射回 npm 包;容器实测通过)
- [x] R4:claude/codex 页签为 cc-switch 式多 preset 卡片列表,含「当前」徽标 + 一键切换;
  canonical schema 迁移后旧 models.json 单 assignment 可正常读(转成单 preset 默认项);
  apply 写当前 preset 的结果与之前等价。(08-27-agent-multi-preset 已落地,
  commit a773316;model-config-guide.md 的 agents preset 段已同步)
- [x] R5:usage 的 cacheRead/cacheWrite/cost 与各 agent 原始日志对账一致(抽样手算核对);
  明细表/汇总表字段对齐修复(列宽、表头与单元格对齐、数字列右对齐)。
  (08-27-usage-correctness 已落地,commit 1b06f9c;opencode+pi 各 3 行手算对账
  一致;成本补算 $/M 约定 + 零值行过滤 + cache 拆列;spec usage 段已同步)
- [x] 集成:四子任务全绿后,父级跑容器实测全链路(供应商新增 pi-web 流 → pi/opencode
  复用 → claude/codex 多 preset 切换 → apply → usage 对账)。
  (08-27 父级容器实测一条链走通,全会话结束逐字节复原、md5 全等;明细见
  journal "Models 二期集成" 段。实测记录:
  R1:POST /api/models/discover(容器内 mock /v1/models)拉取 models[] → GET
    /api/models/catalog(models.dev 200,二次调用 6ms 命中 1h 缓存)→ PUT 存
    integ-mock 供应商 → GET 回读 mask key + models 在;
  R2:agents.pi 复用 integ-mock 保存 → POST apply/pi → ~/.pi 双文件:
    models.json 新增 integ-mock 节点且既有 3 节点保留 + settings.json 默认切
    mock-eagle-4(既有 keys 全保留)→ GET actors pi.live 反映;
  R3:agents.opencode 同款 → apply → opencode.jsonc provider 块 npm 逆映射
    @ai-sdk/openai-compatible、既有块保留、默认切 mock-falcon-mini;
  R4:claude/codex 各增 preset(后端 backfill id)→ 悬空 current PUT 400 →
    切 current → apply → ~/.claude/settings.json env(AUTH_TOKEN/BASE_URL/
    MODEL/HAIKU)与 ~/.codex/{config.toml,auth.json} 随 preset 变;
  R5:usage?window=all&refresh=1,pi(deepseek-v4-pro)+opencode(provider-1/
    deepseek-v4-flash)tokens 从原始源手算与 API 精确一致;opencode cost
    补算 7776/1e6*0.14+10/1e6*0.28=0.00109144 精确 PASS)
- [x] spec:更新 `.trellis/spec/backend/model-config-guide.md`(agents schema 改 preset、
  models.dev catalog 路由、usage cache 归账修订);前端 spec 同步 panes/models 结构变更。
  (各子任务随做随更:R4 preset 段+迁移、R1 catalog 段、R5 cost backfill 段、
  R2/R3 live 管理段+前端 LiveProviderList/ModelPicker 条目)

## 集成与顺序（建议）

1. `usage-correctness`(R5)独立、低耦合,可先行并行;也最先暴露用量真实基线,便于后续对账。
2. `agent-multi-preset`(R4)改 canonical schema,是后端 breaking 改动,先落 schema+迁移+
   渲染器+测试,再上前端 preset UI。
3. `provider-form-piweb`(R1)与 `agent-tabs-live-config`(R2/R3)共享「供应商模型列表选择」
   组件,宜在 R1 落地该组件后 R2/R3 复用;或 R1/R2/R3 同步设计组件契约。
4. R1 依赖一个后端 `models.dev catalog` 代理端点(避免前端直连外网 + 缓存)。
5. 父级集成验证收尾,spec + journal + commit。

顺序非强制,子任务可并行推进;强约束是 R4 的 schema 迁移要先于其前端、R1 的选择组件先于 R2/R3 复用。

## 范围护栏

- 只动统一模型配置页及其后端路由组。pi-web / cc-switch 本体、其他 pane、页脚、布局持久化一律不碰。
- 仍然不引入 chart 库(用量页沿用上一期的手写 SVG/div 图表)。
- models.dev 作为**可选增强**:沙箱网络可达时填充元数据,不可达时静默降级,绝不阻断主流程(与 pi-web 一致)。
- claude/codex preset 仍**派生**自统一供应商库(preset 引用 providerId + model + 覆盖项),不复制
  settingsConfig blob——保留「供应商库 SSOT + 每 agent 分配」范式(见 cc-switch 调研 §6 取舍)。
- 数据模型迁移一律向后兼容:旧 models.json 单 assignment 反序列化时转成单 preset,不报错、不丢数据。

## 研究引用

- pi-web 表单流/discover/models.dev catalog:`research/pi-web-model-config.md`
- cc-switch 切换式 vs 增量式 / preset / backfill:`research/cc-switch-config-model.md`
- 各 agent 用量源 + cache 字段:`research/aio-integration-and-usage-sources.md` + `app/src/routes/models/usage.rs`
