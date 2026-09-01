# Type Safety

## Strict TypeScript

`tsconfig.json` is `strict: true` with `noUnusedLocals`, `noUnusedParameters`,
`noFallthroughCasesInSwitch`. `tsc --noEmit` runs in `npm run build` - type
errors fail the build.

## The manifest contract

`types.ts` defines `ServiceEntry` and `Manifest`, matching the backend's
`/api/manifest` JSON (`app/src/config.rs::ManifestEntry`). These are the
shared types across the HTTP boundary.

## Cross-boundary decoding: one type guard, no inline casts

golden-layout's `componentState` is an opaque `JsonValue`. Decode it back to
the pane payload `{ service, seq? }` through the SINGLE type guard
`isServiceEntry` / `readPaneState` in `App.tsx`:

```ts
function isServiceEntry(v: unknown): v is ServiceEntry { ... }
function readPaneState(state: JsonValue | undefined): { service: ServiceEntry; seq?: number } | undefined { ... }
```

`seq` is the per-service instance number behind tab titles ("Terminal (2)");
it must pass a `typeof seq === "number"` check inside the decoder, never a
cast at the call site. Never `as ServiceEntry` inline - the manifest payload
has one owner on each side of the boundary (cross-layer-thinking-guide).

## `fetch` typing

Type the response via `r.json() as Promise<Manifest>` after checking `r.ok`
(see `App.tsx`). Do not consume `.json()` without the `ok` guard.
