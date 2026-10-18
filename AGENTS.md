# AGENTS.md

## 1. Language rule (non-negotiable)

**All repository content is in English, regardless of the language used in chat.**

Code, identifiers, comments, doc comments, commit messages, branch names, PR
descriptions, docs, ADRs, log and error messages, test names and UI strings are
English. UI strings go through i18n keys with `en` as the source locale.
The only exceptions are translation files under `src/i18n/locales/<lang>/`
(for example `es`) and real-world data captured verbatim into
`crates/*/tests/fixtures/` (server responses, API payloads), which is external
input, not authored text. Reply to people in their chat language, but never let that
language leak into the repository. Run `pnpm check:english` before every commit.

## 2. Project overview

**Cinepol Launcher** is the official launcher of the CINEPOL community Minecraft
server (the community of the streamer polispol1, server `mc.policarpiers.com`).

- Platforms: Windows (x64, arm64), macOS (Intel, Apple Silicon), Linux (x64, arm64).
- Accounts: Microsoft (premium) and offline profiles.
- Loaders: Vanilla, Fabric, Quilt, Forge, NeoForge.
- Modpack sources: Modrinth (`.mrpack`), CurseForge (manifest zip), packwiz.
- Display locale: `es` by default, `en` as fallback and source locale.
- Disclaimer shown in the footer/About: "Not an official Minecraft product. Not
  approved by or associated with Mojang or Microsoft." Never use Minecraft or
  Mojang logos or trademarks as app branding.

## 2b. Design rules

- The UI must match the CINEPOL website (https://cinepol.policarpiers.com/,
  source at https://github.com/ricardogottheil/cinepol-web, usually checked out
  next to this repository as `../cinepol-web`).
- `docs/design/design-system.md` and `src/styles/tokens.css` are the source of
  truth. Never hardcode colors, fonts or spacing in components; use tokens.
- New components go in `src/components/ui/` and follow the Overworld / Nether /
  End and cinema motifs (color-temperature captions, label -> value spec sheets).
- When the website's design changes, update the design doc and tokens first,
  then the components.
- Port the look, not the code: no Astro components, rebuild them in React.
- Accessible: WCAG AA contrast, visible focus, full keyboard navigation,
  `prefers-reduced-motion` respected. Dark theme only for v1.

## 3. Architecture

```
 React UI (src/features, src/components/ui)
        │  TanStack Query hooks
        ▼
 src/ipc/client.ts  ── the only importer of bindings.ts and @tauri-apps/api/core
        │  invoke / Channel<T>
        ▼
 src-tauri/src/commands   thin: validate → one use case → DTO / AppError
        │  wired in src-tauri/src/state.rs (composition root)
        ▼
 launcher-core::application   use cases, one per file
        │  depend on
        ▼
 launcher-core::domain        entities, value objects, ports (traits), errors
        ▲  implemented by
        │
 launcher-core::infrastructure  adapters: http, auth, mojang, loaders, modrinth,
                                curseforge, packwiz, java, nbt, server_ping,
                                process, keychain, fs
```

**Dependency rule**: `domain` ← `application` ← `infrastructure`.
`src-tauri` depends on `launcher-core`; `launcher-core` never depends on `tauri`.
`crates/launcher-core/tests/architecture.rs` fails the build when `domain` or
`application` import outer layers or `tauri`.

- Long operations (install, import, launch) report progress through
  `tauri::ipc::Channel<T>`; use cases see a `ProgressReporter`/`EventSink` port.
  Global events are only for app-wide signals (game exited, account expired).
- Long operations accept a `tokio_util::sync::CancellationToken`.
- Errors: typed `thiserror` enums in the core, mapped in `src-tauri` to
  `AppError { code, message, details? }`. The frontend switches on `code`, never
  parses messages.
- See `docs/adr/` for the recorded decisions.

## 4. Commands

| Task | Command |
| --- | --- |
| Install | `pnpm install` |
| Dev | `pnpm tauri dev` |
| Typecheck (TypeScript 7 native `tsc`) | `pnpm typecheck` |
| Lint (Biome) | `pnpm lint` (auto-fix: `pnpm format`) |
| Rust format | `cargo fmt --all` |
| Rust lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust tests | `cargo test --workspace` |
| Network tests (opt-in) | `cargo test -p cinepol-launcher-core --features network-tests` |
| Real vanilla E2E (opt-in, ~1 GB, opens the game) | `cargo test -p cinepol-launcher-core --features network-tests --test e2e_vanilla -- --nocapture` (data in `target/e2e-data`; delete it afterwards) |
| Real Fabric/Quilt E2E | the same command with `CINEPOL_E2E_LOADER=fabric` (or `quilt`) |
| Frontend tests | `pnpm test` |
| English-only check | `pnpm check:english` |
| Regenerate bindings | `pnpm bindings` |
| Build | `pnpm tauri build` |

## 5. Conventions

### Rust

- No `unwrap`/`expect` outside tests (workspace clippy lints deny them).
  `main.rs` startup reports errors and exits instead of panicking.
- `thiserror` in the core; `anyhow` only at the outermost edges, never in `domain`.
- One use case per file in `application/`, named after the action
  (`launch_instance.rs`, `create_offline_profile.rs`).
- Ports are traits with `async fn` (use `async-trait` only when a port must be
  used as `dyn`). Every port gets an in-memory fake for tests.
- DTOs live only in `src-tauri/src/dto.rs` and `AppError`/`ErrorCode` in
  `src-tauri/src/error.rs`; domain types never derive `ts_rs::TS`.
- Cancellable operations take an `operationId` from the frontend, register it in
  `OperationRegistry` (`src-tauri/src/operations.rs`) and are cancelled with the
  `cancel_operation` command.
- Deserialize external APIs into strict serde types that tolerate unknown fields;
  never panic on missing optional data.
- Validate every string that becomes a path or a JVM/game argument.
- Files and modules: `snake_case`. Types: `PascalCase`.

### TypeScript

- `strict` mode, no `any`, named exports only (config files excepted).
- Feature folders: `src/features/<feature>/` holds its components, hooks and tests.
- Never import `src/ipc/bindings.ts` or `@tauri-apps/api/core` outside `src/ipc/`
  (Biome enforces it).
- Files: `PascalCase.tsx` for components, `camelCase.ts` for modules, tests next
  to the code as `*.test.ts(x)`.
- All user-facing text goes through i18n keys in `src/i18n/locales/en/` first.

## 6. Recipe: add a new command

1. Domain: add or extend entities, value objects and the ports you need.
2. Use case: `crates/launcher-core/src/application/<action>.rs`, plus unit tests
   with in-memory fakes.
3. Adapter: implement new ports in `infrastructure/` (with fixtures if it parses
   external data).
4. Wire it in `src-tauri/src/state.rs`.
5. Thin command in `src-tauri/src/commands/`; DTOs in `dto.rs`; map errors to
   `AppError`. Register the DTOs and the command in `src-tauri/src/bindings.rs`.
6. `pnpm bindings` to regenerate `src/ipc/bindings.ts`.
7. Add the typed function to `src/ipc/client.ts`.
8. Add a TanStack Query hook in the feature folder.
9. Build the UI with `src/components/ui/` components and i18n keys.

## 7. Recipe: add a modpack source or loader adapter

- Modpack source: implement the `ModpackSource` port in
  `infrastructure/<source>/`. Loader: implement the `LoaderInstaller` port in
  `infrastructure/loaders/<loader>/`.
- Add real-world fixtures under `crates/launcher-core/tests/fixtures/<source>/`
  (API responses, manifests, version JSONs).
- Required tests: parsing of every fixture, hash verification, side filtering
  where it applies, zip-slip rejection for any archive, and the use case that
  consumes the adapter (with fakes). Network tests go behind `network-tests`.
- Send a descriptive `User-Agent`, respect rate limits, read API keys from
  config/env only.

## 8. Testing expectations

- Core domain and application logic: unit tests with in-memory fakes for every
  port. No network in unit tests.
- Fixtures in `crates/launcher-core/tests/fixtures/`: vanilla, Fabric, Forge and
  NeoForge version JSONs, an `.mrpack`, a CurseForge manifest, a packwiz pack,
  a `servers.dat`.
- Rule-evaluation tests for argument and library rules across the OS/arch matrix.
- Integration tests that hit real APIs only behind `--features network-tests`;
  not run by default in CI.
- Frontend: Vitest + Testing Library per feature; one smoke test against the
  built app when feasible.

## 9. Do / Don't

Do:

- Keep commands thin; use Channels for progress.
- Verify hashes of every artifact that declares one; refuse to launch with
  unverified libraries.
- Redact secrets in logs (tokens, auth codes, API keys).
- Protect every archive extraction against zip-slip.

Don't:

- Hardcode colors, fonts or spacing outside the tokens.
- Hardcode the server address, links or texts outside config.
- Put business logic in `src-tauri` or React.
- Call `invoke` from components.
- Add a `tauri` dependency to `launcher-core`.
- Commit API keys or write tokens to disk (the OS keychain only).
- Hand-edit `src/ipc/bindings.ts`.
- Write non-English text in the repository.

## 10. Commit convention

Conventional Commits in English: `feat(core): ...`, `fix(ui): ...`,
`chore: ...`, `ci: ...`, `docs(adr): ...`. Small, reviewable commits.

## 11. Definition of done

- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` and `pnpm test` pass.
- [ ] `pnpm typecheck` and `pnpm lint` pass.
- [ ] `pnpm bindings` produces no diff.
- [ ] Docs and ADRs updated if behavior or architecture changed.
- [ ] `pnpm check:english` passes.
