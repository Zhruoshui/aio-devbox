# S3 设计：镜像页增强——组合说明 + 删除 + 清理（D3）

## 0. 现状事实（勘察结论）

- **images 表**（db.rs:63-68）：`env_hash PK, tag=sandbox-base-<h[:12]>, built_at, build_log`。无 combo 列。
- **写入点**：jobs.rs:199 `run_create` 中 `db::upsert_image(&conn, &hash, &base_tag, &base_build_log)`。此刻 `env: SandboxEnv` 与 `services: db::Services` 都在作用域，**组合描述在此一次性派生最自然**（不用运行时反算）。
- **db::Services**（db.rs:117-121）：只有 `code_server: bool, vnc: bool`；**pi/pi-web 是 scenario**（S1 normalize_services 折叠进 env.scenarios），描述里 pi/pi-web 必须从 `env.scenarios` 推导。
- **镜像组**：`envhash::image_tags(hash)` → `(base, app, cs)`；`VNC_TAG="sandbox-vnc"` 全局共享、无 images 行。单删/清理按组删 base/app/cs，**不碰 vnc**（它被所有开 vnc 的沙箱共享，且无 refcount 可查）。
- **refcount**：`db::image_refcount(conn, env_hash)` = `COUNT(*) sandboxes WHERE env_hash`。
- **list_images**（routes.rs:1152）：返回 `{env_hash, tag, built_at, refcount, build_log}`，handler 是 async（可 await docker inspect）。
- **docker.rs**：有 `run_capture`（私有）+ `image_exists`；**缺 rmi / size / builder prune**。
- **job 范式**：`spawn_create`/`spawn_delete`（jobs.rs:31/278）：`db::insert_job(kind, sandbox)` → `JobShared` 入 `state.jobs` → `tokio::spawn(run_*)` → 结束 `persist_job`。Job kind 目前 `"create"|"recreate"|"delete"`（state.rs）。
- **ImagesPage**：只读表（96 行），有 `Image` 类型 `{env_hash, tag, built_at, refcount, build_log}` + `listImages()`。

## 1. 数据模型

### 1.1 images 表增 combo 列（R1）

```sql
ALTER TABLE images ADD COLUMN combo TEXT;  -- 幂等迁移，仿 services_json 范式
```

- `init_schema` 尾部加 pragma 检查（`pragma_table_info('images')` 无 `combo` 则 ALTER），与 services_json 的迁移同款。
- `upsert_image` 签名增 `combo: &str`（5 参）；INSERT/ON CONFLICT 都写 combo。
- `list_images` 返回增 `combo: Option<String>`（旧行 NULL）。
- 新库 `CREATE TABLE` 直接带 combo 列；旧库靠迁移。

### 1.2 组合描述格式（envhash.rs 新函数）

```rust
/// 例："node@20+python (cs,vnc,pi,piweb)" / "(base) (vnc)"
pub fn describe_combo(env: &SandboxEnv, services: &db::Services) -> String
```

- 场景：`env.scenarios`（注意 pi/pi-web 也在里面——S1 折叠后场景即选中集）join("+")；空 = `(base)`。
- 版本：`env.versions` 中 `id@label` 追加（BTreeMap 顺序）。
- 服务：`services.code_server→"cs"`、`services.vnc→"vnc"`、`env.scenarios∋"pi"→"pi"`、`∋"pi-web"→"piweb"`；未勾选省略。全开显示 `(cs,vnc,pi,piweb)`，全关显示 `()` 或干脆省略服务括号——**定稿**：场景+版本为主，服务用 `+` 后缀 `(cs,vnc,pi,piweb)`，全关省略括号。

**格式定稿**：`<scenario+...> [<id>@<label>...] [(cs,vnc,pi,piweb)]`
- 例：scenarios `[node, pi, pi-web]` + versions `{node: 20}` + services `{code_server:true, vnc:true}` → `node+pi+pi-web node@20 (cs,vnc,pi,piweb)`。
- 空 scenarios + 空 services → `(base)`。旧行 combo NULL → 前端回退 `env_hash[:12]`（R1）。

## 2. docker 原语（docker.rs）

```rust
pub async fn image_rmi(tag: &str) -> Result<String>          // docker rmi -f <tag>
pub async fn image_size(tag: &str) -> Result<u64>            // docker image inspect --format {{.Size}}
pub async fn builder_prune() -> Result<String>               // docker builder prune -f
```

- `image_rmi` 用 `-f`（组内 FROM 链，非强制会报 "image is being used by"）。**安全**：只允许删除 `sandbox-` 前缀、且必须是 `image_tags(hash)` 产出的三个 tag（白名单校验函数 `is_deletable_image_tag(tag, hash)`）——不允许任意 tag 注入 rmi（防误删宿主/用户镜像）。
- `image_size` 对不存在 tag → Err（调用方 image_exists 先跳）。

## 3. 删除 / 清理 job（jobs.rs）

复用 spawn_create/spawn_delete 的 job 骨架。job kind：`"image-delete"` / `"image-cleanup"`（state.rs 仅 String，无需枚举改动；前端 Job kind union 需扩）。

### 3.1 单删（R3）：`POST /api/images/:env_hash/delete`

- **预检**（handler 同步）：row 存在（404）；`image_refcount > 0` → 409（R3 运行中引用亦覆盖——create/recreate 沙箱在 run_create 步骤 6 `update_sandbox_config` 已写 env_hash，refcount 即含构建中的沙箱）。**不过度设计**：不扫 jobs map；refcount=0 即可删。
- job `run_image_delete(env_hash, log)`：
  1. **执行时再查 refcount**（竞争窗口兜底）>0 → 中止 error。
  2. `image_tags(hash)` 三 tag；逐个 `image_exists` 跳过不存在 → `image_rmi`。
  3. 全程 append_log（含"已删 X / 跳过 Y"）。
  4. **任一 rmi 失败**：中止并报 error（报告已删项与失败项，R5「失败不半删」→ 前面已删的缓存收不回来但如实报告；DNS 行保留，下次 upsert 重建）。DB 行**不删**（下次构建 upsert 会重建——避免删行后组合没记录）。
  5. 全成功 → `db::delete_image_row(conn, env_hash)`。
  - **不删 vnc**（全局共享无行可删）。
- 响应 `202 {job}`，前端轮询 job log。

### 3.2 一键清理（R4）：`POST /api/images/cleanup`

- job `run_image_cleanup(log)`：
  1. `list_images` 全部行，`refcount` 逐个算，收集 refcount=0 的行 → 逐行按 §3.1 同法删组（每行独立 try，单行失败不 abort 整体——报该行失败继续下一行）。
  2. 累计：删了多少组 / `image_size` 求和（rmi 前逐个 size）→ **回收空间（镜像列）**。
  3. `docker builder_prune` → 输出原始行（含 "Total reclaimed space"）→ **回收空间（缓存列）**。
  4. log 汇总行 + 完成。
- 响应 `202 {job}`。

## 4. API 契约

- `GET /api/images`（既有，扩字段）：每行增 `combo: string | null`、`size_bytes: number | null`（实时 `image_size(tag)`，失败/缺失 → null，前端显示 `—`；R2）。
- `POST /api/images/:env_hash/delete` → `{ok, job}`；404 / 409（refcount>0）。
- `POST /api/images/cleanup` → `{ok, job}`。
- 校验 slug：`env_hash` 是 64 hex，由 path 参数强校验（`Path<String>` + 校验函数，非法 400）。

## 5. 前端（ImagesPage）

- 表列：`组合说明`（combo ?? env_hash.slice(0,12)）、`体积`（size_bytes → MiB / —）、既有 tag/refcount/built_at。
- 每行删除按钮：`refcount>0` → disabled + title「被 N 个沙箱引用」；否则可删 → confirm → `deleteImage(env_hash)` → 轮询 job → 刷新。
- 页头「一键清理」按钮 → confirm → `cleanupImages()` → 轮询 job → 展示回收分列（解析 job log 顶部/汇总行）。
- `Image` 类型增 `combo: string | null; size_bytes: number | null`；Job kind union 增 `"image-delete" | "image-cleanup"`。
- api.ts 增 `deleteImage`、`cleanupImages`。

## 6. 兼容与回滚

- combo 列：幂等迁移；旧 mgr 读新 db（SELECT 不选 combo）→ 旧行为；新 mgr 读旧 db → 迁移补列。双向安全。
- 新端点/job kind：旧前端不调；新前端调旧后端 → 404/405（一并部署，无回滚压力）。
- 回滚 = revert commit；combo 列保留无碍（不使用即闲置）。
- R5 失败语义：`image-delete` 单行失败 abort；`image-cleanup` 单行失败报告继续——均如实 log，不假报成功。

## 7. 测试锚点

- db：combo 迁移幂等、upsert 写 combo、list 读回 NULL、delete_image_row。
- envhash：describe_combo 各形态（空/多场景/版本/服务开关全关）。
- docker：is_deletable_image_tag 白名单（允许三个 tag 组、拒绝任意 tag）。
- routes：delete handler 的 404/409、slug 校验。
- 手工（宿主机 make mgr-up）：AC1 组合+体积、AC2 禁用态、AC3 docker images 真删、AC4 分列回收。