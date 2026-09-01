# Quality Guidelines

## Doc comments are mandatory

Every module starts with a `//` block comment explaining what it is and why.
Every public item has a `///` doc comment. See any file in `app/src/` for the
pattern (e.g. `config.rs`, `pty.rs`). Comments explain *why*, reference design
sections (`design §N`), and call out non-obvious decisions (e.g. the three-route
`/api` `/api/` `/api/*rest` pattern in `main.rs`).

## No bare unwrap/expect in handlers

`unwrap()`/`expect()` are only acceptable at startup (see error-handling.md) or
on values that are structurally guaranteed. Runtime paths use `?`, `if let`,
or log-and-continue.

## Testing

- **No unit tests currently.** Compile proof = the image builds; behavior is
  verified by the integration checklist in the task `implement.md` (AC1-AC5)
  and the `web/smoke-test.cjs` end-to-end check.
- When adding tests: `cargo test`; place unit tests in `#[cfg(test)] mod tests`
  at the bottom of the module. Prefer testing the pure functions (`config.rs`
  manifest building, `pty_err` conversion).

## Build validation

`cargo build --release` must pass; `app/Dockerfile` runs version checks
(`Xvnc -version`, `chromium --version`, etc. in the vnc image) as build-time
gates. `services.toml` edits require an app rebuild (`include_str!`).

## Style

- `use` grouping: external crates, blank line, `crate::`.
- 4-space indent; snake_case for fns/vars, PascalCase for types.
- Match the existing comment density (high - this codebase documents reasoning
  heavily).
