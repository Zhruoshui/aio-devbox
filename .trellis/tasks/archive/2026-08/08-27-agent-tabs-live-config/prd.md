# pi/opencode 页签:现有配置管理 + 供应商模型列表复用

> 父任务 08-27-models-config-v2 的 R2+R3。pi 与 opencode 同为「增量式」agent
> (全部 provider 共存于各自原生配置文件),改造模式同构,合并为一个子任务。

## Goal

pi / opencode 页签从「单行 readback + 手填 model id」升级为:展示该 agent 原生配置文件里的
**现有模型配置**并可管理;分配时**复用统一供应商库**,调出该供应商的模型列表选择。

## Requirements

### live 配置可视化(pi)

- 后端扩展 `GET /api/models/agents` 的 live 回读:pi 返回 `~/.pi/agent/models.json` 的
  providers 摘要(id/name/api/baseUrl/模型数)+ settings.json 的 defaultProvider/defaultModel。
- pi 页签渲染「现有配置」列表:每 provider 一行(名称/协议/模型数/是否当前默认),可展开看
  模型;提供「同步到供应商库」(把 pi 侧手改吸收进 canonical,复用 import 的 1:1 映射逻辑)。
- 管理:编辑/删除 pi 侧 provider 节点(写回 pi 的 models.json,键级合并,不动其他键)。

### live 配置可视化(opencode)

- 同构:live 回读 `~/.config/opencode/opencode.jsonc` 的 `provider` 对象(解析后摘要)+
  顶层 `model`;页签渲染现有 provider 列表;「同步到供应商库」;编辑/删除写回(json5 容错,
  键级合并)。

### 分配交互(pi + opencode 共用)

- 选供应商后,模型下拉改为**该供应商 `models[]` 列表**(id + 显示名),不再手填;
  复用 R1 的 ModelPicker 组件。
- 保存/生效/写入结果面板(written/backup/errors)保留。
- 未安装 agent 时:live 区显示「未安装——预写模式」提示,分配照常可用(现有行为保留)。

## Acceptance Criteria

- [ ] pi 页签:live 列表与 `~/.pi/agent/models.json` 实际内容一致(容器实测抽样核对);
  编辑/删除写回且不破坏文件其他键;「同步到供应商库」幂等(已存在跳过)。
- [ ] opencode 页签:同上,对 `opencode.jsonc`(含 json5 注释文件)读写无损。
- [ ] 分配模型:从供应商模型列表选择;选择后保存→apply→live 回读反映变更。
- [ ] 未安装 agent 时页签不报错,预写行为不回退。
- [ ] `npm run build` 干净;后端 live 回读/写回单测绿(pi + opencode 各覆盖:正常/文件缺失/
  损坏/json5 注释)。

## Notes

- 前置:R1 的 ModelPicker 组件(若 R1 未先落地,先在本任务内实现并回供 R1 使用,口径一致即可)。
- 「同步到供应商库」复用 store.rs `import_from_pi` 的映射;opencode 需要一个反向适配
  (opencode provider 片段 → canonical ProviderEntry,`options.baseURL/apiKey` → baseUrl/apiKey)。
- live 编辑写回统一走 render/pi.rs 与 render/opencode.rs 的键级合并通道,不新开写文件路径。
