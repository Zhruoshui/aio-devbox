# mgr 子域 URL 端口跟随（任意宿主端口作唯一入口）

## Goal

让用户用任意宿主机端口（如 8081）作为系统唯一入口时，mgr-web 生成的所有
`*.mgr.localhost` 子域 URL 自动携带当前浏览器端口，不再写死撞宿主 80。
改完后：宿主机只需发布一个端口给 mgr 总网关，UI/API/code-server/vnc/pi-web
全部经域名分发可达，宿主 80 端口不再被占用。

## Background

- 现状（bug 实测 09-10）：用户宿主机发布 `8081:80` 访问 `http://mgr.localhost:8081/`，
  terminal/pi-agent 面板正常（同源 / 绝对带端口 URL），但 code-server、vnc 面板
  打不开——`paneUrl.ts` 生成的 `http://sbx-<name>.mgr.localhost` 不带端口，
  浏览器默认连宿主 80，而 80 未发布。
- pi-web 已有同类机制（`{host}` 占位符 + `PI_WEB_HOST_PORT`，issue #3），
  本任务把「端口跟随」补齐到子域网关 origin 上。
- 沙箱/沙箱内各服务端口（8080/8088/8200/6080/30141）不动——它们只在
  docker 网络内部使用。

## Requirements

- R1 前端 `paneUrl.ts` 的 `sandboxGatewayOrigin()` 在非 80 端口访问时，
  生成的子域 URL 附加 `window.location.port`（80 端口访问时保持无端口，避免回归）。
- R2 piWeb 绝对 URL（`PI_WEB_URL` 子域形式 `sbx-<name>-piweb.mgr.localhost`）
  同样需要端口跟随——pi-web 独享子域也走总网关，撞 80 同理会挂。
- R3 后端 `routes.rs` 三处返回的 `entry_url` / `piweb_url`（创建应答、列表、
  entry_url 端点）：后端不知道浏览器端口 → 由 mgr-web 前端在展示/打开这些
  链接时补端口（后端保留无端口形式作为默认值，前端负责修正）。
- R4 8080 根指路页 `app/redirect/index.html` 中 `http://mgr.localhost/` 链接：
  MGR_URL 占位符由 entrypoint sed 注入，需确认其在非 80 端口下也正确
  （mgr 生成的 MGR_URL 应含端口，或前端跳转逻辑补偿）。
- R5 既有行为兼容：经宿主 80 访问（`http://mgr.localhost/`）时一切照旧。
- R6 重建 mgr 镜像（web-builder 阶段重打 mgr-web static）后改动生效，
  无需改 caddy / 网络拓扑。

## 执行期追加修复（09-10 下午，同根因）

- R7 pi-web 403：端口跟随上线后用户实测 pi-web 页面报 HTTP 403。根因是
  `caddy.rs` 生成的 piweb 站点 `header_up Host sbx-<name>-piweb.mgr.localhost`
  把 Host 改写成无端口字面量——pi-web 中间件对 `/api/*` 要求
  `Origin == protocol//Host 推导的 origin`，浏览器 Origin 带 :8081 而被改写的
  Host 不带 → "Untrusted API request" 403。修复：去掉 `header_up`，透传浏览器
  原始 Host（pi-web 的名单校验自己剥端口，`.localhost` 后缀天然放行）。
  验证：同源 POST/GET 不再 403（404/400=已过安全层），全链路回归 200，
  cargo test 73/73。

## Out of Scope

- 给每个沙箱发布独立宿主端口（架构上不需要，明确不做）。
- HTTPS / 认证（D9 信任边界不变）。
- 沙箱内部端口（8080/8088/8200/6080/30141）的改动。

## Acceptance Criteria

- [x] AC1 在宿主机以 `http://mgr.localhost:8081/` 打开管理 UI，code-server
      面板能加载（iframe 内 VSCode 界面出现）。
      — 沙箱内等效验证：JS 含端口跟随逻辑 + 子域/code-server/ 链路 200；
      浏览器最终确认待用户。
- [x] AC2 同场景下 vnc 面板能加载（noVNC 界面出现）。— 同上，链路 200。
- [x] AC3 同场景下 pi-web 面板能加载（pi agent 界面出现，URL 形如
      `http://sbx-<name>-piweb.mgr.localhost:8081/`）。— piweb 子域 200，
      响应体确认为真实 Pi Web；withMgrPort 单测覆盖 URL 形态。
- [x] AC4 沙箱列表/创建应答中展示的 entry_url 打开后可达沙箱 8080 指路页
      （或其 mgr 跳转），端口正确。— 指路页已部署注入验证（gate 修复后
      target 替换正确），sed+JS 四场景行为单测通过。
- [x] AC5 经容器网络直接访问（Host 头无端口，等效 80 场景）行为不回归
      （现有 e2e/单元测试通过）。— cargo test 73/73，tsc 通过，
      容器网络 Host 头无端口全 200。
- [x] AC6 `make mgr-up` 重建后容器健康，`http://mgr.localhost:8081/` 全功能可用。
      — mgr 栈重建部署完成，UI/子域链路全 200。
