# Release and signing

Releases are built by `.github/workflows/release.yml` when a tag `vX.Y.Z` is
pushed, and published to the public repository
[`ricardogottheil/cinepol-launcher-releases`](https://github.com/ricardogottheil/cinepol-launcher-releases).
This source repository stays private; the releases repository holds only
GitHub Releases. The repository never contains real secret values.
See [ADR 0009](adr/0009-packaging-and-updates.md) for the decisions.

## Targets

| OS | Architectures | Bundles |
| --- | --- | --- |
| Windows | x64, arm64 | NSIS (x64, arm64), MSI (x64 only: WiX cannot build arm64) |
| macOS | universal (Apple Silicon + Intel) | DMG, .app |
| Linux | x64, arm64 | AppImage, deb |

Release runners: `windows-latest` (x64, and arm64 cross-compiled),
`macos-latest` (universal), `ubuntu-22.04` and `ubuntu-22.04-arm`. The CI
workflow (`ci.yml`) builds unsigned bundles on every push to `main` and every
pull request.

## One-time setup

1. **Releases repository.** Create the public repository
   `ricardogottheil/cinepol-launcher-releases` with at least one commit on
   `main` (a README), because each release tag is created from that branch.
2. **Updater key pair.** Run `pnpm tauri signer generate -w ~/.tauri/cinepol-launcher.key`
   on a trusted machine and keep the private key and its password somewhere safe:
   losing them means installed launchers can never update again.
3. **Secrets and variables** in this repository (Settings > Secrets and variables > Actions):

| Name | Kind | Used for |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | secret | Content of the private key; signs every updater artifact. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secret | Password of the private key. |
| `CINEPOL_UPDATER_PUBKEY` | variable | Content of the `.pub` file. Compiled into the launcher (`src-tauri/build.rs`, `src-tauri/src/updater.rs`) and written into the release config, so the CLI checks the key pair matches. It is public by design. |
| `RELEASES_TOKEN` | secret | Fine-grained token with **Contents: read and write** on the releases repository only. |
| `MSA_CLIENT_ID` | secret | Microsoft login client ID (see the README). |
| `CURSEFORGE_API_KEY` | secret | CurseForge API key. |
| `APPLE_CERTIFICATE` | secret | Base64 `.p12` Developer ID Application certificate. |
| `APPLE_CERTIFICATE_PASSWORD` | secret | Password of the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | secret | e.g. `Developer ID Application: <Name> (<TEAMID>)`. |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | secrets | Notarization (app-specific password). |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | secrets | Authenticode signing. Placeholder: not wired into the workflow yet. |

The release fails before building when `TAURI_SIGNING_PRIVATE_KEY`,
`CINEPOL_UPDATER_PUBKEY` or `RELEASES_TOKEN` is missing. The Apple secrets are
optional: without them the macOS build is published unsigned, with a warning.

Build-time settings (not secrets):

| Variable | Effect |
| --- | --- |
| `CINEPOL_UPDATER_PUBKEY` | Enables in-app updates. Builds without it report updates as not configured. |
| `CINEPOL_REQUIRE_PREMIUM_FOR_OFFLINE` | `true` requires a Microsoft account before offline profiles can be created. Default off. |

## Cutting a release

1. Bump `version` in `src-tauri/tauri.conf.json`, `package.json` and the
   workspace `Cargo.toml` (`[workspace.package]`), and merge it.
2. Create an **annotated** tag whose message is the release notes, and push it:
   `git tag -a v0.2.0 -m "Faster modpack installs"` then `git push origin v0.2.0`.
   The tag must match the version, or the release stops at the preflight.
3. The workflow builds the five targets, then the `publish` job writes
   `latest.json`, creates a draft release in the releases repository, uploads
   every file and publishes it. Only then do installed launchers see it.

A failed run publishes nothing. Re-running the workflow reuses a draft that a
failed run left behind. To change the code instead, delete that draft and the
tag, fix the problem and tag again.

## How updates reach players

- The launcher checks
  `https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest/download/latest.json`
  (`plugins.updater.endpoints` in `tauri.conf.json`) on start and every hour.
- `latest.json` lists one signed file per platform: the NSIS installer on
  Windows, the `.app.tar.gz` on macOS (both architectures) and the AppImage on
  Linux, plus `-msi` and `-deb` entries for players who installed those.
- Downloads are verified against the compiled public key before they install.
  `requireSignedVersion` is on, so a signature only counts for the version it
  was signed for: an edited `latest.json` cannot offer an older, validly signed
  release as an update.
- The launcher never installs an update while a game installs or runs, because
  installing restarts it.

## macOS notes

- Hardened Runtime is enabled in `tauri.conf.json` (`bundle.macOS.hardenedRuntime`).
- Unsigned or un-notarized downloads get the `com.apple.quarantine` attribute and
  Gatekeeper blocks them. Developers testing a local unsigned build can clear it
  with `xattr -dr com.apple.quarantine "/Applications/Cinepol Launcher.app"`.
  Users must receive signed and notarized builds.
- Files the launcher downloads through its own HTTP client (Java runtimes,
  libraries) are not quarantined unless the app opts into `LSFileQuarantineEnabled`.
