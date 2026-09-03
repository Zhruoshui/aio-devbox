# Journal - taosi222 (Part 1)

> AI development session journal
> Started: 2026-09-01

---


## 2026-09-02 — issue #3 piWeb iframe 端口硬编码

- 认领 #3,走 Trellis 任务 `09-02-piweb-port-env`(lightweight,PRD-only)。
- 方案照 issue:通用 `{env:VAR:default}` 占位符(config.rs 展开一次)+
  compose `PI_WEB_HOST_PORT`(默认 30141,同驱 ports 与容器 env)。
- 环境无 Rust 工具链,rustup + gcc 装齐后本地验证:`cargo fmt/clippy/test`
  242 全过(含 5 个新单测);`cargo run` 实测 manifest 30142/30141 两态;
  `docker compose config` 两态校验通过。clippy 既有 warning 不在本任务范围。
- 新 rustfmt 重排了 18 个无关 route 文件,已 checkout 还原保持提交聚焦。
- spec:api-contracts.md 增补占位符契约一节。提交 57bfe46(feat/taosi)。
