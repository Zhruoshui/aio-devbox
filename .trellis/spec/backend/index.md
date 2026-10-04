# Backend Development Guidelines

> Rust (axum) app server for the AIO sandbox. It serves the SPA, a data-driven
> service manifest, a terminal pty WebSocket, reserved seam stubs, and the
> unified model-config route group (`/api/models/*`, see
> [Model Config Guide](./model-config-guide.md)). The canonical store is a
> single JSON file (`~/.aio/models.json`); opencode.db is read read-only for
> usage stats — no write database.
>
> **10-04-cc-switch 重构注记**: app 层(沙箱)仍是 canonical store 模式,
> Phase 2 将切换为 cc-switch 快照/增量模式。**mgr 层已切换**(Phase 1.2/3):
> 不再有 profile 系统/canonical 上收;mgr 只保留供应商库
> (`aio_models::template::ProviderLibrary`,kv 键 `models_library`),沙箱
> 侧配置端点从 `GET /api/models/templates` 拉模板。

## Stack

- **Language**: Rust 2021 edition (`app/Cargo.toml`).
- **Framework**: axum 0.7 (with the `ws` feature) on tokio.
- **Static assets**: `tower-http` `ServeDir` (the React build, baked into the image).
- **Other deps**: `portable-pty` (pty bridge), `serde`/`serde_json`/`toml`/`json5`,
  `reqwest` (json + rustls-tls, for discover/test HTTP), `rusqlite` (bundled,
  read-only opencode.db), `tracing` + `tracing-subscriber`, `futures-util`.

## Where things live

See [directory-structure.md](./directory-structure.md). In short: `app/src/main.rs`
(router), `config.rs` (services.toml + manifest), `state.rs` (AppState +
shared `reqwest::Client`), `pty.rs` (pty bridge), `routes/` (handlers, incl.
`routes/models/` for the unified model config), `app/services.toml` (the pane
registry - single source of truth).

## Guidelines Index

| Guide | Status |
|-------|--------|
| [Directory Structure](./directory-structure.md) | filled |
| [Sandbox-mgr Ops](./sandbox-mgr-ops.md) | filled (DooD 路径同一性、bind-mount inode、pi-web Host 规则、profile 全量、mgr.localhost 静态站点、模型上收同步链与写降级、共享网络别名唯一性/localhost 探测) |
| [API Contracts](./api-contracts.md) | filled (GET /api/stats; /api/models/* in the Model Config Guide; mgr 控制面 API: live 四态、PUT limits 三态、build_log 保留) |
| [Model Config Guide](./model-config-guide.md) | filled (canonical store, renderers, discover/test, usage, key masking) |
| [Database Guidelines](./database-guidelines.md) | read-only opencode.db only - documented |
| [Error Handling](./error-handling.md) | filled |
| [Logging Guidelines](./logging-guidelines.md) | filled |
| [Quality Guidelines](./quality-guidelines.md) | filled |
