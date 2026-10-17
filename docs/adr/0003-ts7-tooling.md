# ADR 0003: TypeScript 7 tooling (type-check and lint)

- Status: accepted
- Date: 2026-10-17

## Context

The UI targets TypeScript 7, the native (Go) compiler. Its stable release
(`typescript@7.0.2`) ships the native compiler as the `tsc` binary, with
per-platform binary packages. Its programmatic API is only exposed under
`typescript/unstable/*`. Tools that import the classic `typescript` API, such as
typescript-eslint, would need TypeScript 6 aliased next to it
(`"typescript": "npm:@typescript/typescript6@^6"`).

## Decision

- Type-check with the TypeScript 7 compiler: `pnpm typecheck` runs
  `tsc --noEmit` from `typescript@7.0.2`. (`tsgo` was the binary name of the
  `@typescript/native-preview` package; the stable package calls it `tsc`.)
- Lint and format with **Biome** (2.5.x). No typescript-eslint and no
  TypeScript 6 alias.
- Biome enforces the repository rules that matter most:
  - `noExplicitAny`, `noDefaultExport` (named exports only; config files exempt);
  - `noRestrictedImports`: `@tauri-apps/api/core` and `src/ipc/bindings.ts` may
    only be imported inside `src/ipc/`.

## Consequences

- One fast tool for lint and format, one dependency tree with a single TypeScript.
- We lose typescript-eslint's type-aware rules (for example
  `no-floating-promises`). Revisit when typescript-eslint supports the TS 7 API
  or when Biome's type-aware rules stabilize.
