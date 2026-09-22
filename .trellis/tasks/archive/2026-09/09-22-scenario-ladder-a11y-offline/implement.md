# 执行计划：L3 阶梯 + 可见性修复 + 离线验证

## 阶段 1 · 可见性修复（R2，独立可交付）

1. [x] 写悬空引用审计：提取 `mgr-web/src/*.css` 全部 `var(--X)` 引用集合与
       `--X:` 定义集合，求差集（排除 `--font-*` 等已定义项）
2. [x] `:root` 补 `--elev-ring: 0 0 0 1px var(--border);`
3. [x] 修复审计发现的**其余**悬空引用
4. [x] `npm run build`（mgr-web）确认产物含该 token
5. [x] 验证 `.check` 计算样式：puppeteer 起 vnc 容器截图或 CDP 读 computed style
   → **AC1 / AC2**

## 阶段 2 · L3 阶梯分组（R1）

6. [x] `scenario.rs`：`ScenarioMeta` 加 `installer` 字段 + `default_installer()`
       + 单测（默认回落 / 显式取值）
7. [x] 23 个 `scenarios/*/scenario.toml` 显式补 `installer = "..."`
8. [x] `tui.rs`：行模型加 `SubHeader`，同层多 installer 时插入阶梯标题
9. [x] `mgr-web/src/types.ts`：`Scenario` 加 `installer`
10. [x] `mgr-web/src/i18n.ts`：加 `insMise/insApt/insNpm/insTarball` 双语词条
11. [x] `EnvPicker.tsx`：层内按 installer 二次分组，多方式时渲染阶梯子标题
12. [x] `cargo test`（config + mgr）+ `npm run build`
    → **AC3 / AC4**

## 阶段 3 · 离线验证（R3）

13. [x] 清点磁盘 + 核对旧变体镜像无引用 → 清理腾空间（D4）
14. [x] `make save` → 检查 bundle 内容与体积 → `make load`
15. [x] `make up NOBUILD=1` 起栈并 curl 探活 → **AC5**
16. [x] `docker run --network none` 对 enabled.toml 每个场景跑代表性命令探针
    （login + 非 login 双通道）→ **AC6**
17. [x] 离线容器内 `mise use -g hyperfine` → 记录行为（成功/失败/报错形态）
    → **AC7**
18. [x] 结论写入 `docs/offline-install-guide.md`

## 阶段 4 · 收尾

19. [x] 全量 `cargo test` 最后一次全绿
20. [x] spec 同步（若阶梯机制值得沉淀：`frontend/` 或 `guides/`）
21. [x] 提交 + 推送分支

## 风险与回滚

| 风险 | 缓解 | 回滚 |
|---|---|---|
| 删镜像腾空间误伤 | 删前核对 `docker ps -a` / compose 引用 | 镜像可 `make gen + build-base` 重建 |
| `make save` 写满磁盘影响宿主 | 先算体积再执行；分步 `du -sh` | 删 `aio-offline-bundle/` |
| Rust 改动破坏既有单测 | 先跑基线 `cargo test` 存证 | `git checkout -- config/` |
| 离线探针误报（探针本身依赖网络） | 探针用 `--network none` 且逐条核对命令形态 | — |

## 启动前检查

- [x] prd.md / design.md / implement.md 齐备
- [x] 用户已授权自主执行（本任务由 owner 明确「不需要再确定」）

---

## 执行结果（2026-09-22 完成）

全部 21 项执行完毕。三个可交付：

| 交付 | 结论 |
|---|---|
| 可见性修复 | 悬空 `var()` 审计 0；`--elev-ring` 恢复 + `--control-ring`（实测 80% alpha 全主题达 WCAG 3:1） |
| L3 阶梯 | `installer` 字段 + TUI/SPA 双侧子标题；L3 分两阶、L2/L4 保持平坦 |
| 离线验证 | `make save`/`load`/`up NOBUILD=1` 通过；离线探针 login 35/35 + 非 login 35/35 |

**过程中纠正的两处自己的错误**（记录以备复查）：
1. 静态 HTML 复刻页里把基线取成了 `.row`，但 `.row` 无背景（透明），
   真实可见背景在 `.rows` 上 —— 换用真实 SPA 驱动后才量对。
2. 给 `scenario-authoring.md` 写「gen 按 (category_rank, installer_rank, id) 排序」
   是凭印象写的；核对 `gen.rs::sort_by_layer` 后发现实际是 `(category_rank, id)`，
   已改正。

**未做（明确超出本任务范围）**：已归档父任务 `09-22-scenario-items-layered-mise`
的 AC6（pi 扩展登记流程，需终端）未复验 —— 本任务的离线探针覆盖了 pi CLI 本体，
但 `aio-pi-extensions` 的登记动作本身仍未跑过一次。
