# implement.md — picker UX 执行计划

> 顺序：后端 schema → 后端流式 build → 前端类型/详情页 → EnvPicker → 校验。
> 后端先行：前端 Image 新字段依赖 /api/images 响应。
> 每个 [ ] 是一个可独立验证的原子步；实现/校验走 trellis-implement / trellis-check 子代理。

## Phase A — mgr 后端

- [ ] A1 `mgr/src/db.rs`：images SCHEMA 增 `env_json TEXT`、`services_json TEXT` 两列；
  `init_schema` 加两条 pragma-probe ALTER（照 `combo` 模式）；`db::upsert_image` 增
  `env_json`/`services_json` 参（空串 = 保留旧值，CASE WHEN 语义）；`list_images`
  返回扩为 7 元组；db.rs 内相关测试同步更新
- [ ] A2 `mgr/src/db.rs`：Services 已有 `canonical_json()`（若无则补，以写入侧为准）
- [ ] A3 `mgr/src/docker.rs`：`build` 改流式（`build_streaming` 形态：两个 piped reader
  select! 合并、逐行回调、全程累积带 cap）；失败 error 携带 merged tail；
  `run_capture` 保留给其他命令；docker.rs 若有 build 单测同步更新
- [ ] A4 `mgr/src/jobs.rs`：增 `BUILD_LOG_TAIL = 256 * 1024`；base 构建改调
  `build_streaming`（回调 `append_log`），`base_build_log = tail(&captured, BUILD_LOG_TAIL)`；
  app/cs/vnc 构建同样改流式；失败时把 error 的 merged tail 追加进 job log 再返回 Err
- [ ] A5 `mgr/src/jobs.rs`：upsert_image 调用补 `env.canonical_json()` +
  `services.canonical_json()`
- [ ] A6 `mgr/src/routes.rs`：`list_images` 每行增 `env`/`services` 键（解析失败 → null）；
  注释头更新契约

**A 验证**：`cargo test -p mgr` 绿；`cargo build -p mgr` 过。

## Phase B — mgr-web 类型与镜像详情

- [ ] B1 `mgr-web/src/types.ts`：`Image` 增 `env: SandboxEnv | null`、
  `services: { code_server: boolean; vnc: boolean } | null`；文件头注释镜像契约行更新
- [ ] B2 `mgr-web/src/i18n.ts`：中/英增详情区与折叠/全选键（如 `imgDetail`/`imgScen`/
  `imgAlwaysOn`/`imgServices`/`layerAll`/`layerNone`/`layerCollapse`）
- [ ] B3 `mgr-web/src/pages/ImagesPage.tsx`：`useEffect` 增 `listScenarios()` 拉 catalog
  （自拉一次，错误置空数组降级）；展开 state 改单一 `open: "" | "combo:<h>" | "log:<h>"`；
  combo 列改按钮触发展开；展开行渲染详情区（场景段/必装段/服务段），`env === null` 时
  回退仅日志提示
- [ ] B4 `mgr-web/src/components/components.css`：`.layer-head`/`.layer-count`/详情区/
  combo-toggle 样式（复用现有色系与 `.table`/`.card`）

**B 验证**：`tsc --noEmit`（mgr-web 目录既有命令）绿；人工起 mgr 看 ImagesPage 新行
有详情、旧行回退。

## Phase C — EnvPicker 折叠 + 全选

- [ ] C1 `mgr-web/src/pages/EnvPicker.tsx`：增 `collapsed` state；`group()` 头改
  折叠按钮（chevron + label + `selected/total` 计数）+ 全选/全不选按钮；折叠分支
  跳过 `layerRows`；全选逻辑按 design §1.2（并集/差集该层非 always_on id）
- [ ] C2 `mgr-web/src/i18n.ts`：增 `layerAll`/`layerNone` 中英（若 B2 已加则复用）
- [ ] C3 `mgr-web/src/components/components.css`：`.layer-head` 等（若 B4 已加则复用）

**C 验证**：`tsc --noEmit` 绿；AC1/AC2 手工：折叠 L2 提交 env 不变；L2 全选后
env.scenarios 恰含 10 个 shell id。

## Phase D — 端到端与回归

- [ ] D1 AC4：构造一次失败构建（如临时把某 scenario fragment 改坏再还原），JobView
  过程见滚动行；失败后 ImagesPage 该 hash build_log 含失败现场、>8KB 截断感消失
- [ ] D2 AC3：点击新行 combo 展开详情，内容含场景+版本+服务；旧行（无 env）回退
- [ ] D3 回归：创建向导完整走一遍（选 L2 两项 + L4 一项 + 服务开关），创建成功、
  env_hash 正确；`cargo test -p mgr` 全绿
- [ ] D4 文档：`.trellis/spec/backend/index.md` 与 frontend 若有 job 日志/镜像契约
  条目，补 env/services 两键与 256KB 约定

## 回滚点

| 步 | 回滚 |
|---|---|
| A1 schema | 两列可留（NULL 无害），`git revert` A 系提交 |
| A3/A4 流式 | 还原 `build` 为 run_capture 形态 + jobs.rs 旧调用（一个 commit 内） |
| B/C 前端 | 纯前端，revert 即回滚；无数据迁移 |

## 验证命令汇总

```bash
cargo test -p mgr
cargo build -p mgr
cd mgr-web && npx tsc --noEmit   # 以项目既有脚本为准（package.json）
```
