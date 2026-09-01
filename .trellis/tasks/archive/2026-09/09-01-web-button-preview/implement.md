# Implement — 用户注册 web 类型按钮 + /preview 动态反代

前置:design.md 定稿;分支 `feat/andes`(当前分支,满足 issue 约定)。

## 执行清单(按序)

### Step 1 后端数据模型 + manifest(config.rs)

- [ ] `ButtonDef` 增 `port: Option<u16>`(`#[serde(default, skip_serializing_if = "Option::is_none")]`)。
- [ ] `load_buttons`:web 分支产出 `target: Some("127.0.0.1:{port}")`、`url: Some("/preview/{port}/")`、`cmd: None`;port 为 None 的 web 按钮(手工坏档)跳过 + warn。
- [ ] 单测:web 按钮解析(target/url/port);旧格式无 port agent 按钮兼容;web 无 port 容错。
- 验证:`cargo test -p aio-app`。

### Step 2 注册 API(buttons.rs)

- [ ] `ButtonInput` 增 `#[serde(default)] button_type: String`(缺省 agent)+ `port: Option<u16>`。
- [ ] 校验矩阵见 design §3;`ButtonOut` 同步 port(web 时携带)。
- [ ] web 入库 `cmd=""`、`port=Some(p)`;agent 入库不带 port。
- [ ] 单测:校验矩阵 + agent 回归(slug/unique_id 既有测试保持绿)。
- 验证:`cargo test -p aio-app`。

### Step 3 /preview 反代(preview.rs 新文件 + main.rs 路由)

- [ ] 新增 `app/src/routes/preview.rs`:三条路由 handler(HTTP 流式转发 + WS 双向泵,design §4)。
- [ ] `Cargo.toml` 增 `tokio-tungstenite`(default-features = false)。
- [ ] main.rs 注册三条 `/preview/:port` 路由(置于 seam 组旁,注释风格一致)。
- [ ] 单测:port 解析拒绝表(0/8088/非数字)、hop-by-hop 剥离、前缀 strip 路径拼接。
- 验证:`cargo test -p aio-app` + `cargo clippy`。

### Step 4 前端表单 + API 镜像(web/)

- [ ] `types.ts`:补 `RegisterButtonInput`。
- [ ] `RegisterDialog.tsx`:类型 segmented control + web 态端口输入与校验(1–65535,≠8088)。
- [ ] `App.tsx`:`registerButton` 签名改造,body 按 type 组装。
- [ ] `i18n.ts`:zh/en 新文案(类型选择、端口 label/placeholder/hint/错误)。
- 验证:`cd web && npm run build`(tsc 零错)。

### Step 5 集成验证(make up,docker 环境)

- [ ] 终端起 `python3 -m http.server 9000 --directory /tmp` → 注册 web 按钮 :9000 → 按钮出现,预览可浏览(相对资源场景,零配置)。
- [ ] 起 vite dev(配 `base: '/preview/5173/'` + `server.hmr.path`)→ 页面加载,HMR 生效(改文件浏览器热更)。
- [ ] SSE 冒烟:任一 SSE 端点在预览下持续输出不缓冲。
- [ ] 停掉 dev server → 刷新后按钮隐藏;删除按钮生效;agent 按钮注册/删除不回归。
- 验证记录追加到本文件末尾。

### Step 6 文档 + 收尾

- [ ] README.md / README.zh-CN.md:Status 一节移除 "user buttons terminal+command only" 缺口;架构表 app 行提及 `/preview/:port` 反代;新增 dev server 预览小节(vite `base`/`hmr.path` 配置示例、已知边界:根绝对资源需上游配合)。
- [ ] spec 更新(`.trellis/spec/backend/api-contracts.md` 增 POST /api/buttons web 语义 + /preview 反代;Phase 3.3)。
- [ ] 提交(Phase 3.4),push,`gh pr create --fill` → `Closes #1`。

## 回滚点

- 每 Step 一个 commit(或 Step1+2 合并),任一 Step 失败 `git revert` 单步即可;buttons.toml 无迁移,回滚零残留。

## 关键验证命令

```bash
cargo test -p aio-app --manifest-path app/Cargo.toml   # 后端单测
cargo clippy --manifest-path app/Cargo.toml            # lint
cd web && npm run build                                 # 前端 tsc + build
make up PROFILES="code-server vnc"                      # 集成环境
```

---

## 验证记录(2026-09-01)

- `cargo test`:**234 passed, 0 failed**(config.rs 4 个新单测、buttons.rs 校验矩阵 5 个、preview.rs 3 个纯函数单测)。
- `cargo clippy`(rust:1-bookworm, clippy 1.98):改动文件零警告(doc 格式 + useless_conversion 已修)。
- `npm run build`(node:20):tsc --noEmit + vite build 零错误。
- 聚焦集成测试(/tmp/preview-itest/setup.sh,rust:1-bookworm 容器内,release 二进制):**21/21 PASS** ——
  web 注册 201 + 响应含 type/port/cmd=""、legacy agent 注册 201 不回归、校验矩阵(无 port/0/8088/未知 type/无 cmd 均 400)、
  buttons.toml 持久化含 port 字段、manifest 下发 url=/preview/9000/ + deletable、TCP 探活(agent command_exists 语义不回归)、
  代理 HTTP(root/子路径/POST 透传)、502(死端口)/404(非数字端口/8088 自环)、SSE 无缓冲流式、
  WS echo 往返 + 子协议回传(vite-hmr)、DELETE 204 + manifest 消失。
- 集成测试过程中发现并修复一个真实缺陷:上游 101 协商的 `Sec-WebSocket-Protocol` 未回传浏览器(vite HMR 会挂),
  已在 proxy_ws 中从上游响应读取并回填。
- 全栈 `make up` 浏览器手工验收未做(本沙箱无 docker compose 栈与浏览器);由聚焦集成测试覆盖等价链路(API/manifest/代理/WS/SSE/删除)。
