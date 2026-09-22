# S3 执行计划

前置：design.md §1-§7 为实现依据。顺序按数据流（db → envhash → docker → jobs → routes → 前端），每步独立可编译。

## Step 1 db 层（mgr/src/db.rs）

- [ ] `images` 表 CREATE 增 `combo TEXT`；`init_schema` 尾部加 pragma 迁移（`pragma_table_info('images')` 无 combo → ALTER TABLE ADD COLUMN combo TEXT，仿 services_json 范式）
- [ ] `upsert_image` 签名 +combo（5 参），INSERT/ON CONFLICT 写 combo
- [ ] `list_images` 返回 +combo（`Option<String>`，旧行 NULL）
- [ ] 新增 `delete_image_row(conn, env_hash) -> Result<bool>`
- [ ] 单测：combo 迁移幂等、upsert 保留旧 combo（同 A5 语义）、读回 NULL、delete_image_row 真删
- 验证：`cargo test -p aio-mgr db::`

## Step 2 envhash 描述（mgr/src/envhash.rs）

- [ ] `pub fn describe_combo(env: &SandboxEnv, services: &db::Services) -> String`（格式见 design §1.2；pi/pi-web 从 env.scenarios 推导）
- [ ] 单测：空/多场景/版本/服务全关/pi-web 折叠
- 验证：`cargo test -p aio-mgr envhash::`

## Step 3 docker 原语（mgr/src/docker.rs）

- [ ] `pub async fn image_rmi(tag: &str) -> Result<String>`（`docker rmi -f`）+ `is_deletable_image_tag(tag, hash)` 白名单（仅 `image_tags(hash)` 三 tag）
- [ ] `pub async fn image_size(tag: &str) -> Result<u64>`（`docker image inspect --format {{.Size}}`）
- [ ] `pub async fn builder_prune() -> Result<String>`（`docker builder prune -f`）
- [ ] 单测：is_deletable_image_tag 接受组内/拒绝任意
- 验证：`cargo test -p aio-mgr docker::`

## Step 4 jobs（mgr/src/jobs.rs）

- [ ] `run_create` upsert_image 调用改 5 参：`describe_combo(&env, &services)`
- [ ] `spawn_image_delete`（kind "image-delete"，sandbox=None）：job 执行时重查 refcount>0 → 中止；`image_tags` 三 tag 依次 exists→rmi；append_log 报已删/跳过/失败；全成功删行，失败保行报错
- [ ] `spawn_image_cleanup`（kind "image-cleanup"）：遍历 refcount=0 行逐行删组（单行失败报告继续）+ 累计 size + builder_prune + log 汇总
- 验证：`cargo build -p aio-mgr`

## Step 5 routes（mgr/src/routes.rs）

- [ ] `POST /api/images/:env_hash/delete`：404（无行）/409（refcount>0）/202 {job}
- [ ] `POST /api/images/cleanup`：202 {job}
- [ ] `GET /api/images` 增 combo + size_bytes（实时 image_size，失败 null）
- [ ] slug 校验（64 hex）
- [ ] 路由注册
- 验证：`cargo test -p aio-mgr`

## Step 6 前端（mgr-web）

- [ ] types.ts：Image 增 combo/size_bytes；Job.kind 增 "image-delete"|"image-cleanup"
- [ ] api.ts：deleteImage/cleanupImages
- [ ] ImagesPage：组合列（combo ?? hash[:12]）+ 体积列（MiB/—）+ 行删（refcount>0 disabled+title）+ 页头一键清理 + 轮询 job + confirm
- [ ] i18n 新键
- 验证：tsc + build

## Step 7 质量检查

- [ ] `cargo test -p aio-mgr` 全绿
- [ ] `cd mgr-web && npx tsc --noEmit && npm run build`
- [ ] 手工链路（make mgr-up）：AC1-AC4

## Step 8 收尾

- [ ] spec 更新（api-contracts images 契约 + sandbox-mgr-ops 若需要）
- [ ] 提交 + journal

## 回滚点

- 每 Step 独立可编译。combo 列迁移幂等（旧 mgr 忽略）；新端点/job 无数据逆操作 → revert 即回滚。