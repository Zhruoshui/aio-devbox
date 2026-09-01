# 修复 pi models.json 渲染：cost 单位错误(÷1e6) + cacheWrite 必填缺失

## Goal

模型配置页 apply 后 pi 启动报 `Invalid models.json schema: providers.provider-1.models.N.cost.cacheWrite: must have required properties cacheWrite`，且 provider-1 的 cost 数值比 models.dev 原值小 1e6（如 deepseek-v4-flash input 写成 `1.4e-7`，应为 `0.14`）。根因是 `render_pi_cost`（`app/src/routes/models/render/pi.rs`）基于错误假设做单位换算 + 省略缺失字段。本任务修正渲染语义、同步注释与测试、重建部署并使线上 `~/.pi/agent/models.json` 痊愈。

## 根因（已对照 pi 本体源码 earendil-works/pi 验证）

1. **单位错误**：pi 原生 models.json 的 cost 是 **USD/1M tokens**（`packages/ai/src/models.ts:895`：`rates.cacheWrite * shortWrite ... / 1000000`；pi 自带生成器的值与 models.dev 完全同值）。`render_pi_cost` 注释断言"pi 是 $/token"并 ÷1e6，错误。
2. **必填缺失**：pi `ModelCostSchema`（`packages/coding-agent/src/core/model-config.ts:144-157`）中 `input/output/cacheRead/cacheWrite` 四字段全部必填（仅 `tiers` 可选）。models.dev 的 deepseek 系列只有 `cache_read` 无 `cache_write` → canonical 存 `None` → 渲染省略 → schema 校验失败。pi 自己的生成器语义是缺失补 `0`（`generate-models.ts:1044-1045`：`cacheRead: cost?.cache_read || 0`）。

## Requirements

- `render_pi_cost`：canonical 值直传（$/M），删除 ÷1e6 换算。
- cost 对象存在时四字段补齐：缺失字段写 `0`（对齐 pi 生成器语义）；cost 全字段皆 None 时整体省略 cost 对象（pi schema 中 cost 本身可选）。
- 同步修正注释：`render/pi.rs`（render_pi_cost 文档）、`catalog.rs:10-13`（对 render_pi_cost 的交叉引用）。
- 更新受影响测试（`model_cost_renders_camel_case` 等），新增"缺失字段补 0 / 全空省略"用例。
- 不改动 canonical 存储语义（store.rs）、usage 统计（usage.rs 自用 $/M 直算，无耦合）、前端 fill 逻辑。
- 不需要回写 08-27 归档设计文档（归档只读），但 spec 若有相关约定需更新（Phase 3.3 判断）。

## Acceptance Criteria

- [ ] `cargo test` 全绿（models 模块含新增用例：缺省补 0、全空省略、$/M 直传）。
- [ ] apply 后生成的 pi models.json：provider cost 为 $/M 原值（如 input 0.14），且含全部四个 cost 字段（cacheWrite 缺省为 0）。
- [ ] pi 侧 schema 校验通过：线上 `~/.pi/agent/models.json` 中 provider-1 三条模型 cost 痊愈（0.14/0.28/0.0028 + cacheWrite 0），pi 启动不再报 `Invalid models.json schema`。
- [ ] 其他 provider 节点（aruoshui-* 等）与未知键保持原样（既有 golden 测试覆盖）。

## Notes

- 影响面排查结论：store.rs 导入是 1:1 无换算（与 pi $/M 自洽）；usage.rs 用 canonical $/M 直算与 models.json 无耦合；其余 renderer（claude/codex/opencode）不携带 cost。
- 修复后线上修复路径：重建后端 → 触发重新 apply（canonical 里存的是正确 $/M 值）→ 校验文件。
