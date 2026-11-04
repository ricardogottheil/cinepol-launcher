import type { TFunction } from "i18next";
import type { DiagnosticDto, HopDto } from "../../ipc/client";
import { clock, ms, percent, verdictWords } from "./assess";

export interface ReportContext {
  diagnostic: DiagnosticDto;
  /** When the test started. */
  startedAt: Date;
  launcherVersion: string | null;
  /** Show the player's router and provider hops instead of hiding them. */
  showPrivate: boolean;
}

function pad(value: string, width: number, end = false): string {
  return end ? value.padEnd(width) : value.padStart(width);
}

function cell(value: string | null): string {
  return value ?? "-";
}

/** `2026-11-04 18:22` in the player's local time. */
function timestamp(date: Date): string {
  const two = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())} ${two(
    date.getHours(),
  )}:${two(date.getMinutes())}`;
}

function hopHost(t: TFunction, hop: HopDto, showPrivate: boolean): string {
  if (hop.privateToPlayer && !showPrivate) {
    return t(`diagnostics.hops.hidden.${hop.zone}` as "diagnostics.hops.hidden.home");
  }
  return hop.address ?? t("diagnostics.hops.noAnswer");
}

/**
 * The report a player shares with the staff: plain monospace text (mclo.gs
 * shows it as a log), in the player's language, with their router and
 * provider hidden unless they chose to show them.
 */
export function renderReport(t: TFunction, context: ReportContext): string {
  const { diagnostic, showPrivate } = context;
  const words = verdictWords(diagnostic.verdict);
  const lines: string[] = [];
  lines.push(t("diagnostics.report.title"));
  lines.push("=".repeat(t("diagnostics.report.title").length));
  const target = diagnostic.address ? ` (${diagnostic.address})` : "";
  lines.push(`${t("diagnostics.report.server")}: ${diagnostic.server}${target}`);
  lines.push(`${t("diagnostics.report.date")}: ${timestamp(context.startedAt)}`);
  const ran = t("diagnostics.report.ran", {
    elapsed: clock(diagnostic.elapsedSecs),
    planned: clock(diagnostic.plannedSecs),
  });
  const stopped = diagnostic.state === "stopped" ? ` ${t("diagnostics.report.stoppedEarly")}` : "";
  lines.push(`${t("diagnostics.report.duration")}: ${ran}${stopped}`);
  if (context.launcherVersion) {
    lines.push(`${t("diagnostics.report.launcher")}: Cinepol Launcher ${context.launcherVersion}`);
  }
  lines.push("");
  const verdict = [t(words.title), words.issue ? t(words.issue) : null].filter(Boolean).join(" ");
  lines.push(`${t("diagnostics.report.verdict")}: ${verdict}`);
  lines.push("");

  const status = diagnostic.serverStatus;
  lines.push(`${t("diagnostics.cards.dns")}: ${cell(ms(diagnostic.dnsMs))} ms`);
  lines.push(
    `${t("diagnostics.cards.tcp")}: ${cell(ms(diagnostic.tcp.averageMs))} ms, ${t(
      "diagnostics.report.loss",
    )} ${percent(diagnostic.tcp.lossPercent)}`,
  );
  lines.push(
    `${t("diagnostics.cards.minecraft")}: ${cell(ms(diagnostic.minecraft.averageMs))} ms${
      status
        ? `, ${status.onlinePlayers}/${status.maxPlayers} ${t("diagnostics.report.players")}, ${
            status.versionName
          }`
        : ""
    }`,
  );
  lines.push(
    `${t("diagnostics.cards.loss")}: ${percent(diagnostic.destinationLossPercent)} (${t(
      "diagnostics.report.jitter",
    )} ${cell(ms(diagnostic.destinationReached ? diagnostic.hops.at(-1)?.stats.jitterMs : diagnostic.tcp.jitterMs))} ms)`,
  );
  lines.push("");

  if (diagnostic.hopProbingUnavailable !== null) {
    lines.push(t("diagnostics.hops.unavailable"));
    return `${lines.join("\n")}\n`;
  }
  lines.push(showPrivate ? t("diagnostics.report.hopsShown") : t("diagnostics.report.hopsHidden"));
  const hosts = diagnostic.hops.map((hop) => hopHost(t, hop, showPrivate));
  const hostWidth = Math.max(t("diagnostics.hops.host").length, ...hosts.map((h) => h.length));
  const headers = [
    pad("#", 2),
    pad(t("diagnostics.hops.host"), hostWidth, true),
    pad(t("diagnostics.hops.loss"), 7),
    pad(t("diagnostics.hops.sent"), 5),
    pad(t("diagnostics.hops.last"), 7),
    pad(t("diagnostics.hops.average"), 7),
    pad(t("diagnostics.hops.best"), 7),
    pad(t("diagnostics.hops.worst"), 7),
    pad(t("diagnostics.hops.jitter"), 7),
  ];
  lines.push(headers.join("  "));
  diagnostic.hops.forEach((hop, index) => {
    lines.push(
      [
        pad(String(hop.ttl), 2),
        pad(hosts[index] ?? "", hostWidth, true),
        pad(percent(hop.stats.lossPercent), 7),
        pad(String(hop.stats.sent), 5),
        pad(cell(ms(hop.stats.lastMs)), 7),
        pad(cell(ms(hop.stats.averageMs)), 7),
        pad(cell(ms(hop.stats.bestMs)), 7),
        pad(cell(ms(hop.stats.worstMs)), 7),
        pad(cell(ms(hop.stats.jitterMs)), 7),
      ]
        .join("  ")
        .trimEnd(),
    );
  });
  return `${lines.join("\n")}\n`;
}
