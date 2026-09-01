# Error Handling

## Startup errors -> panic (programmer bug)

Things that can only fail if the source/config is wrong use `expect()` with a
context message and are allowed to panic at startup:

- `tokio::net::TcpListener::bind("0.0.0.0:8088").expect("failed to bind ...")`
  (`main.rs`).
- `toml::from_str::<ServicesFile>(SERVICES_TOML).expect("services.toml is
  invalid - fix the source file")` (`config.rs::load_services`) - `services.toml`
  is a compile-time asset, so a parse error is a programmer bug, not runtime.

## Runtime errors -> log + degrade gracefully

Handlers never panic on runtime failures; they log and continue or close the
session:

- **pty spawn failure** (`routes/terminal.rs`): `tracing::error!`, send a red
  error line to the WS client, then close the socket.
- **pty write/resize failure** (`routes/terminal.rs`): `tracing::warn!`, break
  the session loop (write) or continue (resize - a failed resize is non-fatal).
- **pty read error**: `tracing::debug!`, stop the reader thread.
- **WS recv error**: `tracing::debug!`, end the session.

## Error conversion

`portable-pty` errors are `anyhow::Error` (private when re-exported). Convert
with the `pty_err` helper in `pty.rs` (`io::Error::new(Other, e.to_string())`)
rather than depending on the concrete type.

## Reserved seams

`/api`, `/v1`, `/mcp` (and sub-paths) return HTTP 502 `{"error":"seam reserved"}`
(`routes/seam.rs`) - a deliberate non-code-server response, not a crash.
