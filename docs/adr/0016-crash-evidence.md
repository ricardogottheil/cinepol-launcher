# ADR 0016: Crash evidence

## Status

Accepted.

## Context

Players on Windows reported that the game crashes "because of the launcher",
with nothing to back it up. Two gaps made that impossible to check:

- A crash was only reported when the game wrote a file into
  `crash-reports/`. The most common Windows crashes write none: the JVM dies
  itself (graphics drivers, native libraries) and writes `hs_err_pid<pid>.log`
  into its working folder, or a loader fails before the game starts and only
  prints to its output.
- Nothing was kept on disk: the launcher logged to stdout only, and the game
  output lived in the window until the next launch.

## Decision

- **Every failed exit is a crash.** A launch that ends with a non-zero exit
  the launcher did not cause shows the crash notice, with the exit code,
  whether or not the game left a report (ADR 0008 required a report).
- **JVM crash logs.** After such an exit, the newest `hs_err_pid*.log` in the
  game folder modified since the launch started (the same two seconds of
  clock skew as crash reports) is part of the outcome. `FileSystem::newest_file_since`
  takes a file name prefix so the game's other files (`options.txt` is
  rewritten on every exit) never count. The notice offers to open it; the
  command opens only a plain `hs_err_pid*.log` name, a regular file inside
  that instance's game folder.
- **Logs on disk** in `<data>/logs/`:
  - `launcher.log`: the launcher's `tracing` output, through the same
    redacting writer as stdout, without ANSI colors. The three previous runs
    are kept as `launcher.1.log` to `launcher.3.log`. Logging is set up once
    the data folder is known (Tauri `setup`).
  - `game-<instance>.log`: everything the game printed during its last
    launch, written line by line next to the live view (a pair of sinks is a
    sink). The previous launch is kept as `game-<instance>.1.log`. A file that
    cannot be created never stops the game.
  - The session token is masked in the game output before it reaches the
    screen or the file (`MaskingLogSink`), because some loaders print their
    arguments.
  - The crash notice and the About page open this folder.
- **Share report.** The crash notice publishes one text bundle and shows its
  link (`share_crash_report`): launcher version, system, instance, loader,
  Java choice, memory and exit code; the head of the crash report (160 KiB)
  and of the JVM crash log (128 KiB); and the tail of the game output
  (160 KiB), under the 512 KiB upload limit. Only the file names the launch
  outcome reported are accepted, checked again in the core. Before upload,
  `--accessToken <value>` and any JWT are masked and the JVM crash log's
  environment variables (which may hold API keys) are omitted. It reuses the
  diagnosis upload (pastes.dev, ADR 0015), one upload per click, and the
  notice says what the report includes and that anyone with the link can
  read it.

## Consequences

- A player can always find what happened, even after closing the launcher,
  and hand it over as one link.
- The shared bundle holds file paths with the player's system user name and
  the mod list; the notice says so before the player shares it.
- Disk use is bounded: four launcher logs and two output logs per instance.
