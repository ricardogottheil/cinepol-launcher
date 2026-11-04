# ADR 0015: Network diagnostics

- Status: accepted
- Date: 2026-11-04

## Context

Players often report a high ping. Sometimes the cause is the server's host,
and sometimes it is the player's Wi-Fi or internet provider. Staff need
evidence, and most players have never run `mtr` or `tracert`. The launcher
should run that test for any server and explain the result in plain words.

## Decision

- **MTR-style rounds.** `diagnose_connection` runs one round per second. Each
  round sends one ICMP echo per hop (TTL 1 to 30, all at once), times one TCP
  handshake with the server's port, and runs a Server List Ping every fifth
  round. A round lasts at least one second, and every probe waits at most
  one second. Once the server answers at TTL *n*, later rounds probe only up
  to *n*. When the server never answers ICMP, probing stops one hop after the
  last hop that answered, starting with the third round.
- **Durations.** 1, 5 (default) or 10 minutes. Intermittent loss needs a few
  hundred samples to show. Stopping early is a result too: the report says
  how long the test ran out of the planned time.
- **No administrator rights.**
  - macOS and Linux use unprivileged ICMP sockets (`SOCK_DGRAM` +
    `IPPROTO_ICMP`). macOS always allows them. Linux allows them for the
    groups in `net.ipv4.ping_group_range`, which covers every user on systemd
    systems. Linux delivers "time exceeded" through the socket's error queue
    (`IP_RECVERR` + `MSG_ERRQUEUE`), not as a normal read.
  - Windows uses `IcmpSendEcho`, the API `tracert` and `pathping` use.
  - We do not run `tracert` or `traceroute`: their output is localized on
    Windows and differs on every OS.
  - Replies match on the probe's sequence number and a payload tag, because
    kernels may rewrite the identifier.
  - Hop probing is IPv4 only. When the system refuses ICMP, the diagnosis
    goes on without hops, using the TCP connects, and says so.
- **DNS** is the host lookup the game does, after the SRV record. The SRV
  resolver's first query also pays for building the resolver (seconds on
  macOS), so it is not part of the DNS reading.
- **Verdict.** The core classifies hops into zones:
  - **home**: private addresses before the first public hop;
  - **provider**: carrier-grade NAT, private hops after the first public hop,
    and the first two public hops;
  - **internet**: the hops in between;
  - **server**: the last two hops when the path reached the server.

  The "destination" is the server's own hop when it answers ICMP, and the TCP
  connects otherwise. Loss (2 % or more) outranks latency (an average of
  150 ms or more), which outranks jitter (30 ms or more). An issue is placed
  at the first hop where it starts **and carries on to every later hop that
  answers**. Loss or delay at a single router is how routers deprioritize
  answering probes, and is ignored. Ten samples come first; a port that never
  accepts a connection is "unreachable".
- **Report.** The frontend writes the report as plain monospace text, always
  in neutral English whatever the UI language, because players send it to the
  server's host or to their ISP. It states the target, the start time with its
  UTC offset, the method, an assessment in third person, and a hop table with
  a zone column (LAN, ISP, Transit, Server). The core uploads it unchanged to mclo.gs
  (`POST https://api.mclo.gs/1/log`, JSON `content` and `source`), and links
  expire after 90 days. "Copy as text" copies the same report. The home and
  provider hops are hidden by default, in the report and in the table; one
  toggle shows both.

## Consequences

- New platform dependencies: `socket2` and `libc` (Unix), and `windows-sys`
  (Windows). All three versions were already in the lockfile.
- `cargo test -p cinepol-launcher-core --features network-tests --test diagnostics_network -- --nocapture`
  traces the real path to CINEPOL for one minute. It was verified on macOS
  and on Linux as a non-root user. The Windows adapter only type-checks
  against `windows-sys` from macOS, so a real Windows run is still pending.
- The zone boundaries are heuristics: a provider whose first hops are not
  where the boundary assumes is classified one hop off. The table always
  shows the raw numbers, so staff can judge for themselves.
