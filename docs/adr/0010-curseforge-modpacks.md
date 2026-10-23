# ADR 0010: CurseForge modpacks

- Status: accepted
- Date: 2026-10-23

## Context

Players install modpacks from CurseForge as well as Modrinth. CurseForge's
API needs a third-party key (`x-api-key`). A pack zip holds a
`manifest.json` that lists project and file ids instead of paths and
download URLs, plus an overrides folder. Authors can forbid third-party
distribution of their files; the API then returns no download URL.

## Decision

- **Key:** `CURSEFORGE_API_KEY` is a build-time setting, read like
  `MSA_CLIENT_ID` (environment or the git-ignored `.env`; a CI secret for
  releases). Builds without it hide CurseForge: the UI asks
  `get_modpack_catalogs`, and the source answers `NotConfigured` without
  sending a request. The key is sent through a dedicated `HttpClient` call
  (`get_bytes_keyed` / `post_json_keyed`) and never appears in errors or logs.
  Keyed calls follow no redirect (a redirect is an error), so the key never
  reaches another host: reqwest only strips standard auth headers.
- **Catalog:** `CurseForgeSource` implements `ModpackSource` (search sorted by
  popularity, filtered by game version and loader, project and file details)
  and `CurseForgeLookup` (files and projects by id, in batches). Project ids
  must be decimal numbers before they become part of a URL.
- **Files:** `resolve_files` maps each required manifest file to a folder by
  its project's class (mods, resource packs, shader packs; any other class is
  refused) and verifies it by the sha1 and size CurseForge declares, from
  CurseForge's CDN hosts only. A file without a download URL becomes a manual
  download, handled by the existing Downloads-folder flow (ADR 0007).
- **Zips:** `ZipCurseForgeArchive` reads a size-capped `manifest.json` and
  extracts the overrides folder it names with the same zip-slip rules and
  size caps as `.mrpack` overrides (the extraction is shared).
- **Install and update:** installs come from the catalog (origin
  `Modpack { CurseForge, project, file }`) or a local zip (origin `Custom`)
  through the shared sync pipeline, so player changes are kept as for other
  packs. `update_modpack` routes CurseForge instances to
  `update_curseforge`, which follows the new file's game version and loader.
- **packwiz:** with a key, packwiz files in CurseForge mode download on their
  own when CurseForge gives their download URL, verified with the hash the
  pack declares; the rest stay manual. When CurseForge cannot be reached or
  refuses the key, every such file stays manual (with a warning in the log),
  so an outage never stops a packwiz pack from installing or launching.

## Consequences

- The key is compiled into release builds, as every launcher ships its
  CurseForge key; a leaked key can be rotated with a new release.
- CurseForge's `manifest.json` cannot express optional files the player picks;
  only required files install.
- Instances imported from a local zip cannot be updated from the catalog.
- No CurseForge export: Modrinth's `.mrpack` stays the export format.
