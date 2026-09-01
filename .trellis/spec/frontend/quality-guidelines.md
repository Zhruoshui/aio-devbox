# Quality Guidelines

## Build is the typecheck gate

`npm run build` = `tsc --noEmit && vite build`. A type error or unused var
fails the build. Run `npm run typecheck` for a fast type-only check.

## Linting

No ESLint/Prettier configured yet. Match existing style: 2-space indent,
double quotes for strings, semicolons, trailing commas in multiline. Add ESLint
if the team wants enforcement - until then, `tsc` + code review gate style.

## Testing

- No unit tests. `web/smoke-test.cjs` is a manual end-to-end check
  (puppeteer-core) that loads the workspace and asserts panes render.
- When adding tests: prefer testing pure logic (`buildLayoutConfig`,
  `isServiceEntry`, `readServiceState`) with Vitest before component tests.

## Component discipline

- A new service = a `services.toml` entry, NOT a new component (unless it needs
  a new `type`). See component-guidelines.md.
- Keep imperative-library setup and its cleanup together in one `useEffect`.
- File-level `//` comment on every `.tsx` explaining its role.
