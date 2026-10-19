# ADR 0008: Server list editor, log viewer and crash detection

- Status: accepted
- Date: 2026-10-19

## Context

Players manage each instance's multiplayer servers, read the game's output
to understand problems, and need to find out when the game crashed. Modern
versions print their log as log4j XML events, which are unreadable raw. The
game rewrites `servers.dat` when it exits, and writes a crash report into
`crash-reports/` when it crashes.

## Decision

- **Server list:** `edit_server_list` adds, edits, removes and reorders
  servers in `servers.dat` through the existing `ServerListStore`. Editing a
  server changes only its name, address and resource-pack choice; its cached
  icon, hidden flag and unknown tags are kept. Every change is validated
  before it is saved, and the list cannot change while the instance runs
  (the game would overwrite it on exit).
- **Log decoding:** the launch wraps the log sink in `Log4jSink`, a pure
  decoder in the domain. log4j events (also when split across lines, with
  split CDATA sections and escaped attributes) become entries with a level,
  thread, logger and message, the throwable appended to the message. Other
  lines pass through as plain entries. Sinks that only show text receive the
  message (`LogSink::entry` defaults to `line`).
- **Log viewer:** the instance log shows the thread and level of log4j lines,
  filters by minimum level (plain stdout counts as info, plain stderr as an
  error) and by text in the message, thread or logger, and reads warnings and
  errors in `stale` (there is no error red).
- **Crash detection:** a launch that ends with a non-zero exit the launcher
  did not cause, plus a file in `crash-reports/` modified since the launch
  started (with two seconds of clock skew allowed), is a crash. The outcome
  carries the report's file name; the UI offers to open it, and the command
  opens only a plain file name inside that instance's `crash-reports/`
  folder, with the system's default app, from Rust.

## Consequences

- A crash that writes no report (for example a JVM that dies before the game
  starts) shows the plain exit code, as before.
- The launcher keeps no crash history; the last outcome is shown until the
  next launch.
