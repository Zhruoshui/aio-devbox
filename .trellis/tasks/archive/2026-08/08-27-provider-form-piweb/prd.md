# 供应商表单 pi-web 流 + models.dev 集成

> 父任务 08-27-models-config-v2 的 R1。参考 pi-web `components/ModelsConfig.tsx` 表单流
> (调研:`.trellis/tasks/archive/2026-08/08-26-unified-model-config/research/pi-web-model-config.md`)。

## Goal

新增/编辑供应商改为 pi-web 式的「供应商 → 拉取模型 → 勾选接入 → 逐模型配置」流程,
逐模型配置支持 models.dev 元数据一键填充、手动填写、高级设置;编辑器布局向 pi-web 靠拢。

## Requirements

### 布局(界面窗口布局)

- **改造尺度(已定:只改抽屉内部)**:保留供应商库页签的卡片网格 + 右侧编辑抽屉;
  把抽屉**内部**重构为 pi-web 式流程。不弃卡片网格、不改整页为左树布局。
- 抽屉内 pi-web 式两区/分步结构:
  - **供应商信息区**(上/左):名称 / baseUrl / 协议 / apiKey(显隐) / 高级(headers+compat)。
  - **模型接入区**(下/右):「拉取模型列表」→ 可搜索勾选列表(沿用现有 discover 能力与
    多候选 URL)→「加入所选」后进入逐模型配置。
  - 逐模型配置从「一张横向大表」改为**逐模型行/卡片展开**:点某模型展开配置区
    (显示名/reasoning/contextWindow/maxTokens/cost 四项/高级),pi-web 的模型详情面板形态。
- 保留 Kumo 视觉语言(ml-* token 类,不回退裸色)。

### models.dev 一键填充

- 后端新增 `GET /api/models/catalog`:代理 `https://models.dev/api.json`(15s 超时,进程内
  1h 缓存 + in-flight 去重,照 pi-web),返回归一化目录 + 按 provider baseUrl 主机名/id 的
  推荐预设。
- 模型配置区提供「从 models.dev 填充」按钮:命中时一键填 name/reasoning/input/
  contextWindow/maxTokens/cost;未命中或网络不可达时**静默降级**(按钮置灰+提示),手动填写
  不受影响。
- 不做前端直连外网;全部经后端代理。

### 高级设置

- 每模型高级:input 类型、thinkingLevelMap(按 pi schema 可选)、per-model 协议覆盖(已有)、
  compat(供应商级已有;模型级按 pi schema `compat` 不存在则不加)。
- 供应商高级:headers / compat JSON(已有,保留解析校验)。

## Acceptance Criteria

- [x] 新增供应商:填基本信息 → 拉取模型 → 搜索勾选 → 加入 → 逐模型展开配置,全流程不离开抽屉。
  (容器实测:discover/加入流程不变,ModelRow 折叠/展开在抽屉内验证通过)
- [x] models.dev 命中时一键填充 6 项元数据;未命中/离线时降级不报错、不阻断。
  (容器实测:deepseek provider 命中填充 name/contextWindow/maxTokens/cost 三项四子值;
  aruoshui-openai-completions 未命中 host 时显示"未在 models.dev 目录中找到匹配项",不报错)
- [x] `GET /api/models/catalog` 有缓存(重复请求不打外网)、失败返回 502 + 截断信息。
  (容器实测:首次调用抓真实 models.dev 数据,二次调用 6.5ms 命中缓存;502 路径单测覆盖)
- [x] 旧编辑能力全保留:手动加模型、per-model 协议、cost 手填、test 连通、删除。
  (ModelRow 展开态保留全部字段编辑;折叠态保留 test 按钮/pill + 删除)
- [x] Kumo 走查通过;`npm run build` 干净;后端 catalog 单测(缓存/超时/解析)绿。
  (视觉走查见容器截图;cargo test models 173 绿,含 6 个 catalog 新测试)
- [x] 「供应商模型列表选择」抽成可复用组件(ModelPicker),供 R2/R3 agent 页签复用。
  (`ModelPicker.tsx` 已实现为无状态纯 props 组件,R2/R3 待接线)

## Notes

- 依赖:无前置子任务;ModelPicker 组件是 R2/R3 的前置。
- models.dev 归一化参照 pi-web `flattenModelsDevCatalog`。**单位口径(已随 R5 更新)**:
  canonical cost 现定为 $/M(08-27-usage-correctness 决定),models.dev 原生也是
  $/M——两边一致,**不需要换算**。真正需要修的是 `render/pi.rs` 渲染到 pi 原生
  models.json 时的单位(pi schema 是 USD/token),见 design.md §0。
