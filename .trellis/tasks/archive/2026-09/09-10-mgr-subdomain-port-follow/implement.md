# Implement — mgr 子域 URL 端口跟随

前置：design.md 已定案（前端 `withMgrPort` 单点补偿 + redirect 页 JS 补端口 +
后端零语义改动）。执行顺序按依赖排列，每步有验证命令。

## Step 1 — paneUrl.ts：withMgrPort + 消费点接线

- [ ] `mgr-web/src/pages/workspace/paneUrl.ts`：
  - 新增 `withMgrPort(url)`（design §1 的实现；注意注释里写清 80/空端口
    直通、显式端口不重复追加）。
  - `sandboxGatewayOrigin()` 返回值过 `withMgrPort`。
  - `urlFor()` piWeb 绝对 URL 分支过 `withMgrPort`（`{host}` 已替换之后）。
- [ ] 测试：查 mgr-web 是否已有 paneUrl 测试文件；有则扩展，无则新增
  `paneUrl.test.ts`，覆盖：无 port、port=80、port=8081、URL 已带显式端口
    （`http://x:30141/` 不被改写）、`{host}` 替换后的绝对 URL。
- 验证：`cd mgr-web && npx tsc --noEmit && npx vitest run`（或项目实际
  测试命令，先看 package.json scripts）。

## Step 2 — entry_url / piweb_url 前端消费点

- [ ] `mgr-web/src/pages/SandboxListPage.tsx:253` 的 `href={sb.entry_url}`
  → `href={withMgrPort(sb.entry_url)}`。
- [ ] `mgr-web/src/pages/models/ModelsPage.tsx` 中 entry_url 消费点（MgrNotice
  链接，约 :146 附近）同样处理。
- [ ] 全仓 grep `entry_url|piweb_url` mgr-web/src 确认无遗漏消费点。
- 验证：tsc + 测试（同 Step 1 命令）。

## Step 3 — redirect 页跳转端口补偿（R4）

- [ ] `app/redirect/index.html`：跳转 JS（mgr 分支）在跳转前把目标 URL 的
  origin 替换为带 `window.location.port` 的形式（port 空则不动）；静态
  `<a id="mgr-link">` 与 noscript 保留无端口回落。
- [ ] `app/entrypoint.sh:58`：sed 目标改为优先 `MGR_REDIRECT_URL`、回落
  `MGR_URL`、再回落 `http://mgr.localhost/`（design §3 定案；MGR_URL 的
  模型 pull 身份不受影响）。
- [ ] `mgr/src/composegen.rs`：sandbox compose env 增
  `MGR_REDIRECT_URL: http://mgr.localhost/`（与 MGR_URL 并列，注释区分
  两者用途）。同步更新其单测断言。
- 验证：`cargo test -p aio-mgr`（composegen 断言）；redirect 页改动是
  纯静态，重建后人工验证。

## Step 4 — 后端注释契约固化

- [ ] `mgr/src/routes.rs` 三处 entry_url/piweb_url（:426/:516/:881）加
  注释：无端口是规范形式（容器网络视角），浏览器端口由 mgr-web
  `withMgrPort` 前端补偿——勿在后端拼端口。
- 验证：`cargo test -p aio-mgr`（回归）。

## Step 5 — 构建与部署

- [ ] `make mgr-up`（重建 mgr 镜像：web-builder 重打 mgr-web static）。
- [ ] app 镜像改动（entrypoint.sh / redirect 页）暂不强制重建 sbx-111
  （design §5 说明 AC 不依赖）；如重建：
  `docker compose -f mgr-data/instances/sbx-111/docker-compose.yml up -d
  --build`（路径以实际为准，先 ls mgr-data/instances/）。
- 验证：`docker compose -p aio-mgr -f mgr/compose.yml ps` 两容器 healthy；
  `curl -s -H 'Host: mgr.localhost' http://localhost:8081/ | head`（经宿主
  8081 路径不可达 from 沙箱内，改为沙箱内 `curl -H 'Host: mgr.localhost'
  http://localhost:80/` 验证 UI 仍 200）。

## Step 6 — 验收（AC1-AC6，宿主机浏览器实测）

- [ ] AC1 `http://mgr.localhost:8081/` → code-server 面板加载（如经 vnc
  puppeteer 截图验证：frontend-verify-live workflow，vnc 容器内 chromium
  访问 `http://mgr.localhost:8081/`）。
- [ ] AC2 vnc 面板加载（同上方式或人工）。
- [ ] AC3 pi-web 面板加载，URL 带 :8081。
- [ ] AC4 列表页 entry_url 链接打开 → 沙箱指路页/跳转正常。
- [ ] AC5 容器网络访问不回归（沙箱内 curl Host 头无端口全 200）。
- [ ] AC6 mgr 栈健康 + 既有测试全绿（`cargo test`、mgr-web 测试）。

## 回滚点

- 前端改动：单 commit revert 即可（mgr-web 静态资源重打）。
- entrypoint/composgen 改动：同理 revert + 重建。
- 无数据库/网络拓扑/卷变更，零迁移风险。

## Review 门

- Step 1-2 完成后：trellis-check 跑前端质量检查（spec: frontend/）。
- Step 3-4 完成后：trellis-check 跑后端（spec: backend/ + cargo test）。
- Step 6 全部 AC 勾完才算完成。
