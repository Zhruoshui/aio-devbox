# Logging Guidelines

## Library

`tracing` + `tracing-subscriber::fmt`. Initialized once in `main.rs`:
`tracing_subscriber::fmt::init()`.

## Levels (as actually used)

| Level | Used for | Example |
|-------|----------|---------|
| `info!` | startup milestones | `"listening on {}"` (main.rs) |
| `error!` | fatal-for-session failures | pty spawn failed (terminal.rs) |
| `warn!` | recoverable failures | pty write/resize failed (terminal.rs) |
| `debug!` | expected connection lifecycle | pty read error, WS recv error |

## Conventions

- Messages are human-readable strings; no structured fields yet (MVP).
- Log at the point of failure, with the error (`{e}`) in the message.
- Do NOT log normal request traffic or keystrokes (the pty bridge carries
  user terminal data - never log pty contents).
