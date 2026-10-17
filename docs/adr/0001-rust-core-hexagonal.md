# ADR 0001: Framework-free Rust core with hexagonal architecture

- Status: accepted
- Date: 2026-10-17

## Context

Cinepol Launcher has a lot of logic that does not depend on the desktop shell:
the Microsoft/Xbox auth chain, version JSON resolution, loader installers, modpack
formats, downloads and process management. That logic must be testable without a
window, a network or a real file system, and must survive a future change of shell.

## Decision

- A Cargo workspace with two crates:
  - `crates/launcher-core` (`cinepol-launcher-core`): domain, application and
    infrastructure. It has **no `tauri` dependency**.
  - `src-tauri` (`cinepol-launcher`): the Tauri app, a driving adapter only.
- Inside the core, dependencies point inwards:
  `infrastructure` -> `application` -> `domain`.
  - `domain`: entities, value objects, ports (traits) and errors. No I/O.
  - `application`: one use case per file, depending only on `domain`.
  - `infrastructure`: adapters implementing the ports (reqwest, keyring, fs...).
- `src-tauri/src/state.rs` is the composition root. Commands are thin: validate,
  call one use case, map the result or error to a DTO.
- Errors are `thiserror` enums in the core; `anyhow` is allowed only at the
  outermost edges. `unwrap`/`expect` are denied by workspace clippy lints
  (allowed in tests).

## Enforcement

- Crate boundary: the core cannot name `tauri` because it does not depend on it.
- `crates/launcher-core/tests/architecture.rs` scans `domain` and `application`
  and fails on imports of outer layers or `tauri`, and checks the core manifest.
- Module visibility: internals default to `pub(crate)`; only ports, use cases and
  adapter constructors are `pub`.

## Consequences

- Use cases are unit-tested with in-memory fakes for every port.
- Adding a feature touches more files (port, use case, adapter, command, DTO);
  `AGENTS.md` documents the recipe.
- Progress and cancellation cross the boundary through ports (`ProgressReporter`,
  `CancellationToken`), never through Tauri types.
