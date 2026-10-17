# Release and signing

Status: placeholders. Packaging, signing and the updater are delivered in
Milestone 14. This document lists every secret the pipeline will need so the
repository never contains real values.

## Targets

| OS | Architectures | Bundles |
| --- | --- | --- |
| Windows | x64, arm64 | NSIS, MSI |
| macOS | Apple Silicon, Intel (or universal) | DMG, .app |
| Linux | x64, arm64 | AppImage, deb (rpm optional) |

CI builds on `windows-latest`, `macos-latest` (arm64), `macos-15-intel` (x64;
`macos-13` was retired by GitHub in December 2025) and `ubuntu-22.04`.

## GitHub Actions secrets

| Secret | Used for |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Signs updater artifacts (`tauri signer generate`). |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password of the updater key. |
| `APPLE_CERTIFICATE` | Base64 `.p12` Developer ID Application certificate. |
| `APPLE_CERTIFICATE_PASSWORD` | Password of the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: <Name> (<TEAMID>)`. |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Notarization (app-specific password). |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Authenticode signing. |
| `MSA_CLIENT_ID` | Microsoft login client ID injected at build time. |
| `CURSEFORGE_API_KEY` | CurseForge API key injected at build time. |

The updater public key goes in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`).
It is public by design; the private key never leaves the secrets store.

## macOS notes

- Hardened Runtime is enabled in `tauri.conf.json` (`bundle.macOS.hardenedRuntime`).
- Unsigned or un-notarized downloads get the `com.apple.quarantine` attribute and
  Gatekeeper blocks them. Developers testing a local unsigned build can clear it
  with `xattr -dr com.apple.quarantine "/Applications/Cinepol Launcher.app"`.
  Users must receive signed and notarized builds.
- Java runtimes downloaded by the launcher also get the quarantine attribute when
  extracted by some tools; the launcher extracts them itself, so they do not.
