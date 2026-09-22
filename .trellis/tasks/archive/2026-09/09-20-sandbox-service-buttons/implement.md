# 实施清单:沙箱服务开关依赖展示与工作区按钮联动修复

按序执行;每步末尾有验证命令。设计依据 design.md(D1–D4)。

## 0. 前置

- [x] 分支 `fix/minimal-sandbox-service-buttons`(自 origin/main)
- [x] 测试沙箱 min1(四服务全关)与 dev1(全开)已在运行,作为 AC 对照组

## 1. D3 — app/services.toml 探测目标 localhost

- [ ] `app/services.toml`:codeServer `target = "localhost:8200"`,
      vnc `target = "localhost:6080"`,piWeb `target = "localhost:30141"`
- [ ] 同步更新三处行注释 + 头注释(`target` 字段说明补"localhost 只反映
      本沙箱;`app` 别名在共享 aio-mgr-net 上跨沙箱碰撞,勿回退")
- 验证:`cargo test -p aio-app`(若 app 是独立 crate 则对应包名;manifest
  解析用例不涉及 target 值,应全绿)

## 2. D4 — mgr composegen 唯一别名 + 路由裁剪

- [ ] `render_compose`:app 的 sandbox-net 增加 `aliases: [sbx-{name}-app]`
- [ ] `render_caddyfile(services)` 化:未安装不生成对应 handle_path 块;
      反代目标 `sbx-{name}-app:<port>`;`generate()`/`write` 链路传参
- [ ] 单测(composegen.rs tests 模块,对齐现有 `compose_omits_services_when_disabled` 风格):
      - 全关:compose 无 code-server:/vnc: 服务块,Caddyfile 无
        `/code-server/`、`/vnc/` 路由,catch-all 反代带 `sbx-t1-app`
      - cs-only:有 code-server 路由无 vnc 路由
      - 全开:两路由 + 唯一别名 + `app:` 裸别名不再出现在 Caddyfile
- 验证:`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`(mgr)

## 3. D2 — mgr-web 工作区按钮安装门控

- [ ] `SandboxTree.tsx`:`buttonsOf` 增加 installed 参数 + `INSTALLED_GATE`
      映射(设计 D2 代码);调用点传 `sb.installed_services`
- [ ] 确认 Terminal / deletable(用户注册)不在门控表(回归保护:
      遍历 gate 表仅四个 id)
- 验证:`cd mgr-web && npm run build`(tsc --noEmit + vite build)

## 4. D1 — ServicesPicker 级联 + 依赖提示

- [ ] `ServicesPicker.tsx` `set()`:pi/vnc 关闭且 pi_web 开 → 级联关 pi_web
      (设计 D1 代码);删除原静默 return 分支
- [ ] pi / VNC 行:pi_web 开启时渲染 `svcDependedBy` 标签(复用 `.lock`
      样式;readonly 不渲染)
- [ ] `i18n.ts`:zh `svcDependedBy: "pi Web 依赖此服务"` /
      en `"required by pi Web"`(插到 svcPiWebDep 旁)
- 验证:`npm run build`;手工对照 AC1

## 5. 端到端实测(宿主 docker 均在运行)

- [ ] 重建 min1(删除后按同参数重建),使其 compose/Caddyfile 换代:
      `curl -X DELETE .../api/sandboxes/min1` → 重新 POST(同参数)
- [ ] app 镜像换代(services.toml 变更,env-hash 不变不会自动重建):
      `docker build --build-arg BASE_IMAGE=sandbox-base-96077d02d2dd(...) -t sandbox-app-96077d02d2dd app/`
      (以 jobs.rs 同参数为准,见 mgr/src/jobs.rs build 调用),然后
      `docker compose -p sbx-min1 -f <instance>/compose.yml up -d --force-recreate app`
- [ ] AC2:`GET sbx-min1…/api/manifest` → codeServer/vnc/piWeb enabled=false;
      mgr-web 工作区树 min1 节点仅 Terminal + 注册
- [ ] AC4:min1 网关 `/code-server/`、`/vnc/vnc.html` 不再 200 代理 dev1
      (404/502 或路由缺失)
- [ ] AC5:dev1 四按钮、code-server 按需启动、Chromium/pi Web 正常
      (dev1 的 app 镜像同 hash 时也需手动换代一次;观察点:manifest
      enabled 仍 true)
- [ ] min1 测试沙箱收尾保留(dev1/min1 对照组留给用户验收后自行删)

## 6. 质量门(最后一轮全量)

- [ ] mgr:`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] app:`cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] mgr-web:`npm run build`
- [ ] trellis-check 子代理全量校验(对照 prd AC1–AC6)

## 7. 收尾

- [ ] spec 回写(`trellis-update-spec`):sandbox-mgr-ops.md 增补
      "共享网络服务别名唯一性"契约;aio-env-config 技能的
      compose-registry 参考同步(服务按钮与安装集联动)
- [ ] 提交(Phase 3.4):单 commit 或按 D1–D4 分块;推送 + PR

## 回滚点

- 每步独立可回滚;D4 对存量沙箱零影响(实例文件不回写)。
- 端到端失败时优先回滚单步(D3 target 改动影响面最小,先排查它)。
