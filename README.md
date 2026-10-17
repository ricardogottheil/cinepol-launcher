# Cinepol Launcher

The official launcher of **CINEPOL**, the community Minecraft server of the
streamer [polispol1](https://twitch.tv/polispol1). Website:
[cinepol.policarpiers.com](https://cinepol.policarpiers.com/).

> Not an official Minecraft product. Not approved by or associated with Mojang or
> Microsoft.

## Features (planned)

- One click to play on CINEPOL, with live server status.
- Microsoft (premium) and offline accounts.
- Isolated instances with Vanilla, Fabric, Quilt, Forge and NeoForge.
- Modpacks from Modrinth, CurseForge and packwiz.
- Automatic Java runtime management.
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

Copy `.env.example` to `.env` to configure `MSA_CLIENT_ID` (an Azure app
registration that also needs Mojang approval to call the Minecraft Services API)
and `CURSEFORGE_API_KEY`. Never commit real values.

All checks, conventions and recipes are in [`AGENTS.md`](AGENTS.md).

## License

To be decided.
