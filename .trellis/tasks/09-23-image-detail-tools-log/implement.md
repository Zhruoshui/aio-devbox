# implement.md — image-detail-dialog + custom-tools-editor + build-log-full（含 review 后三条修正）

## 实施顺序

1. **R1 镜像详情弹窗** — 纯前端，改动小，可先落地验证弹窗模式。（已完成）
2. **R3 构建日志完整可看** — 后端 + 前端，有 DB 迁移，中等复杂度。（已完成）
3. **R2 简易编辑器（修正 2）** — 前后端简化，删结构化表单，加 Tab 缩进 + 状态行。
4. **R4 自定义工具 L5 化（修正 3）** — 横跨 config + mgr + 前端，数据模型变化最大。

建议按 1 → 3 → 2 → 4 顺序提交（R3 在 R2 之前是因为它改动了 `JobShared` / `jobs` 表 / `routes.rs`，与 R2 的新 API 有轻微的文件级重叠，先做掉避免合并冲突）。每个 R 完成即跑对应 AC。

---

## R1 镜像详情弹窗（已完成）

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr-web/src/pages/ImageDetailDialog.tsx` | **新增** — 提取现有 `comboDetail` 并扩展头部元数据 |
| `mgr-web/src/pages/ImagesPage.tsx` | 删 `combo:<hash>` 槽位 / `comboDetail`；加 `detailFor` state；渲染弹窗 |
| `mgr-web/src/i18n.ts` | 新增弹窗标题/aria-label 等少量文案 |
| `mgr-web/src/styles.css` | 加 `.img-detail-dialog` 宽度与元数据块样式 |

### 步骤

- [x] 新建 `ImageDetailDialog.tsx`：props `{ img, scenarios, lang, onClose }`，用 `.overlay.open` + `.dialog`，Esc / 遮罩 / 关闭按钮均可关闭
- [x] 把 `ImagesPage.tsx:143-189` 的 `comboDetail` 函数搬进弹窗，头部加元数据块（tag / env_hash / size / built_at / refcount）
- [x] `ImagesPage` 中 `OpenPanel` 改为 `"" | "log:<hash>"`，删除 `comboOpen` 分支和 `comboDetail` 函数
- [x] 组合说明按钮的 `onClick` 改为 `setDetailFor(img)`
- [x] 在 `ImagesPage` 底部条件渲染 `<ImageDetailDialog>`
- [x] 样式：`styles.css` 加 `.img-detail-dialog` 容器宽度约束与元数据块
- [x] `i18n.ts` zh / en 补文案

### 修正 1 追加步骤

- [ ] `ImagesPage.tsx` 组合说明单元格渲染 `comboShort(img.combo)`（截断到前 3 个场景 id + `+…`）；`combo === null` 时回退 `img.env_hash.slice(0,12)`
- [ ] 移除列按钮的 `title={img.combo ?? img.env_hash}`（避免悬停再次曝露长说明）

### 验证

- [ ] AC1 / AC2 / AC3：`npm run build` 通过；镜像页组合说明列只显示短摘要；点开弹窗看完整组合；行内展开只剩 build log

---

## R3 构建日志完整可看（已完成）

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr/src/jobs.rs` | `LOG_TAIL` 8KB→64KB；`BUILD_LOG_TAIL` 256KB→4MB；修 `append_log` char 边界；`run_create` 落盘；`JobShared.log_path` |
| `mgr/src/db.rs` | jobs 表加 `log_path` 列；images 表加 `job_id` 列；相应 helper 调整 |
| `mgr/src/state.rs` | `JobShared` 加 `log_path: Option<String>` |
| `mgr/src/routes.rs` | 新增 `GET /api/jobs/:id/log`；`GET /api/images/:env_hash/log` |
| `mgr-web/src/types.ts` | `Job.log_path?: string \| null`；`Image.job_id?: number \| null` |
| `mgr-web/src/api.ts` | `getJobLog(jobId, offset, limit)`；`getImageLog(envHash)` |
| `mgr-web/src/pages/JobView.tsx` | 跟随/暂停跟随 + 跳到顶部 + 回到底部 |
| `mgr-web/src/pages/ImagesPage.tsx` | 展开时调 `getImageLog` 加载完整日志 |
| `mgr-web/src/i18n.ts` | 新增按钮/提示文案 |
| `mgr-web/src/styles.css` | `.job-log-controls` 按钮行样式 |

### 步骤

- [x] `state.rs`：`JobShared` 加 `log_path: Option<String>` 字段
- [x] `db.rs`：jobs 表 / images 表加列（pragma-probe 幂等 ALTER）；`insert_job` / `persist_job` / `upsert_image` 调整签名
- [x] `jobs.rs`：`LOG_TAIL` 64KB / `BUILD_LOG_TAIL` 4MB；`append_log` 对齐 char 边界；`run_create` 创建 `data/jobs/<id>.log` 并把每行追加进去；`JobShared.log_path` 填上
- [x] `routes.rs`：`GET /api/jobs/:id/log` 按 offset/limit 读文件，fallback 内存；`GET /api/images/:env_hash/log` 按 `job_id` 找落盘文件，fallback `build_log` 列
- [x] `types.ts` / `api.ts`：新类型与新方法
- [x] `JobView.tsx`：scroll 事件跟踪 followRef；`job?.log` 变化只在 follow 时滚底；加"跳到顶部" / "回到底部"按钮；job 结束后用 `getJobLog(0, limit)` 预取完整日志替换 `<pre>` 内容
- [x] `ImagesPage.tsx`：展开日志时调 `getImageLog(envHash)`；加载前显示 loading 态
- [x] `i18n.ts` zh / en；`styles.css` 按钮行

### 验证

- [ ] AC8：完整构建一次，检查 `mgr-data/jobs/<id>.log` 行数与输出一致
- [ ] AC9：JobView 向上滚动暂停跟随，回到底恢复；跳到顶部加载从头内容
- [ ] AC10：ImagesPage 日志展开能加载超过 256KB 的完整输出
- [ ] AC11：构造含中文的长构建输出，跨 64KB 边界时无 panic，字符完整

---

## R2 简易编辑器（修正 2）

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr/src/usertools.rs` | 删 parse/render/validate_entries/render_block/InstallKind/ToolEntry；加 `list_tools`；保留 validate/write_atomic/fragment_path |
| `mgr/src/routes.rs` | `get_user_tools` 返回 `{content, tools, path}`；删 `put_user_tools_entries` |
| `mgr-web/src/pages/UserToolsPage.tsx` | 重写为单模式文本编辑器（Tab 缩进 + 状态行） |
| `mgr-web/src/types.ts` | `UserToolsFile` 改为 `{content, tools: [{id,name}], path}`；删 `ToolEntry`/`ToolInstall` |
| `mgr-web/src/api.ts` | 删 `putUserToolsEntries` |
| `mgr-web/src/i18n.ts` | 编辑器状态行/工具计数文案 |
| `mgr-web/src/styles.css` | 编辑器状态行样式 |

### 步骤

- [ ] `usertools.rs`：删表单支撑逻辑；加 `list_tools(content) -> Vec<String>`（数 `# --- tool:` 标记块）
- [ ] `routes.rs`：`get_user_tools` 返回 `{content, tools, path}`；删 `put_user_tools_entries` 端点
- [ ] `types.ts` / `api.ts`：同步契约
- [ ] `UserToolsPage.tsx`：重写为单 textarea 编辑器；Tab/Shift+Tab 缩进；状态行（行数/字节/dirty）；工具计数展示；保存后重建路径提示
- [ ] `i18n.ts` zh / en；`styles.css` 状态行

### 验证

- [ ] AC4：`npm run build` 通过；编辑器保存后重新读取内容一致；Tab 缩进生效；状态行显示正确
- [ ] `cargo test` 通过（usertools 简化后测试同步删）

---

## R4 自定义工具 L5 化（修正 3）

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr/src/envhash.rs` | `SandboxEnv` 增 `user_tools`；`to_manifest_checked` 校验 L5 id；`assemble_with_tools`；`describe_combo` 增 `[tools: ...]` |
| `mgr/src/jobs.rs` | `run_create` 改用 `assemble_with_tools` + `env_hash` |
| `mgr/src/routes.rs` | `normalize_services` 保留 user_tools；PUT 合并 user_tools；`get_user_tools` 返回工具列表；`put_user_tools` 同步 `scenarios/` 目录 |
| `mgr/src/usertools.rs` | 加 `sync_scenario_dirs(repo, content)` |
| `config` | 删 `scenarios/user-tools/`；`.aio/enabled.toml` 删 `"user-tools"` |
| `mgr-web/src/types.ts` | `SandboxEnv` 增 `user_tools?: string[]`；`CreateBody` 增 `user_tools?: string[]` |
| `mgr-web/src/pages/EnvPicker.tsx` | `LAYERS` 增 L5 分组 |
| `mgr-web/src/pages/CreatePage.tsx` | `env` 增 `user_tools`；`envKey` 纳入 user_tools |
| `mgr-web/src/pages/EditPage.tsx` | `sortEnv` 纳入 user_tools |
| `mgr-web/src/pages/ImagesPage.tsx` | `comboKey` 纳入 user_tools |
| `mgr-web/src/i18n.ts` | 增 `layL5` 文案 |

### 步骤

- [ ] `envhash.rs`：`SandboxEnv` 增 `user_tools: Vec<String>`（serde default，canonical_json 排序去重）；`to_manifest_checked` 校验 user_tools 都是 `category=="service"` 的 scenario 且不加入 manifest.scenarios；`assemble_with_tools(repo, env)`；`describe_combo` 增 `[tools: ...]`
- [ ] `jobs.rs`：`run_create` 改 `let dockerfile_base = envhash::assemble_with_tools(&repo, &env)?;`
- [ ] `routes.rs`：`normalize_services` 保留 body.env.user_tools；PUT-recreate 时合并当前行 env_json 里的 user_tools；`get_user_tools` 返回 `tools: [{id,name}]`（扫 `scenarios/*` 的 `category=="service"`）；`put_user_tools` 成功后 `sync_scenario_dirs`
- [ ] `usertools.rs`：`sync_scenario_dirs(repo, content)` — 对每个 `# --- tool: <name> ---` 块确保 `scenarios/<id>/scenario.toml` + `fragment.Dockerfile` 存在且一致；删除多余的 `scenarios/<service>/`
- [ ] `config`：删 `scenarios/user-tools/`；`.aio/enabled.toml` 删 `"user-tools"`
- [ ] `types.ts`：`SandboxEnv.user_tools?: string[]`；`CreateBody.user_tools?: string[]`
- [ ] `EnvPicker.tsx`：`LAYERS` 增 `{ cats: ["service"], labelKey: "layL5" }`
- [ ] `CreatePage.tsx`：`env` state 增 `user_tools: []`；`envKey` 纳入 user_tools
- [ ] `EditPage.tsx`：`sortEnv` 纳入 user_tools
- [ ] `ImagesPage.tsx`：`comboKey` 纳入 user_tools
- [ ] `i18n.ts` zh / en：增 `layL5`

### 验证

- [ ] AC5：`GET /api/scenarios` 返回 L5 分组（自定义工具条目）
- [ ] AC6：勾选不同 L5 工具集合产生不同 env_hash / 镜像 tag；勾掉后重建不含该工具
- [ ] AC7：保存 user-tools 后 `GET /api/scenarios` 立即反映新工具集（无需重启）
- [ ] `cargo test`（mgr + config）通过；`npm run build` 通过

---

## 回归与合并

- [ ] 每个 R 完成分别跑对应 AC；全部完成后跑一次全量 `cargo test`（workspace）+ `npm run build`
- [ ] `task.py check` 触发 review gate，按反馈修复
- [ ] 单分支单 commit 或按 R 分四个 commit 提交（推荐后者）
