# ADR 0007: packwiz packs and manual downloads

- Status: accepted
- Date: 2026-10-19

## Context

packwiz packs are published as a folder: `pack.toml` points at `index.toml`,
which lists every file with its hash, and `*.pw.toml` metafiles describe
files to download (from Modrinth or another https host, or from CurseForge).
Players install them from an https URL or a local folder and may want them
updated before every launch. CurseForge-mode metafiles carry no URL: their
file has to be resolved through the CurseForge API, which the launcher's
current API key cannot use (it reads search results but not files).

## Decision

- **Validation in the domain:** `domain::modpack::packwiz` checks `pack.toml`
  (format `packwiz:1.x`, one loader at most, the Minecraft version), the index
  (safe relative paths, no duplicates ignoring case, a supported hash per
  entry) and each metafile (one-segment file name, client or both sides,
  `https` URL or CurseForge ids). sha1, sha256, sha512 and murmur2 (the
  CurseForge fingerprint) are verified; md5 and anything else is refused.
- **Verified reads:** the packwiz source verifies the index and every metafile
  against the hash their parent declares before parsing them, and caps their
  size (1 MiB for `pack.toml` and metafiles, 16 MiB for the index). Local
  packs never resolve outside their folder, symlinks included.
- **One sync pipeline:** mrpack and packwiz share planning, staging,
  placement, removal and the manifest. Downloads land in staging first; local
  files are copied into staging and verified there. Player changes are kept
  as for Modrinth packs (ADR 0006), and `preserve = true` files are written
  only when absent.
- **Manual downloads:** CurseForge-mode files become pending `ManualFile`s in
  the pack manifest. The instance row lists them with a link to the
  project's CurseForge page and checks the Downloads folder every few
  seconds; a file named like the expected one (including browser `(1)`
  suffixes) is installed only when its hash matches, and the original stays
  in Downloads. Play is disabled while files are missing. With a CurseForge
  key that can read files (M11), these files will download on their own.
- **Locations:** the webview may send only `https` pack URLs. Local packs are
  chosen with the native file picker, so the UI cannot point the launcher at
  arbitrary paths. The location is stored in the instance origin
  (`ModpackRef { source: Packwiz, project: <location>, version }`).
- **Update on launch:** an opt-in per instance (`update_pack_on_launch` in the
  launch settings, off by default). Play syncs the pack first and starts the
  game only when the sync succeeded.

## Consequences

- A pack whose CurseForge files are missing is installed but cannot start
  until the player downloads them.
- The CurseForge page link uses `https://www.curseforge.com/projects/<id>`,
  which curseforge.com does not serve to non-browser clients, so it is
  checked in a browser rather than in tests.
- Updating hashes each existing pack file once in its declared algorithm, so
  updates read files that are already on disk.
