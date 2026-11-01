# ADR 0014: Community server list

- Status: accepted
- Date: 2026-10-25

## Context

`server-config.json` named exactly one server. The community may run more
than one (or none at a given time), and the launcher must show whatever the
website lists, next to the servers the player saved (ADR 0012).

## Decision

- **Schema 2.** The config has a top-level community `name` and a `servers`
  list, each entry with a `name`, an `address` and an optional `port`:

  ```json
  {
    "schemaVersion": 2,
    "name": "CINEPOL",
    "servers": [{ "name": "CINEPOL", "address": "mc.policarpiers.com", "port": 25565 }],
    "versions": { "min": "1.21.11", "max": "26.2", "recommended": "26.2" },
    "links": { "website": "...", "twitch": "...", "discord": "..." }
  }
  ```

  The list may be empty and holds at most 16 servers. Names are 1 to 64
  printable characters; the same address never appears twice.
- **Schema 1** (a single `server`) still parses, as a one-server list named
  after it. Any other schema version is refused and the bundled copy is used.
- **Main server.** The first server is the main one: Play on Home joins it.
  With no servers, Play starts the recommended version without joining.
- **Joining another.** `play_on_cinepol` takes an optional `server`; it must
  be one the config lists, checked before anything is installed.
- **Community pack.** An optional top-level `pack` (any schema) is the https
  URL of the community's packwiz `pack.toml`. Older launchers ignore it, so it
  needs no schema bump; a non-https value makes the whole config invalid, like
  any other bad field. With a pack, Play uses the packwiz instance installed
  from that URL (installing it when there is none and syncing it before every
  launch); without one, Play keeps the managed Vanilla instance.
- **Status.** Every server, community or saved, is pinged with `ping_server`;
  the dedicated CINEPOL status command is gone.

## Consequences

- Launchers up to 0.2.0 refuse a schema 2 config and fall back to their
  bundled copy, so they keep working with the server they shipped with.
- The website must publish schema 2 once this version is released.
