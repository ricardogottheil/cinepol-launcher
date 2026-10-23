# ADR 0012: Saved servers

- Status: accepted
- Date: 2026-10-23

## Context

Players also play on servers other than CINEPOL. The launcher lists CINEPOL
first and lets the player save more servers, each with its live status and a
Play button that starts an instance and joins that server. The game's own
`servers.dat` (ADR 0008) is per instance and stays as it is.

## Decision

- **Storage.** `servers.json` in the data root holds the saved servers (id,
  name, address, the chosen instance), written like `accounts.json`: one lock
  for read-modify-write, `write_atomic`, a schema version, and a corrupt file
  set aside instead of lost. Entries edited into something invalid are
  dropped on read, and a chosen instance id that is not valid is forgotten.
- **Rules.** Names are trimmed, 1 to 64 characters, without control
  characters. Addresses are `ServerAddress` (host, optional port, IPv6 in
  brackets). The same server (host case and the default port aside) is never
  saved twice, and a chosen instance must exist.
- **One instance per server.** Each saved server keeps the instance it is
  played with (the player picks it); Play without one asks to pick it. A
  removed instance counts as none.
- **Joining.** `launch_instance` takes an optional `joinServer`. It is parsed
  as a `ServerAddress` before it becomes a game argument (Quick Play, or
  `--server`/`--port` on older versions).
- **Status.** `ping_server` pings any address with the same Server List Ping
  and timeouts as CINEPOL, refreshed every 30 seconds while the page is open.

## Consequences

- Removing a saved server never touches its instance or the instance's
  `servers.dat`.
- Servers are not shared between devices; `servers.json` is local.
