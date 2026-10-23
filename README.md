<div align="center">

![Cinepol Launcher](https://shieldcn.dev/header/gradient.svg?title=Cinepol%20Launcher&subtitle=The%20launcher%20of%20the%20CINEPOL%20Minecraft%20community&theme=emerald)

[![CI](https://shieldcn.dev/github/ci/ricardogottheil/cinepol-launcher.svg?variant=secondary)](https://github.com/ricardogottheil/cinepol-launcher/actions/workflows/ci.yml)
[![Latest release](https://shieldcn.dev/github/release/ricardogottheil/cinepol-launcher-releases.svg?theme=emerald)](https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest)
[![Downloads](https://shieldcn.dev/github/downloads/ricardogottheil/cinepol-launcher-releases.svg?theme=emerald)](https://github.com/ricardogottheil/cinepol-launcher-releases/releases)
[![License](https://shieldcn.dev/github/license/ricardogottheil/cinepol-launcher.svg?variant=secondary)](LICENSE)
[![Last commit](https://shieldcn.dev/github/last-commit/ricardogottheil/cinepol-launcher.svg?variant=secondary)](https://github.com/ricardogottheil/cinepol-launcher/commits/main)

[![Windows](https://shieldcn.dev/badge/Windows-x64%20%7C%20arm64-0078D4.svg?logo=lu:AppWindow&variant=secondary)](https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest)
[![macOS](https://shieldcn.dev/badge/macOS-universal-000000.svg?logo=apple&variant=secondary)](https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest)
[![Linux](https://shieldcn.dev/badge/Linux-x64%20%7C%20arm64-FCC624.svg?logo=linux&variant=secondary)](https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest)

</div>

The official launcher of **CINEPOL**, the community Minecraft server of the
streamer [polispol1](https://twitch.tv/polispol1). Website:
[cinepol.policarpiers.com](https://cinepol.policarpiers.com/).

**[Download the latest release](https://github.com/ricardogottheil/cinepol-launcher-releases/releases/latest)**

> Not an official Minecraft product. Not approved by or associated with Mojang or
> Microsoft.

## Features

- One click to play on CINEPOL, with live server status.
- Microsoft (premium) and offline accounts.
- Isolated instances with Vanilla, Fabric, Quilt, Forge and NeoForge, each with
  its own Java, memory, JVM arguments and window size.
- Modpacks from Modrinth (`.mrpack`), CurseForge (catalog or `.zip`) and
  packwiz, kept up to date without touching the player's own changes. Files
  whose authors forbid third-party downloads are picked up from the
  Downloads folder.
- Automatic Java runtime management.
- A server list editor, a log viewer with level and text filters, and crash
  reports one click away.
- Signed automatic updates.
- Windows, macOS and Linux.

The roadmap is split in milestones; see `AGENTS.md` for the architecture and
`docs/adr/` for the decisions.

## Development

Requirements: Rust (pinned in `rust-toolchain.toml`), Node.js 24+, pnpm 12
and the [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm tauri dev
```

Copy `.env.example` to `.env` to configure `MSA_CLIENT_ID` and
`CURSEFORGE_API_KEY`. Never commit real values.

`MSA_CLIENT_ID` enables Microsoft sign-in. To get one:

1. In the Azure portal, open **Microsoft Entra ID > App registrations > New
   registration** (a personal Microsoft account needs its own directory first;
   creating a free Azure account makes one).
2. Supported account types: **Personal Microsoft accounts only**. No redirect URI.
3. **Authentication > Advanced settings > Allow public client flows: Yes**
   (the device code flow needs it; no client secret is used).
4. Copy the **Application (client) ID** into `.env` as `MSA_CLIENT_ID=...`.
5. Ask Mojang for Minecraft Services access at https://aka.ms/mce-reviewappid.
   Until it is approved, sign-in fails with "not enabled for this launcher yet".

`src-tauri/build.rs` reads `MSA_CLIENT_ID` from the environment or `.env` at
build time. Builds without it hide Microsoft sign-in. Set
`CINEPOL_REQUIRE_PREMIUM_FOR_OFFLINE=true` at build time to require a Microsoft
account before offline profiles can be created (off by default, because the
CINEPOL server accepts non-premium players). See
[ADR 0005](docs/adr/0005-microsoft-sign-in-and-token-storage.md).

All checks, conventions and recipes are in [`AGENTS.md`](AGENTS.md).

## Releases

Installers are published to the public
[releases repository](https://github.com/ricardogottheil/cinepol-launcher-releases/releases),
and the launcher updates itself from there. How to sign and cut a release is in
[`docs/release.md`](docs/release.md).

## License

The source code is licensed under the [GNU General Public License v3.0 only](LICENSE)
(`GPL-3.0-only`).

The CINEPOL name, wordmark, logo and the imagery under `src/assets/brand/` are
**not** covered by the GPL. They identify the official launcher of the CINEPOL
community and may not be used to distribute modified versions. Forks must remove
them and ship under a different name.

Third-party fonts keep their own licenses (SIL Open Font License 1.1), listed in
`src/assets/brand/fonts/`.
