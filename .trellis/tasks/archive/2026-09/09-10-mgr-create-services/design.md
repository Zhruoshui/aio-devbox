# Design — S1 创建向导重构：服务四开关 + 场景四层分组

## 核心洞察（决定整个设计的简化）

四服务的"装"由三个不同机制承载，UI 统一、后端分流：

| 服务 | 装的机制 | 已有自由度 |
|---|---|---|
| pi | scenario（scenarios/pi） | 本就可选 |
| pi-web | scenario（scenarios/pi-web） | 本就可选 |
| code-server | compose 服务 + 独立镜像（sandbox-code-server-\<hash\>） | **新增开关** |
| vnc | compose 服务 + 全局共享镜像（sandbox-vnc） | **新增开关** |

因此：**envhash 随服务开关变**——pi/pi-web 的开关就是场景勾选，天然进哈
希；code_server/vnc 是 compose 层，不进哈希反而正确：同场景组合的沙箱无
论开关如何都共享 base/app 镜像（镜像内容确实相同），code-server 镜像在该
组合首次有沙箱要它时才构建。

> **实现期修正（09-10，比原稿更重要）**：原稿「envhash 完全不动 / pi 本就
> 可选」基于事实错误——pi/pi-web 当时 `always_on = true`（与 node/python
> 同属必装基线，issue #8），`to_manifest_checked` 拒绝它们出现在
> env.scenarios（默认全开的创建直接 400）。S1 实际语义（父 PRD D1：组合哈
> 希含服务开关）要求 pi/pi-web **改为可选场景**（`always_on = false`，
> scenarios/{pi,pi-web}/scenario.toml 已翻 + 注释改写，TUI 动态按目录渲
> 染为 L4 可选行）。后果：
> - 全开服务 + 与旧沙箱相同版本 → 装配字节仍与旧 always_on 时代一致 → 同
>   一 hash、复用既有镜像（实测 svcpweb 复用 c3648…，无重建）
> - 关掉 pi/pi-web → 场景集不含 → 新 hash → 首建触发一次 base 构建（每组
>   合缓存，非风暴；父 D1 已授权）
> - app entrypoint 本就按 `command -v pi-web` 守卫（注释明言 optional），
>   未装静默跳过，面板按钮探活自动隐藏；构建期无 pi 依赖
> - 仓库自身 .aio/enabled.toml 显式列出 pi/pi-web → 主栈行为不变

## 数据模型

### DB（mgr/src/db.rs）

sandboxes 表新增列 `services_json TEXT`，内容
`{"code_server":bool,"vnc":bool}`（canonical 序）。迁移：`ALTER TABLE ...
ADD COLUMN`（IF NOT EXISTS 风格守卫，mgr 已有启动迁移模式——沿用）。
旧行 NULL → 读取时默认 `{"code_server":true,"vnc":true}`（与旧行为一致：
无条件构建）。

**只存 code_server/vnc 两个布尔**——pi/pi_web 是场景，存两份即双源真值。
API 层的 services 四开关是 UI 契约，后端归一化（见下）。

### API（mgr/src/routes.rs）

`SandboxBody` / `PutBody` 增加可选字段：

```rust
#[derive(Deserialize, Default)]
struct ServicesBody {
    #[serde(default = "default_true")] code_server: bool,
    #[serde(default = "default_true")] vnc: bool,
    #[serde(default = "default_true")] pi: bool,
    #[serde(default = "default_true")] pi_web: bool,
}
```

归一化（create 与 put 同一 helper `normalize_services`）：
- `pi=false` → 从 env.scenarios 剔除 `"pi"`；`pi=true` → 若无则加入
- `pi_web=false` → 剔除 `"pi-web"`；`pi_web=true` → 加入（并强制 pi=true、
  services.vnc=true，否则 400）
- `code_server`/`vnc` 原样存入 services_json

校验顺序：先归一化 env，再走现有 `to_manifest_checked`。

响应：list/get/entry_url 的沙箱 JSON 增 `services` 对象（四键全出：
code_server/vnc 来自 services_json，pi/pi_web 由 env.scenarios 推导）。

### 生命周期

- `jobs::spawn_create` 签名增 `services: Services`（code_server/vnc 两布尔）
- 构建条件化（jobs.rs）：
  - `code_server=false` → 跳过 `sandbox-code-server-{hash}` 构建（image_exists
    检查照旧——同 hash 早有镜像则复用，无则跳过）
  - vnc 镜像全局共享，无构建开关；`vnc=false` 只影响 compose 与 up profiles
- `composegen::generate(name, hash, cpus, mem, services)`：code_server=false
  省略 code-server 服务块；vnc=false 省略 vnc 块
- `docker.rs up`（:115 UP_PROFILES）：vnc=true 才带 `--profile vnc`；
  `up_args` 增参（调用点：jobs create/restart + start handler）
- `service_start`（按需启动，routes.rs:42）：目标服务不在 services_json 时
  400（"该沙箱未安装此服务"）
- PUT 编辑：env 变更走 recreate（现状），**services 字段忽略并提示不可改**
  （镜像内容决定；code-server 镜像可能没构建）

### 前端（mgr-web）

**CreatePage / EnvPicker 重构**：
- 新「服务」区（EnvPicker 顶部，场景区之前）：四个开关卡片
  （code-server/VNC/pi/pi-web），pi-web 开启时若 pi/vnc 未开则自动联动开启
  （前端即时联动，后端仍有校验兜底）
- 场景区：隐藏 `pi`/`pi-web` 两个 id（服务区接管），其余按 category 分四节：
  `系统 (L1)` / `Shell (L2)` / `语言 (L3)` / `应用 (L4)`（i18n 双语），
  节标题旁显 category 说明；场景卡片加 description 第二行
- `types.ts`：SandboxEnv 保持 {scenarios,versions}；新增 ServicesInput
  类型 + Sandbox 类型增 services 展示字段
- Submit body：`{name, env, services, cpus, mem_mb}`

**SandboxListPage / EditPage**：沙箱卡片/编辑页显示服务组合（四枚小徽章，
装/未装状态）；EditPage 服务区只读。EditPage 的 EnvPicker 同样隐藏
pi/pi-web 场景（服务只读展示）。

**manifest 探测不变**：服务面板按钮可见性已由 app 侧 TCP/命令探测决定
（code-server/vnc 关 → 容器不存在 → 探测失败 → 按钮自动隐藏），零改动。

## 兼容与迁移

- 旧沙箱行 services_json=NULL：读取默认全 true（行为等同现状）
- 旧 mgr-web 对新后端：body 不带 services 字段 → serde default 全 true（现状）
- 新 mgr-web 对旧后端：不发生（同仓库同构建）
- envhash 不变 → **所有现存镜像哈希继续有效**，无重建风暴
- composegen 输出变化只影响**新建**沙箱的实例文件

## 测试

- envhash: 不动（现有 73 测试含其锁定断言）
- routes: normalize_services 单测（pi_web 联动 400 / pi=false 剔除场景 /
  default 全 true / 旧 body 无 services 字段）
- composegen: services 开关的服务块省略断言
- jobs: 构建跳过逻辑单测（mock image_exists 不可行则集成验证）
- 前端: tsc + 现有测试；手动验收 AC1-AC5

## 风险与回滚

- 单 commit revert 即回滚（DB 新列留着无害）
- 风险点：jobs.rs 构建顺序重构牵连 spawn_create 调用方（routes create/
  put-recreate、adopt？——adopt 不走 spawn_create 的构建，只注册，确认无牵连）
