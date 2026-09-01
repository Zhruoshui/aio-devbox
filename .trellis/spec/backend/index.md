# Backend Development Guidelines

> Rust (axum) app server for the AIO sandbox. It serves the SPA, a data-driven
> service manifest, a terminal pty WebSocket, reserved seam stubs, and the
> unified model-config route group (`/api/models/*`, see
> [Model Config Guide](./model-config-guide.md)). The canonical store is a
> single JSON file (`~/.aio/models.json`); opencode.db is read read-only for
> usage stats — no write database.

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
| [API Contracts](./api-contracts.md) | filled (GET /api/stats; /api/models/* in the Model Config Guide) |
| [Model Config Guide](./model-config-guide.md) | filled (canonical store, renderers, discover/test, usage, key masking) |
| [Database Guidelines](./database-guidelines.md) | read-only opencode.db only - documented |
| [Error Handling](./error-handling.md) | filled |
| [Logging Guidelines](./logging-guidelines.md) | filled |
| [Quality Guidelines](./quality-guidelines.md) | filled |
