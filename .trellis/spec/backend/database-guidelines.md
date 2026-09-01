# Database Guidelines

> **N/A - this project has no database.** Documented so future work has a clear
> "nothing here yet" signal rather than guessing.

## Current state

- No ORM, no migrations, no SQL. The app is stateless aside from the in-memory
  `AppState` (built once at startup from `services.toml`) and the shared
  `workspace` Docker volume (filesystem, not a DB).
- Persistence (R6/AC3) is via the shared named volume mounted at `/home/gem`,
  not a database.

## If a database is added later

Replace this file with the actual ORM/migration/query conventions at that point.
Until then, do NOT introduce a DB for state that belongs in `services.toml`
(build-time) or the workspace volume (runtime files).
