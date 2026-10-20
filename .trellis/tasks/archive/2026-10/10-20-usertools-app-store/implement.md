# implement — usertools 应用商店化

单任务执行(backend/frontend 同仓强耦合、一个验收闭环,不拆父子任务;inline 工作流,经 trellis-before-dev 加载上下文)。

## 有序清单

1. **mgr:模块目录化** — `mgr/src/usertools.rs` → `usertools/{mod.rs,...}`;保留 slug/list_tools/validate/write_atomic 等骨干与其单测(纯移动,`cargo test -p aio-mgr` 先绿再继续)。
2. **mgr:recipe.rs** — serde schema(tagged enum 五类型)+ 字段校验 + slug 冲突;单测:合法/缺字段/未知类型/kebab-case round-trip。
3. **mgr:render.rs** — 五类型模板 + 确定性输出;单测锚定 AC1/AC6:同 recipe 两次渲染字节相同;只改元数据 fragment 不变;各类型产物过 validate();apt 模板含清 lists 与双通道自检。
4. **mgr:builtins.rs** — 种子清单(apt 类 8~12 个,实现期定名单,含元数据)+ 物化函数;单测:id 与 slug 合法、去重。
5. **mgr:store.rs** — 货架卡片扫描(recipe 元数据合并、无 recipe 回退标记名)+ 引用统计(扫 sandboxes env_json);单测:存量无 recipe 目录、引用计数、stale 引用。
6. **mgr:routes.rs** — 新端点(store/tools CRUD/preview)+ L5 元数据增强(routes.rs:196-204)+ 退役单文件 GET/PUT(routes.rs:65)与 usertools.json 读写;路由测试覆盖 AC5(400+清单/force)、409 冲突、preview 一致性。
7. **mgr-web 基础** — types.ts/api.ts/i18n.ts(zh+en)新契约;删 UserToolsPage.tsx,App.tsx rail 位换 StorePage。
8. **mgr-web StorePage** — 我的货架卡片 + 推荐货架 + 上架对话框(类型→动态字段→preview 调用)+ raw 编辑器(复用旧 textarea 交互:Tab 缩进/状态行)+ 删除确认(引用清单)+ 更新轻提示;EnvPicker L5 消费元数据。
9. **仓库清理** — 删 `usertools/fragment.Dockerfile`;清 `scenarios/user-tools/` 空目录;mgr 启动 best-effort 删 `mgr-data/usertools.json`;aio-env-config skill 参考文档 user-tools 条目改指向商店。
10. **验证** — 下述命令全绿后,`make mgr-up` 起栈走 AC1→AC5 实机流程(上架 htop→建沙箱→容器内双通道 `command -v htop`;删除保护两分支)。

## 验证命令

```bash
cargo test -p aio-mgr                    # 步骤 1-6 每步之后
cd mgr-web && npm run build              # tsc --noEmit + vite build(AC8)
make mgr-up                              # 实机验证(AC1-AC5)
# 容器内双通道自检(AC3):
docker exec <app容器> bash -lc 'command -v htop'
docker exec <app容器> bash -c  'command -v htop'
```

前端热验证可走 vnc puppeteer 热部署通道(memory: frontend-verify-live-workflow),不强制全量重建。

## 风险文件与回滚点

- `mgr/src/usertools.rs`(拆目录,纯移动先行、单独可 revert)
- `mgr/src/routes.rs`(端点增删;单文件 API 移除是破坏性变化,无已知消费方)
- `mgr/src/envhash.rs` / `jobs.rs`:**预期零改动**——若实现中发现要动,停下来回到 design 复核。
- 每步一个 commit 粒度;整体 revert = 回滚(无 schema/迁移,见 design「回滚」)。

## start 前检查

- [x] prd.md 收敛(无遗留 Open Questions)
- [x] design.md / implement.md 就绪
- [x] implement.jsonl / check.jsonl 已策展真实条目
- [ ] 用户过审三件套 → `task.py start`
