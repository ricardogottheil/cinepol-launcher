import { i18n } from "../../i18n";
import type { DiagnosticDto, HopDto, VerdictDto } from "../../ipc/client";
import { clock, ms, percent } from "./assess";

export interface ReportContext {
  diagnostic: DiagnosticDto;
  /** When the test started. */
  startedAt: Date;
  launcherVersion: string | null;
  /** Show the client's local network and ISP hops instead of hiding them. */
  showPrivate: boolean;
}

/** The report goes to server hosts and ISPs, so it is always English. */
const t = i18n.getFixedT("en", "common", "diagnostics.report");

function pad(value: string, width: number, end = false): string {
  return end ? value.padEnd(width) : value.padStart(width);
}

function cell(value: string | null): string {
  return value ?? "-";
}

/** `2026-11-04 18:22 UTC-05:00`: local time with its offset, for readers anywhere. */
function timestamp(date: Date): string {
  const two = (value: number) => String(value).padStart(2, "0");
  const offset = -date.getTimezoneOffset();
  const abs = Math.abs(offset);
  const zone = `UTC${offset < 0 ? "-" : "+"}${two(Math.floor(abs / 60))}:${two(abs % 60)}`;
  return `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())} ${two(
    date.getHours(),
  )}:${two(date.getMinutes())} ${zone}`;
}

function assessment(verdict: VerdictDto): string {
  if (verdict.kind !== "issue") return t(`verdict.${verdict.kind}`);
  const issue = t(`verdict.issue.${verdict.issue}`);
  const where = t(`verdict.where.${verdict.zone ?? "unknown"}`, { issue });
  return where.charAt(0).toUpperCase() + where.slice(1);
}

function hopHost(hop: HopDto, showPrivate: boolean): string {
  if (hop.privateToPlayer && !showPrivate) return t(`hidden.${hop.zone}`);
  return hop.address ?? t("noReply");
}

function hopTable(diagnostic: DiagnosticDto, showPrivate: boolean): string[] {
  const hosts = diagnostic.hops.map((hop) => hopHost(hop, showPrivate));
  const zones = diagnostic.hops.map((hop) => t(`zone.${hop.zone}`));
  const hostWidth = Math.max(t("columns.host").length, ...hosts.map((h) => h.length));
  const zoneWidth = Math.max(t("columns.zone").length, ...zones.map((z) => z.length));
  const numeric = ["loss", "sent", "last", "average", "best", "worst", "jitter"] as const;
  const widths = [7, 5, 7, 7, 7, 7, 7];
  const header = [
    pad("#", 2),
    pad(t("columns.host"), hostWidth, true),
    pad(t("columns.zone"), zoneWidth, true),
    ...numeric.map((column, index) => pad(t(`columns.${column}`), widths[index] ?? 7)),
  ];
  const rows = diagnostic.hops.map((hop, index) => {
    const s = hop.stats;
    const values = [
      percent(s.lossPercent),
      String(s.sent),
      cell(ms(s.lastMs)),
      cell(ms(s.averageMs)),
      cell(ms(s.bestMs)),
      cell(ms(s.worstMs)),
      cell(ms(s.jitterMs)),
    ];
    return [
      pad(String(hop.ttl), 2),
      pad(hosts[index] ?? "", hostWidth, true),
      pad(zones[index] ?? "", zoneWidth, true),
      ...values.map((value, column) => pad(value, widths[column] ?? 7)),
    ]
      .join("  ")
      .trimEnd();
  });
  return [header.join("  "), ...rows];
}

/**
 * The report a player shares with the server's host or their ISP: plain
 * monospace text (mclo.gs shows it as a log), always in neutral English, with
 * the client's local network and ISP hidden unless the player chose to show them.
 */
export function renderReport(context: ReportContext): string {
  const { diagnostic, showPrivate } = context;
  const lines: string[] = [];
  const title = t("title");
  lines.push(title, "=".repeat(title.length));
  lines.push(`${t("target")}: ${diagnostic.server}`);
  if (diagnostic.address) lines.push(`${t("resolved")}: ${diagnostic.address}`);
  lines.push(`${t("started")}: ${timestamp(context.startedAt)}`);
  const ran = t("ran", {
    elapsed: clock(diagnostic.elapsedSecs),
    planned: clock(diagnostic.plannedSecs),
  });
  const stopped = diagnostic.state === "stopped" ? ` ${t("stoppedEarly")}` : "";
  lines.push(`${t("duration")}: ${ran}${stopped}`);
  if (context.launcherVersion) {
    lines.push(`${t("client")}: Cinepol Launcher ${context.launcherVersion}`);
  }
  lines.push(`${t("method")}: ${t("methodText")}`);
  lines.push("");
  lines.push(`${t("assessment")}: ${assessment(diagnostic.verdict)}`);
  lines.push("");

  const status = diagnostic.serverStatus;
  const destinationJitter = diagnostic.destinationReached
    ? diagnostic.hops.at(-1)?.stats.jitterMs
    : diagnostic.tcp.jitterMs;
  lines.push(`${t("summary")}:`);
  lines.push(`  ${t("dns")}: ${cell(ms(diagnostic.dnsMs))} ms`);
  lines.push(
    `  ${t("tcp")}: ${cell(ms(diagnostic.tcp.averageMs))} ms, ${t("failed", {
      loss: percent(diagnostic.tcp.lossPercent),
    })}`,
  );
  const players = status
    ? `, ${t("players", { online: status.onlinePlayers, max: status.maxPlayers })}, ${status.versionName}`
    : "";
  lines.push(`  ${t("slp")}: ${cell(ms(diagnostic.minecraft.averageMs))} ms${players}`);
  lines.push(
    `  ${t("destinationLoss")}: ${t("lost", { loss: percent(diagnostic.destinationLossPercent) })}, ${t(
      "jitter",
      { jitter: cell(ms(destinationJitter)) },
    )}`,
  );
  lines.push("");

  if (diagnostic.hopProbingUnavailable !== null) {
    lines.push(t("unavailable"));
    return `${lines.join("\n")}\n`;
  }
  lines.push(`${t("hops")} (${showPrivate ? t("hopsShown") : t("hopsHidden")})`);
  lines.push(...hopTable(diagnostic, showPrivate));
  lines.push("");
  lines.push(t("notes"));
  return `${lines.join("\n")}\n`;
}
