# ADR 0004: TypeScript bindings generation

- Status: accepted
- Date: 2026-10-17

## Context

The frontend must call Rust commands and receive Channel/event payloads with
types generated from Rust, never written by hand. The preferred option was
`tauri-specta` + `specta`, which also generates command wrappers. The rule was
to use it only if its current release is stable for Tauri 2, and otherwise use
`ts-rs`.

As of 2026-10-17, the Tauri 2 line of `tauri-specta` is `2.0.0-rc.25`
(May 2026); the only stable release (1.0.2) targets Tauri 1. It is not stable
for Tauri 2.

## Decision

Use **`ts-rs`** (12.0.1) for type generation.

- DTOs in `src-tauri/src/dto.rs` derive `ts_rs::TS`.
- `src-tauri/src/bindings.rs` renders one file, `src/ipc/bindings.ts`, by
  concatenating each registered DTO declaration and a generated `Commands` map
  (command name -> argument and result types) built from a registry next to the
  commands.
- `pnpm bindings` runs the `export-bindings` binary that writes that file.
- `src/ipc/client.ts` is the only module that imports `bindings.ts` and
  `@tauri-apps/api/core`; it exposes typed functions built on the `Commands` map.
- CI regenerates the file and fails on any diff (`git diff --exit-code`).

## Consequences

- Command wrappers are typed against the generated map, but registering a
  command in the map is a manual step (`AGENTS.md`, "Add a new command").
  A missing registration shows up as a type error in `client.ts`.
- Revisit when `tauri-specta` 2.0 ships a stable release: migrating means
  replacing `bindings.rs` and keeping `client.ts`'s public API unchanged.
