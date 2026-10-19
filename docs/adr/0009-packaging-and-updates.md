# ADR 0009: Packaging, releases and in-app updates

- Status: accepted
- Date: 2026-10-19

## Context

The launcher ships to Windows (x64, arm64), macOS (Apple Silicon, Intel) and
Linux (x64, arm64), and must update itself with signed update manifests. The
source repository is private, so its GitHub Releases cannot serve downloads to
players without a token, and a token compiled into the app would leak to
every player.

## Decision

- **Releases repository:** installers and `latest.json` are published to a
  separate public repository that holds only GitHub Releases. The source stays
  private. CI publishes with a fine-grained token limited to that repository.
- **One publisher:** each matrix job builds and signs with the Tauri CLI
  directly (no third-party release action), then a tested helper
  (`scripts/release/bundles.mjs`) collects the bundles under fixed names with
  each platform's signature. A single `publish` job merges those into
  `latest.json`, refuses a release missing any platform, and creates, fills
  and publishes the release. Parallel jobs never race on `latest.json`, and a
  failed run publishes nothing.
- **Targets:** Windows NSIS + MSI (MSI on x64 only, because WiX cannot build
  arm64), a universal macOS DMG/.app, Linux AppImage + deb.
- **Keys:** the minisign private key and its password are CI secrets. The
  public key is a CI variable, compiled into the launcher at build time
  (`CINEPOL_UPDATER_PUBKEY`) and written into the release-only config, so the
  repository holds an empty placeholder and local builds work without keys.
  `createUpdaterArtifacts` is enabled only by the release config.
- **Downgrade protection:** `requireSignedVersion` is on from the first
  release, since the Tauri CLI records the version in every signature.
- **Updater in the app:** an `AppUpdater` port in the core with the
  `check_app_update` and `install_app_update` use cases;
  `tauri-plugin-updater` implements it in `src-tauri`. Installing downloads
  and verifies only the version the player accepted and refuses while any
  instance is busy (installing restarts the launcher). After the download it
  freezes the games registry: in one step it checks that nothing started
  meanwhile and refuses every new session until the launcher restarts (or the
  install fails). The restart uses Tauri's own `AppHandle::restart`. The plugin's
  JavaScript commands are not exposed; the UI goes through our commands.

## Consequences

- A release needs the one-time setup in `docs/release.md` (releases
  repository, key pair, secrets).
- Losing the private key means installed launchers can never update again;
  rotating it needs a release signed with the old key that ships the new
  public key.
- Windows Authenticode signing is still a placeholder; until it is wired in,
  SmartScreen warns on first install.
- An unsigned macOS build is published with a warning when the Apple secrets
  are missing; Gatekeeper blocks it on players' machines.
