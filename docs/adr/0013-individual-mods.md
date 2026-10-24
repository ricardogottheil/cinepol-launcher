# ADR 0013: Individual mods

- Status: accepted
- Date: 2026-10-24

## Context

Players want single mods, not only whole modpacks, and many do not know which
version or loader a mod needs. Like Prism Launcher, the launcher picks for
them: it only offers mods that run on the instance, installs what each mod
needs, and keeps track of why every jar is there. Modrinth comes first;
CurseForge mods are a later stage.

## Decision

- **Compatibility is the instance's, never a choice.** Search and version
  lookups are filtered by the instance's game version and loader (Quilt also
  accepts Fabric mods). The newest compatible version is installed; a newer
  beta the player already runs is never downgraded by an update.
- **Dependencies.** Required dependencies are resolved recursively (at most a
  fixed number of mods per install) and installed with the mod; optional ones
  are never installed. A mod that declares itself incompatible with an
  installed or planned mod is refused, naming the other mod. The player sees
  the full plan (`preview_mod_install`) before confirming.
- **Registry.** `instances/<id>/mods.json` records each mod the launcher
  manages: project, version, file, sha512, whether the player asked for it,
  and which mods need it. File names are single safe names; anything else in
  the file is refused on read.
- **Inventory.** The `mods/` folder is the truth. Jars the registry does not
  know are identified by sha512 against Modrinth (`/version_files`) and
  adopted as mods the player wanted; unknown jars are listed as such and never
  touched. `.jar.disabled` is how a mod is turned off, as other launchers do.
- **Removal.** Removing a mod offers to remove the dependencies nothing else
  needs any more. Orphans are deleted before the requested jar, so a failed
  removal can be retried.
- **Writes are atomic per install.** Jars are downloaded and verified in a
  per-run scratch folder, then moved; a failed move or registry save removes
  what was placed. An update sets the old jar aside first and puts it back on
  failure; if that fails too, the copy is kept and its path is reported
  (`mod_backup_kept`).
- **One writer per instance.** Commands that change `mods/` hold the instance
  like a running game (`already_running` otherwise). Install and update are
  cancellable operations with a progress channel.
- **Update checks are on request** (one catalog call per mod). Mods the
  catalog could not answer for are reported as unchecked, never as current.

## Consequences

- Mods can be added to modpack and CINEPOL instances too; a pack update keeps
  files the player added.
- Vanilla instances cannot get mods: the Mods tab says a loader is needed.
