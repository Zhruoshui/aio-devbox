# 实施计划:统一模型配置页

> task 08-26-unified-model-config。前置:prd.md(需求/AC)、design.md(技术设计)、research/(pi-web、cc-switch、AIO 接入点三份调研)。
> 执行方式:Claude Code 子代理分发(implement.jsonl / check.jsonl 已策展)。每个里程碑独立可验证、独立提交(回滚点)。

## 里程碑清单(按序)

### M0 脚手架:依赖 + pane 类型 + 路由骨架
- [ ] `app/Cargo.toml`:加 `reqwest`(default-features=false, features=["json","rustls-tls"])、`rusqlite`(features=["bundled"])、`json5`。
- [ ] `app/src/routes/models/mod.rs` 新模块(先只挂 `GET /api/models/config` 返回空骨架),`main.rs` 挂路由。
- [ ] `app/services.toml`:`[[service]] id="modelsConfig" type="page" label="模型配置"`;manifest 路由让 `type="page"` 恒 enabled。
- [ ] `web/src/types.ts` ServiceEntry.type 加 `"page"`;`App.tsx` 的 `isServiceEntry`/`PaneForService` 加分支;`panes/ModelsPane.tsx` 占位(页签空壳);`icons.tsx`/`i18n.ts`/`Sidebar.tsx` 配套。
- 验证:`cd app && cargo build && cargo test`;`cd web && npm run build && node smoke-test.cjs`;容器重建后按钮出现、pane 可开。

### M1 canonical 存储 + 供应商库 UI
- [ ] `store.rs`:`~/.aio/models.json` 读写(原子写 0600、目录 0700、进程内 Mutex、掩码合并语义);serde 类型 = design §2 schema。
- [ ] GET/PUT `/api/models/config`(GET 打码);校验(providerId kebab、modelId 非空、agents 引用存在)。
- [ ] ModelsPane 供应商库页签:provider 列表/新增/删除 + 详情编辑(含 apiKey 密码框"留空保持不变"、anthropic 块、headers/compat 折叠)。
- [ ] 首启「从 pi 导入」:models.json providers 1:1 入库(design §8)。
- 验证:Rust 单测(掩码往返、校验拒绝);容器内 curl PUT→GET 打码正确;UI 增删改持久。

### M2 模型列表拉取 + 可用性检测
- [ ] `discover.rs`:URL 推导(api 分支)+ 多候选回退 + 头构造 + 响应多形态解析(design §5);20s 超时;502 带截断上游 body。
- [ ] `test.rs`:三协议端点最小补全(max_tokens 16、无重试、20s 超时);`{ok, latencyMs, status, error, responseText}`。
- [ ] UI:模型表行内测试按钮+状态胶囊;「从端点拉取模型」搜索勾选弹层(全选/加入)。
- 验证:表驱动单测(URL 候选序列、解析形态);容器内对 ai.aruoshui.com 实测 discover/test 成功与失败两态(真实 AC1/AC2)。

### M3 渲染器 + apply + agent 页签
- [ ] 公共:备份(滚动×3)+ 原子写 + 回读校验 helper。
- [ ] `render/pi.rs`:models.json provider 节点合并(其他 provider 保留)+ settings.json defaultProvider/defaultModel。
- [ ] `render/opencode.rs`:json5 读 + provider 片段 + 顶层 model 键,其余保留。
- [ ] `render/claude.rs`:settings.json env 键级合并(其余保留)。
- [ ] `render/codex.rs`:auth.json 单键 + config.toml 合并(/v1 归一、wire_api、失败回滚 auth)。
- [ ] `GET /api/models/agents`(installed=command_exists + live 回读)、`POST /api/models/apply/{agent}`。
- [ ] UI:四 agent 页签(兼容过滤下拉、覆盖项、生效按钮、written/backup 展示、未安装横幅)。
- 验证:每渲染器 golden-file 单测(用户键保留/合并/备份);容器实测 AC3/AC4/AC5(pi 现有 provider 不被破坏=AC7 抽查)。

### M4 用量统计
- [ ] `usage.rs`:pi jsonl 扫描 + opencode rusqlite(只读)+ claude/codex 解析器(存在性守卫)+ 窗口过滤 + 30s 缓存。
- [ ] `GET /api/models/usage?window=…`。
- [ ] UI 用量统计页签:窗口切换 + 表格 + 合计 + 刷新;cost 列仅数据源有值时显示。
- 验证:fixture 单测聚合正确;容器实测 AC6(与手工 grep 抽验一致)。

### M5 收尾
- [ ] `app/services.toml` 注释、README(zh)章节:统一模型配置页说明(含 pi-web 共存说明、备份/回滚方法)。
- [ ] 全量回归:cargo test、web build+smoke、AC1-AC7 全走查(trellis-check)。
- [ ] spec 沉淀(trellis-update-spec):manifest pane 类型扩展契约、渲染器键级合并约定。

## 验证命令汇总

```bash
cd app && cargo build && cargo test
cd web && npm run build && node smoke-test.cjs
# 容器重建(注意:compose up --build 对运行中服务不重建,须显式 build + force-recreate)
docker compose build app && docker compose up -d --force-recreate app
docker exec aio-app-1 curl -s localhost:8080/api/models/config   # 端口以 compose 实际为准
```

## 风险文件与回滚点

- 高风险:`~/.pi/agent/models.json`(用户现有 2 provider)——渲染器必须先备份再合并,单测覆盖"未知键保留";apply 前自动备份即回滚手段。
- 中风险:`web/src/App.tsx`(布局核心)——只加分派分支,不动布局逻辑;每里程碑一提交,revert 单提交即回滚。
- canonical/新路由均为纯增量,无存量迁移。

## task.py start 前检查

- [ ] prd.md 收敛(无遗留 open question)✅
- [ ] design.md / implement.md 就绪 ✅
- [ ] implement.jsonl / check.jsonl 已策展真实条目 ✅(见文件)
- [ ] 用户审阅规划工件并批准
