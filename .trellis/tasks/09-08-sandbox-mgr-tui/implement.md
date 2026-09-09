# Implement: 多沙箱 Web 管理界面（sandbox-mgr）

按 design.md 落地。阶段间可独立验证；每阶段末跑该阶段验证命令。
分支：`feat/sandbox-mgr`。

## Phase 0: 地基重构（workspace + crate 抽取）

- [ ] 根 Cargo.toml workspace：members = ["app", "config", "mgr", "aio-models"]。
- [ ] `config/` lib 化：`src/lib.rs` pub mod scenario/manifest/gen；bin 不动
      （`make config` / `make gen` 回归）。
- [ ] 抽 `aio-models` crate：canonical schema + read/write + mask/merge/
      validate（自 app/src/routes/models/{store,catalog}.rs 迁移）；app 改为
      依赖该 crate，删本地副本。
- [ ] 验证：`cargo build --workspace` 通过；`make config`（TUI 冒烟）；
      `make build` 全量构建通过（dep-cache 失效属预期）；现有栈 `make up`
      行为不变。
- [ ] 回滚点：commit 1（纯重构，无行为变化）。

## Phase 1: mgr-api 核心（生命周期 + compose 生成）

- [ ] `mgr/` crate 骨架：axum + rusqlite + tokio；mgr-data/ 初始化
      （state.db schema、gitignore）。
- [ ] docker.rs：compose CLI 封装（up/down/restart/ps --format json、
      network create 幂等、build）。
- [ ] 场景目录 API（GET /api/scenarios，复用 config lib）。
- [ ] env_hash 计算（含片段内容 hash）+ images 表管理。
- [ ] composegen.rs：沙箱 compose 模板生成（design §3.5；镜像全 image: 引用、
      aio-mgr-net 别名、PI_WEB_URL/PI_WEB_ALLOWED_HOSTS、资源 limits、
      无 ports/无 basicauth）。
- [ ] 沙箱 CRUD + start/stop/restart/delete(volumes) + job 队列（构建顺序
      base → app → code-server → vnc，日志入 build_log）。
- [ ] 验证：curl 走通创建→up→ps→stop→delete；`docker compose -f
      mgr-data/instances/sbx-x/compose.yml ps` 手工接管验证（A10）；
      同 env 创建第二个沙箱不重建镜像（A5）；`docker inspect` limits（A8）。
- [ ] 回滚点：commit 2。

## Phase 2: 总网关子域名路由

- [x] mgr 生成 mgr-data/caddy/Caddyfile（每沙箱两站点块，design §2）；
      reload 通道（容器 exec / 本地进程）；失败保留 .bak 并上报状态。
- [x] mgr 栈自身 compose（mgr-gateway + mgr-api + mgr-web 静态）+ `make
      mgr-up` / `make mgr-down`。
- [x] app 侧两处小改：config.rs 支持 PI_WEB_URL 覆盖 piWeb url；
      entrypoint.sh PI_WEB_ALLOWED_HOSTS 默认值化（§2.1）。
- [x] 验证：`http://sbx-x.mgr.localhost/` 打开工作台，terminal/code-server/
      vnc/pi-web 面板全可用、无密码框（A3）；`http://sbx-x-piweb.mgr.localhost/`
      打开 pi-web（A4）；删沙箱后两域名不可达（A7 部分）。
      （09-08 容器内 curl 等价验证全过：工作台/code-server/vnc 200 +
      manifest 全 enabled + pi-web 200 含 /_next 资源 + PI_WEB_URL 覆盖
      生效 + 删除后域名 000/容器清/卷清；浏览器验收留宿主机）
- [x] 回滚点：commit 3（d528c76）。

## Phase 3: mgr-web 管理界面

- [x] `mgr-web/` Vite+React+TS 骨架（复用 web/ token/i18n 模式，不引
      golden-layout）。
- [x] 沙箱列表页（卡片 + 操作 + 入口跳转新标签）。
- [x] 创建向导（场景勾选 + always_on 锁定 + 版本下拉 + 资源输入）+ job
      进度页（构建日志尾部）。
- [x] 环境配置编辑页（PUT → 重建流程）+ 镜像列表页。
- [x] mgr-api 服务 mgr-web/dist。
- [x] 验证：A1/A2 浏览器全流程走通（含构建失败注入一次看错误展示）。
      （09-09 API 等价验证全过：mgr.localhost 经总网关返回 index.html +
      js/css/font 资源 200、/api 与静态共存；cargo test -p aio-mgr 13 过
      （caddy render 增 mgr 站点块测试）；创建向导数据面全走——shell-utils
      场景 + 默认版本 → job ok → 列表 running（A8: docker inspect
      NanoCpus/Memory 与 cpus/mem 一致）；注入失败一次（node 20.18.0 过老
      → pi npm install 挂）→ job error 带 docker build 尾部（A2 失败展示）；
      PUT 编辑（+fonts 场景、cpus 1.5→2、mem 清除）→ 重建 job ok → 新
      env-hash 镜像 + Memory=0（限制清除）；stop/start/restart + 删除
      （volumes=1）后容器/卷/子域名全清。浏览器 A1/A2 人工验收留宿主机）
- [x] 回滚点：commit 4。

## Phase 4: 模型配置上收

- [ ] mgr 侧 /api/models/*（config/discover/test，aio-models 逻辑 + kv 存储，
      契约对齐 app 现有接口）。
- [ ] mgr-web 模型配置页（自 web/src/panes/models/ 移植）。
- [ ] app 侧：MGR_URL 后台拉取任务（启动 + 60s 周期 + 落盘 render）；
      写接口在 MGR_URL 设置时 403 managed-by-mgr；沙箱模型页只读降级。
- [ ] usage 汇总：mgr 定时拉各沙箱 /api/models/usage → /api/usage；
      mgr-web 用量页多沙箱视图。
- [ ] 验证：A6（改 mgr 配置 → 沙箱内生效 + 沙箱内只读）；usage 汇总可见。
- [ ] 回滚点：commit 5。

## Phase 5: 存量纳管 + 收尾

- [ ] 导入向导（adopted 流程，design §3.8：network connect --alias、
      外部 compose 登记、只读式管理）。
- [ ] 存量栈去认证：gateway/Caddyfile 删 basicauth、Makefile hash target
      移除、entrypoint secrets 挂载清理。
- [ ] README/wiki 更新（多沙箱使用、mgr 双形态、安全边界声明）。
- [ ] 验证：A9（存量栈导入后可管理）；`make up` 回归；A7 全量（删除确认 +
      卷清理 + 域名失效）。
- [ ] 回滚点：commit 6（PR merge）。

## 全局验证命令

```bash
cargo build --workspace          # 每 phase
make build                       # Phase 0 后回归一次
make mgr-up                      # mgr 栈
# 浏览器验收 A1–A10 逐条（见 prd.md）
```

## 风险文件

- `app/Dockerfile` / `config/Cargo.toml`（workspace 化）—— Phase 0 回归重点。
- `app/src/config.rs` / `app/entrypoint.sh`（PI_WEB_URL/ALLOWED_HOSTS）——
  存量行为必须默认不变（env 未设时零变化）。
- `gateway/Caddyfile`（去认证）—— 破坏性，README 同步。

## task.py start 前检查

- [ ] prd.md / design.md / implement.md 用户已审。
- [ ] implement.jsonl / check.jsonl 填入真实条目（spec/research 清单）。
