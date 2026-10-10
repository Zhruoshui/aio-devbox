# design.md — image-detail-dialog + custom-tools-editor + build-log-full（含 review 后三条修正）

本任务交付四块：R1 镜像详情弹窗、R2 用户自定义工具简易编辑器（修正后）、R3 构建日志完整可看、R4 自定义工具 L5 化（修正 3，横跨 config + mgr + 前端）。R1/R2/R3 相互独立；R4 依赖 R2 的编辑器产出的 fragment 形态。共享的改动约束在最后。

---

## R1 镜像组合说明 → 弹窗式详情（含修正 1：列里只留短摘要）

### 现状

- `ImagesPage.tsx` 组合说明列的按钮直接渲染 `img.combo ?? img.env_hash.slice(0,12)`——`img.combo` 是 `describe_combo` 生成的完整字符串（`a+b+c id@ver,... [svc: cs,vnc]`），长且无截断，行被挤变形。
- 详情弹窗 `ImageDetailDialog.tsx` 已实现，承载完整组合信息。

### 改动（修正 1）

- **组合说明列只显示短摘要**：`comboShort(img)` 对 `img.combo` 做截断——按 "+" 拆分，只保留前几个场景 id，末尾加省略号；`combo === null`（pre-S3 行）时回退到 `img.env_hash.slice(0,12)`（现状不变）。
- 列内容仍是按钮，点击打开 `ImageDetailDialog`（不变）。
- 移除列按钮的 `title={img.combo ?? img.env_hash}`（title 会再次把长说明曝露在悬停里，违背"列里只留短摘要"）。

短摘要算法（前端，pure）：
```
comboShort(combo: string): string
  // "ripgrep+fd+bat+eza+jq node@22.23.2,python@3.13.0 [svc: cs,vnc]"
  //   -> "ripgrep+fd+bat+…"
  const head = combo.split(" ")[0];          // 场景 id 段（已排序）
  const ids = head.split("+");
  return ids.length <= 3 ? ids.join("+") : ids.slice(0, 3).join("+") + "+…";
```

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr-web/src/pages/ImagesPage.tsx` | 组合说明单元格渲染 `comboShort`；去掉 `title` 长曝露 |
| `mgr-web/src/i18n.ts` | 无需新增文案（复用现有 imgDetail/imgCombo） |

**后端**：无改动。

---

## R2 用户自定义工具简易编辑器（修正 2：移除结构化表单，只留直接编辑）

### 现状

- `UserToolsPage.tsx` 目前是"双 Tab（结构化表单 / 直接编辑）+ 解析回落"。结构化表单的解析/生成/双向同步复杂度不值得——`fragment.Dockerfile` 本身就是事实标准。
- `usertools.rs` 持有 `parse` / `render` / `validate_entries` / `render_block` 等表单支撑逻辑。

### 改动

**`mgr-web/src/pages/UserToolsPage.tsx` 重写为单模式简易文本编辑器**：

- 不再有 Tab / 表单 / 解析回落。整个页面就是一个等宽 `<textarea>`（直接编辑 `fragment.Dockerfile`）。
- **Tab 缩进**：textarea `onKeyDown` 拦截 `Tab` → 插入两个空格（不切换焦点）；`Shift+Tab` 删除光标左侧的两个空格（如果有）。
- **状态行**：textarea 下方一行等宽小字，显示 `{行数} 行 · {字节数} · 已修改 / 已保存`，随 `onChange` 实时更新。
- 顶部保留三条硬规则提示块（.aio 硬规则，scenario.toml 同源）；保留"当前编辑的文件路径"展示。
- 保存按钮 → `PUT /api/user-tools`（body `{content}`），保存后重新 `GET /api/user-tools` 刷新 `dirty` 状态。保存成功显示重建路径提示。
- **显示该文件实际对应的工具**：从内容里数 `# --- tool: <name> ---` 标记块的个数，在标题旁显示"当前包含 N 个工具"（让用户感知保存后的 L5 列表会变成什么）。

**后端简化**（`mgr/src/usertools.rs`）：

- 移除 `parse` / `render` / `validate_entries` / `render_block` / `InstallKind` / `ToolEntry` / 相关测试（表单一去，这些就是死代码）。
- 保留 `fragment_path` / `validate`（/root 静态校验）/ `write_atomic`。
- `GET /api/user-tools` 不再返回 `entries` / `parse_note`：返回 `{ content, tools: string[], path }`——`tools` 是数 `# --- tool:` 标记块得到的工具名列表（纯展示，驱动 UI 的"当前包含 N 个工具"）。
- `PUT /api/user-tools/entries` **删除**（表单的保存端点）。

### 数据结构变化

**`UserToolsFile`（前端类型，新契约）**：

```ts
interface UserToolsFile {
  content: string;     // 完整 fragment.Dockerfile 文本
  tools: string[];     // 从 `# --- tool: <name> ---` 标记块解析出的工具名（展示用）
  path: string;        // 文件绝对路径
}
```

### 改动文件

| 文件 | 改动 |
|---|---|
| `mgr/src/usertools.rs` | 删 parse/render/validate_entries/render_block/InstallKind/ToolEntry；加 `list_tools(content)`；保留 validate/write_atomic/fragment_path |
| `mgr/src/routes.rs` | `get_user_tools` 返回 `{content, tools, path}`；删 `put_user_tools_entries` |
| `mgr-web/src/pages/UserToolsPage.tsx` | 重写为单模式文本编辑器 |
| `mgr-web/src/types.ts` | `UserToolsFile` 改为 `{content, tools, path}`；删 `ToolEntry`/`ToolInstall` |
| `mgr-web/src/api.ts` | 删 `putUserToolsEntries`；`getUserTools` 返回类型更新 |
| `mgr-web/src/i18n.ts` | 删表单文案；加编辑器状态行/工具计数文案 |

---

## R3 构建日志完整可看

### 现状问题

- `jobs.rs:27` `LOG_TAIL = 8KB`（job 内存截断）。
- `jobs.rs:33` `BUILD_LOG_TAIL = 256KB`（images 表落库截断）。
- `jobs.rs:614-616` `job.log[cut..]` 未对齐 char 边界，多字节跨边界会 panic。
- `docker.rs:24` `BUILD_OUTPUT_CAP = 4MB`（docker build captured 缓冲上限，已够）。
- 完整输出从不落盘。

### 改动

**`mgr/src/jobs.rs`**：

- `LOG_TAIL`：8KB → **64KB**。
- `BUILD_LOG_TAIL`：256KB → **4MB**。
- 修复 `append_log` 的 char 边界对齐（照抄 `tail()` 的 `is_char_boundary` 逻辑）。
- 落盘：新增 `full_log_path(state: &AppState, job_id: i64) -> PathBuf` = `state.data/jobs/<id>.log`。
- `run_create` 开头创建该文件并持有 `BufWriter`；`line_forward` 闭包在原有 `log_tx.send` 之外同时追加到文件。文件 IO 走 `tokio::fs` 的 spawn_blocking 或 `std::fs` + `tokio::task::spawn_blocking`（写盘低频，每行一次 spawn_blocking 可接受）。
- 落盘失败不阻塞构建，只 `tracing::warn!` 并继续内存路径。
- 构建结束时 flush + close；`persist_job` 时把 `log_path` 一并写入 jobs 表新列。
- `JobShared` 增加 `log_path: Option<String>` 字段（`None` = 旧 job，未落盘）。

**`mgr/src/db.rs`**：

- `jobs` 表新增 `log_path TEXT` 列（幂等 ALTER，跟现有迁移方式一致）。
- `persist_job` / `insert_job` / `get_job` 相应调整。

**`mgr/src/routes.rs`** 新增：

- `GET /api/jobs/:id/log?offset=<byte>&limit=<byte>`
  - 从落盘文件读取指定范围，返回 `{ data: string, total_size: number }`。
  - 文件不存在（旧 job）时 fallback 到内存里的 `job.log`。
  - `offset` 默认 0，`limit` 默认 256KB。

**前端 `JobView.tsx`**：

- 把"无脑滚到底"改为"跟随 + 暂停跟随"：
  - `followRef = useRef(true)`，scroll 事件里根据 `scrollTop + clientHeight >= scrollHeight - 40` 更新。
  - `job?.log` 变化时只在 `followRef.current === true` 才 `scrollTop = scrollHeight`。
- 新增"跳到顶部"按钮：
  - 若 job 有 `log_path`（或 job 状态非 running），调 `getJobLog(jobId, 0, limit)` 从头加载，替换 `<pre>` 内容为完整日志，滚动到顶。
  - 若 job 还在 running 且无 `log_path`，按钮禁用 + tooltip 提示。
- 新增"回到底部"按钮（暂停跟随时显示）。
- 新增 `api.ts getJobLog(jobId, offset, limit)`。

**前端 `ImagesPage.tsx`**：

- 日志展开改为：点击"构建日志"时调 `getImageLog(envHash)`（新 API），不再直接渲染 `img.build_log`。
- 新增 `mgr` API：`GET /api/images/:env_hash/log` → 从该 image 关联的最近一次成功/失败 job 的落盘文件返回完整日志；若无落盘文件，fallback 到 `images.build_log` 列。
  - 需要 images 表新增 `job_id` 列记录关联 job，或查询 jobs 表按 sandbox + kind 匹配；**采用更简单的方案**：images 表新增 `job_id INTEGER` 列，构建完成/失败时一并写入。

**样式**：

- `.job-log` 与 `.img-log` 容器高度保持现有，新增 `.job-log-controls`（按钮行）样式。

### 数据结构变化

- `JobShared` 增 `log_path: Option<String>`。
- jobs 表增 `log_path TEXT`。
- images 表增 `job_id INTEGER`。
- 前端 `Job` 类型增 `log_path?: string | null`。
- 前端 `Image` 类型增 `job_id?: number | null`。

---

## R4 自定义工具 L5 化（修正 3，横跨 config + mgr + 前端）

用户已拍板的三条决策：
1. **同一镜像 + 整层标记**：env 里出现任意 L5 工具 id ⇒ 该工具集合的标记字节进 env_hash（不是按工具逐字节）。重命名工具会让选中它的在用镜像失效，可接受。
2. **基线只含 L1–L4**：`make build-base` / `aio-config gen`（无 mgr 参与时）的 base 不含任何 user tool；`.aio/enabled.toml` 里的 `"user-tools"` 条目失效（该 scenario 目录将删除）。
3. **勾掉的工具完全不装**：只有被选中的 L5 工具的 fragment 拼进该沙箱的 `Dockerfile.base`。

### 数据模型

**自定义工具 = 一个独立的 L5 scenario 目录**：

```
scenarios/<tool-id>/
├── scenario.toml        # id/name/description/category="service"/installer="custom"/always_on=false
└── fragment.Dockerfile  # 该工具的安装片段（WebUI 编辑器维护的内容）
```

- `<tool-id>` = 规范化后的工具名（`^[a-z0-9][a-z0-9-]*$`，小写、空格→`-`）。与 L1–L4 scenario 同构，因此 `GET /api/scenarios` / `EnvPicker` / `env_hash` / `describe_combo` / 镜像复用全部零特判。
- `category = "service"` 命中 `category_rank` 的既有 `"service"` 槽（`scenario.rs:99` 的 `CATEGORY_ORDER` 第 5 位），env_hash / gen 排序自然落在 L4 之后。
- `installer = "custom"`：EnvPicker 的 L5 区不渲染 rung 子标题（L5 全是 custom，单 installer 保持 flat，走 `layerRows` 的既有 `rungs.length < 2` 分支）。
- **不做版本**：scenario.toml 无 `[[versions]]`，EnvPicker 的 L5 行只有勾选框（既有 `s.versions.length > 0` 分支不渲染下拉）。
- `always_on = false`，与其他可选场景一样经 `env.scenarios` 传递、进 `to_manifest_checked` 校验。

**`scenarios/user-tools/` 目录删除**：`user-tools` 不再是 scenario；`config/src/manifest.rs` 的 `expand` 通配符（`"*"` → 所有非 always_on）会自动把磁盘上每个自定义工具目录展开进去——`Enabled::expand` 无需特判。`.aio/enabled.toml` 的 `"user-tools"` 条目变成悬空 id，gen 对未知 id 本就 loud fail；由 migration 清理（见下）。

### mgr 后端

**`mgr/src/envhash.rs`**：

- `SandboxEnv` 增 `user_tools: Vec<String>`（`#[serde(default)]`，canonical_json 里排序去重）。
- `to_manifest_checked`：user_tools 里的每个 id 必须是一个 `category == "service"` 的 scenario（存在且确实是自定义工具）；**不**加入返回的 `manifest.scenarios`（L5 工具不进 gen 的常规 fragment 解析路径）。
- 新增 `assemble_with_tools(repo, &env) -> Result<String>`：在 `assemble_for` 的结果上，追加每个 user_tool 的 fragment（按 id 排序，fragment 前插 `\n`——与 `assemble` 的分段一致）；最后追加整层标记行 `\n# >>> aio user-tools >>>\n# tools: <sorted ids comma-joined>\n# <<< aio user-tools <<<\n`（**只有 env.user_tools 非空才追加**）。
- `env_hash` 改为对 `assemble_with_tools` 的结果取 sha256（`run_create` 传入）。
- `describe_combo(env, services)`：env.user_tools 非空时追加 `[tools: id1,id2]` 段。

**`mgr/src/jobs.rs`**：`run_create` 里 `let dockerfile_base = envhash::assemble_with_tools(&repo, &env)?; let hash = envhash::env_hash(&dockerfile_base);`（替换现在的 `assemble_for` + `env_hash` 两步）。

**`mgr/src/routes.rs`**：

- `normalize_services`：保留 body.env.user_tools（不做 pi 那种折叠）；PUT-recreate 路径上把当前行的 env_json 里已有的 user_tools 与新 env 合并（user_tools 在 env editor 里可见可改，但 PUT 只显式传 env 时不能丢）。
- `get_user_tools`：从 `scenarios/*` 里收集所有 `category == "service"` 的工具，返回 `tools: [{id, name}]`（驱动"当前包含 N 个工具"的展示，以及让 UI 知道有哪些工具）。
- `put_user_tools`：保存后**同步 `scenarios/` 目录**——对每个 `# --- tool: <name> ---` 标记块：规范化 id，确保 `scenarios/<id>/scenario.toml`（category=service）+ `fragment.Dockerfile`（该标记块内容）存在且与块内容一致；**删除**磁盘上有、但当前文件里已没有对应标记块的 `scenarios/<service>/` 目录。让 `GET /api/scenarios` 立即反映新工具集（AC7）。

**`mgr/src/usertools.rs`**：新增 `sync_scenario_dirs(repo, content) -> Result<Vec<String>>`（返回当前工具 id 列表），`write_fragment` 成功后调用。

### config crate

- `scenarios/user-tools/` 目录删除（其 fragment 内容由 migration 搬到下面的 per-tool 目录）。
- `.aio/enabled.toml`：移除 `"user-tools"` 条目。
- `config/src/gen.rs` / `manifest.rs`：无代码改动（L5 工具不进 gen 的常规路径，`expand` 通配符自动覆盖新工具目录）。

### 迁移（一次性 repo 状态变更，随 commit 落地）

- 把现有 `scenarios/user-tools/fragment.Dockerfile` 里已有的每个工具标记块，迁到 `scenarios/<tool-id>/fragment.Dockerfile`（per-tool），各配 `scenario.toml`。
- 删除 `scenarios/user-tools/`。
- 更新 `.aio/enabled.toml`（删 `"user-tools"`）。

### 前端

**`mgr-web/src/types.ts`**：

- `SandboxEnv` 增 `user_tools?: string[]`。
- `CreateBody` 增 `user_tools?: string[]`（可选，向后兼容）。

**`mgr-web/src/pages/EnvPicker.tsx`**：

- `LAYERS` 增 `{ cats: ["service"], labelKey: "layL5" }`（排在 L4 之后）。
- L5 分组里的 scenario 行：版本下拉不渲染（无 versions），rung 子标题不渲染（单 installer），复用既有 `row` / `layerRows` / `group`——零特判。

**`mgr-web/src/pages/CreatePage.tsx`**：

- `env` state 的 `SandboxEnv` 增 `user_tools: string[]`（初始 `[]`）。
- `foldServices` / `envKey`：把 `user_tools` 纳入 envKey（排序后 join），保证 L5 选择参与镜像复用匹配。

**`mgr-web/src/pages/EditPage.tsx`**：`sortEnv` 比较纳入 `user_tools`。

**`mgr-web/src/i18n.ts`**：增 `layL5`（zh："L5 · 自定义工具" / en："L5 · Custom tools"）。

### describe_combo 兼容性

`describe_combo` 追加 `[tools: ...]` 段后，`ImagesPage` 的 `comboKey`（`combo.match(/^(.*) \[svc: [^\]]*\]$/`）和 `CreatePage` 的镜像复用匹配会失配（尾部多了 ` [tools: ...]`）。需要同步更新这两处，让 `[tools: ...]` 段参与 envKey（或在 comboKey 解析里剥掉）。**选择**：`comboKey` / `envKey` 都纳入 user_tools——保持"env_hash 表面 = 场景 + 版本 + L5 工具集合"的一致性。

### 数据结构变化汇总

- `SandboxEnv`（前后端镜像类型）：增 `user_tools: Vec<String>` / `user_tools?: string[]`。
- `UserToolsFile`：`{content, tools: [{id,name}], path}`。
- 删除 `ToolEntry` / `ToolInstall`（前后端）+ `PUT /api/user-tools/entries`。
- `scenarios/` 下新增若干 `scenarios/<tool>/`；删除 `scenarios/user-tools/`。

---

## 共享约束

- 所有 mgr 路由继续走现有 `ApiError` 风格（400 校验 / 500 anyhow）。
- 所有 mgr-web 新文案进 `i18n.ts`（zh + en 同步）。
- 新 UI 样式统一进 `styles.css`，不引入新组件库。
- mgr + config 改动需通过 `cargo test`；mgr-web 改动需通过 `npm run build`。
- 数据库迁移继续使用现有的 pragma-probe + ALTER 模式，保证幂等。
