import type { DiagnosticDto, HopDto, LatencyStatsDto } from "../../ipc/client";

export function stats(overrides: Partial<LatencyStatsDto> = {}): LatencyStatsDto {
  return {
    sent: 300,
    received: 300,
    lossPercent: 0,
    lastMs: 41.2,
    bestMs: 40.1,
    worstMs: 63.5,
    averageMs: 45.97,
    jitterMs: 1.4,
    history: [41.2, null, 40.9],
    ...overrides,
  };
}

function hop(ttl: number, address: string | null, zone: HopDto["zone"], ms: number): HopDto {
  return {
    ttl,
    address,
    zone,
    privateToPlayer: zone === "home" || zone === "provider",
    stats:
      address === null
        ? stats({
            received: 0,
            lossPercent: 100,
            averageMs: null,
            lastMs: null,
            bestMs: null,
            worstMs: null,
            jitterMs: null,
          })
        : stats({ averageMs: ms }),
  };
}

/** The real path from Colombia to mc.policarpiers.com, after 5 minutes. */
export function cinepolDiagnostic(overrides: Partial<DiagnosticDto> = {}): DiagnosticDto {
  return {
    server: "mc.policarpiers.com",
    host: "mc.policarpiers.com",
    address: "103.195.101.252:25565",
    dnsMs: 6.4,
    tcp: stats({ averageMs: 59.7 }),
    minecraft: stats({ sent: 60, received: 60, averageMs: 49.25 }),
    serverStatus: { onlinePlayers: 11, maxPlayers: 100, versionName: "Velocity-CTD 1.21.11-26.3" },
    hops: [
      hop(1, "192.168.68.1", "home", 10.9),
      hop(2, "192.168.1.254", "home", 10.5),
      hop(3, "190.248.0.245", "provider", 16.3),
      hop(4, "200.24.33.104", "provider", 13.2),
      hop(5, null, "internet", 0),
      hop(6, "200.24.33.90", "internet", 61.7),
      hop(7, "206.41.108.129", "server", 60.0),
      hop(8, "103.195.101.252", "server", 45.97),
    ],
    hopProbingUnavailable: null,
    destinationReached: true,
    destinationLossPercent: 0,
    elapsedSecs: 300,
    plannedSecs: 300,
    state: "finished",
    verdict: { kind: "healthy" },
    ...overrides,
  };
}
