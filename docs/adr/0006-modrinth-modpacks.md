# ADR 0006: Modrinth modpacks (install, update, export)

- Status: accepted
- Date: 2026-10-18

## Context

Players install Modrinth modpacks (`.mrpack`), update them later, and export
their own instances as `.mrpack` files. A pack is untrusted input: its index
lists file paths, hashes and download URLs, and its `overrides/` folders are
extracted into the game folder. Players also change configs and add mods of
their own, and an update must not throw that work away.

## Decision

- **Validation in the domain:** `domain::modpack::mrpack` turns the raw index
  into a checked `MrpackIndex`. Every path must be a safe `RelativePath`, every
  file must declare valid sha1 and sha512 hashes (downloads are verified against
  sha512), and download URLs must be `https` on the hosts the mrpack
  specification allows (`cdn.modrinth.com`, `github.com`,
  `raw.githubusercontent.com`, `gitlab.com`). Only format version 1 and the
  `minecraft`, `fabric-loader`, `quilt-loader`, `forge` and `neoforge`
  dependencies are accepted.
- **Sides:** the client installs files whose `env.client` is `required` or
  `optional`; `unsupported` ones are skipped. `overrides/` is applied, then
  `client-overrides/` on top; `server-overrides/` is never extracted.
- **Archives:** `ZipMrpackArchive` validates every entry name before writing
  anything, keeps every target inside the staging folder (zip-slip), skips
  symlinks and caps the decompressed size (8 MiB index, 256 MiB per file,
  1 GiB per pack).
- **Updates keep the player's changes:** each modpack instance stores
  `pack-manifest.json` with the path and sha1 of every file the pack installed.
  The pure `plan_sync` writes a file only when it is missing or still matches
  the recorded hash, leaves files the player changed or added alone, and
  removes dropped pack files only while they are intact. The manifest is saved
  only after the whole sync succeeded, so a failed update can be retried.
- **Export:** mods, configs, resource packs, shader packs and `options.txt` are
  packed. Files Modrinth knows by sha512 (`POST /v2/version_files`) become
  index entries; the rest go to `overrides/`. A catalog failure fails the
  export instead of producing a pack full of overrides.
- **Rate limits:** every Modrinth request (GET and POST) goes through the
  retrying HTTP client, which honours `Retry-After` on 429 and sends the
  launcher's User-Agent; the UI debounces search input. Exhausted retries
  surface as the `rate_limited` error code.
- **File pickers** open from Rust with `tauri-plugin-dialog`; the webview gets
  no dialog or file-system permission and never sends a path.
- **Icons:** the CSP `img-src` allows `https://cdn.modrinth.com` for pack icons.

## Consequences

- A pack imported from a local file is an instance with the `custom` origin; it
  is not updated from the catalog.
- A file the player changed stays at the player's version after an update, even
  when the pack ships a fix for it. A pack file the player deleted comes back on
  the next update (only changed or added files are the player's).
- The manifest records the sha1 of the file actually written, not the one the
  index declares, so a wrong declared sha1 cannot make an intact file look
  changed.
- A new instance never reuses an existing folder, even one whose metadata
  cannot be read, so cleaning up a failed install cannot delete worlds.
- Installing and updating chain the regular instance install (game, loader,
  Java) in the same cancellable operation.
