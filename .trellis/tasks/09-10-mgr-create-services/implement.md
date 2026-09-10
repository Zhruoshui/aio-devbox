# Implement — S1 创建向导重构

前置：design.md 定案（服务分流机制 / services_json 只存两布尔 / envhash
不动）。顺序：后端数据模型 → API → composegen/jobs/docker → 前端 → 验收。

## Step 1 — DB 迁移 + Services 类型

- [x] mgr/src/db.rs：sandboxes 表 ADD COLUMN services_json（启动迁移，
  IF NOT EXISTS 守卫）；SandboxRow 增字段（Option<String>）；读取 helper
  `services_of(row) -> Services`（NULL → 全 true）
- [x] mgr/src/envhash.rs（或新 services.rs）：`Services {code_server, vnc}`
  结构 + `ServicesBody` 四开关（含 pi/pi_web）
- 验证：`cargo test -p aio-mgr`

## Step 2 — API 归一化

- [x] routes.rs：SandboxBody/PutBody 增 `services: Option<ServicesBody>`；
  `normalize_services(env, body) -> Result<(SandboxEnv, Services)>`：
  pi/pi_web 开关改写 env.scenarios；pi_web=true 强制 pi+vnc（400）；
  put 的 services 忽略（返回提示字段）
- [x] 响应增 services 对象：list_sandboxes / get / entry_url / adopt 行
- [x] 单测：normalize 四用例 + default 全 true + pi_web 联动
- 验证：`cargo test -p aio-mgr`

## Step 3 — composegen + jobs + docker

- [x] composegen.rs：generate 增 services 参；code_server=false 省
  code-server 块；vnc=false 省 vnc 块；单测断言
- [x] jobs.rs：spawn_create 传 services；code_server=false 跳过 cs 镜像
  构建；vnc 影响不了镜像（全局共享）
- [x] docker.rs：up_args 的 profiles 参数化（vnc=true 才带）；调用点更新
  （jobs create/restart、start handler）
- [x] routes.rs service_start：目标服务未装 → 400
- 验证：`cargo test -p aio-mgr`；手动 docker compose config 检查生成的 yml

## Step 4 — 前端

- [x] types.ts：ServicesInput / Sandbox.services
- [x] api.ts：createSandbox/putSandbox 带 services
- [x] i18n.ts：服务区标题/四开关名/依赖提示/L1-L4 节标题（双语）
- [x] EnvPicker：服务开关区（含联动）+ 四层分组 + description + 隐藏
  pi/pi-web 场景
- [x] CreatePage：submit 带 services；SandboxListPage 服务徽章；EditPage
  服务只读区
- 验证：`cd mgr-web && npx tsc --noEmit && npm run build`

## Step 5 — 部署验收

- [x] `make mgr-up` 重建
- [x] AC1 向导形态（服务区 + 四层分组 + 描述）
- [x] AC2 创建无服务沙箱：compose 无服务段、镜像列表无 cs 镜像、面板按钮隐藏
- [x] AC3 依赖校验（前端联动 + 后端 400）
- [x] AC4 旧沙箱（sbx-111）列表/编辑/启动回归
- [x] AC5 全测试绿

## 回滚点

- 各步独立可 revert；DB 列残留无害；镜像哈希不受影响（无重建风暴）

## Review 门

- Step 3 后：trellis-check（backend spec）
- Step 5 后：trellis-check（frontend spec + 全量）

**Step 5 review 门结果（09-10，trellis-check）**：对照 5 份 spec + 父 PRD
D1/D2 审查 18 文件，发现 10 处问题并修复（单测 358→361）：

- P0×4：① ServicesPicker 复选框失效（旧代码把翻译后标签当 key 传 set()，
  受控勾选永远卡住）→ `{key, labelKey}` 修复；② PUT 静默掉 pi/pi-web
  （pre-S1 行 services_json NULL、scenarios 空，重归一化推导成"未装"→
  重建丢服务，AC4 回归）→ `installed_services_of` NULL→四键全开，PUT 用
  当前行形状重折叠；③ 详情/列表 installed_services 对 pre-S1 行显示
  false → 同上全开；④ pi_web=true + pi=false 只 400 vnc 不 400 pi（校验
  不对称，R1/AC3 要求两者都 400）→ `!b.pi || !b.vnc`。
- P1×5：Services 部分 JSON 隐式 false vs "无效→全开"契约→逐字段 serde
  default；jobs 预清理 rm 对无该服务 compose 空跑报错→按开关门控；vnc
  注释自相矛盾+app_builds 单元素循环简化；service_start 无效 match
  分支（`_ => true` 会把未来白名单服务当已装）→实查；`as never` 绕过
  tsc 门→导出 StringKey 类型化标签映射。
- P2×1：gap 4px→design token；EditPage 重复类型→ServicesInput。
- 未修（记录）：entry_url 是纯 URL helper 不加沙箱字段；PUT 不加
  "services 被忽略"提示字段（UI 只读，无消费者）；字段名用
  installed_services（`services` 已被运行时容器列表占用）；400 文案中文
  （mgr 其余错误英文，主用户中文，接受）。

复验：361 全绿 + tsc + vite build；`make mgr-up` 重建部署；实机复核
sbx-111 installed_services 全开 + 向导复选框翻转/pi-web 联动。
