# gateway 入口端口 .env 可配 (AIO_GATEWAY_PORT)

对应 GitHub issue #16。

## Goal

gateway(caddy)的对外监听端口 8080 目前硬编码在 Caddyfile 与 docker-compose 两处,是系统中最后一个无法通过 `.env` 配置的对外端口(对照 #3 的 `PI_WEB_HOST_PORT` 已 env 化)。新增 `AIO_GATEWAY_PORT` 环境变量使其可配,默认 8080,行为完全向后兼容。

## Requirements

- R1 `gateway/Caddyfile` 站点地址由 `:8080` 改为 caddy env 占位符 `:{$AIO_GATEWAY_PORT:8080}`
- R2 `docker-compose.yml` gateway 服务:
  - ports 映射改为 `"${AIO_GATEWAY_PORT:-8080}:${AIO_GATEWAY_PORT:-8080}"`(两侧绑定同一端口,与 `PI_WEB_HOST_PORT` 的配对约定一致)
  - env_file 或 environment 传递 `AIO_GATEWAY_PORT` 进容器(caddy 占位符在容器内展开)
- R3 `.env.example` 增补 `AIO_GATEWAY_PORT` 说明(含默认值与 sbx 场景的配对提示)
- R4 文档同步:README(中英)与 `docs/wiki/` 中出现硬编码 8080 的部署说明处标注可通过 `.env` 修改

## Constraints

- 不改动任何 app(axum)代码:前端 URL 全部为网关相对路径或由 `window.location`/Host 头推导,入口端口变化对 UI 零影响(已核实 services.toml / redirect/index.html / paneUrl.ts)
- 不改 `PI_WEB_HOST_PORT` 既有机制
- mgr 栈(`mgr/compose.yml`,80 端口)不在本任务范围

## Acceptance Criteria

- [ ] AC1 不设 `AIO_GATEWAY_PORT` 时,`docker compose config` 输出与改动前一致(默认 8080,Caddyfile 展开 `:8080`)
- [ ] AC2 `AIO_GATEWAY_PORT=8082` 时,`docker compose config` 显示 `8082:8082`;`make up` 后 caddy 实际监听 8082,主 UI / code-server(`:8082/code-server/`)/ vnc 面板均正常
- [ ] AC3 gateway 容器内 `AIO_GATEWAY_PORT` env 可见(caddy 占位符展开依赖它)
- [ ] AC4 `.env.example`、README(中英)文档已更新,无遗漏的硬编码 8080 部署说明
