import type { HopZoneDto, IssueKindDto, VerdictDto } from "../../ipc/client";

/** How a reading looks to the player: a lamp colour plus words, never colour alone. */
export type Tone = "good" | "warn" | "bad" | "unknown";

/** A round trip in milliseconds: under 80 ms plays smooth, over 150 ms feels laggy. */
export function latencyTone(ms: number | null | undefined): Tone {
  if (ms === null || ms === undefined) return "unknown";
  if (ms < 80) return "good";
  if (ms < 150) return "warn";
  return "bad";
}

/** Name resolution happens once per join; only very slow lookups matter. */
export function dnsTone(ms: number | null | undefined): Tone {
  if (ms === null || ms === undefined) return "unknown";
  if (ms < 150) return "good";
  if (ms < 500) return "warn";
  return "bad";
}

/** Lost packets in percent; nothing to say before a probe was sent. */
export function lossTone(percent: number, sent: number): Tone {
  if (sent === 0) return "unknown";
  if (percent < 1) return "good";
  if (percent < 5) return "warn";
  return "bad";
}

export type VerdictZone = HopZoneDto | "unknown";

/** The i18n keys and tone that put a verdict into words. */
export interface VerdictWords {
  title:
    | "diagnostics.verdict.measuring"
    | "diagnostics.verdict.healthy"
    | "diagnostics.verdict.nameNotFound"
    | "diagnostics.verdict.unreachable"
    | `diagnostics.verdict.where.${VerdictZone}`;
  /** What is wrong, for an issue. */
  issue: `diagnostics.verdict.issue.${IssueKindDto}` | null;
  /** What the player can do. */
  advice:
    | `diagnostics.verdict.advice.${VerdictZone}`
    | "diagnostics.verdict.advice.healthy"
    | "diagnostics.verdict.advice.nameNotFound"
    | "diagnostics.verdict.advice.unreachable"
    | null;
  tone: Tone;
}

export function verdictWords(verdict: VerdictDto): VerdictWords {
  switch (verdict.kind) {
    case "measuring":
      return { title: "diagnostics.verdict.measuring", issue: null, advice: null, tone: "unknown" };
    case "healthy":
      return {
        title: "diagnostics.verdict.healthy",
        issue: null,
        advice: "diagnostics.verdict.advice.healthy",
        tone: "good",
      };
    case "nameNotFound":
      return {
        title: "diagnostics.verdict.nameNotFound",
        issue: null,
        advice: "diagnostics.verdict.advice.nameNotFound",
        tone: "bad",
      };
    case "unreachable":
      return {
        title: "diagnostics.verdict.unreachable",
        issue: null,
        advice: "diagnostics.verdict.advice.unreachable",
        tone: "bad",
      };
    case "issue": {
      const zone: VerdictZone = verdict.zone ?? "unknown";
      return {
        title: `diagnostics.verdict.where.${zone}`,
        issue: `diagnostics.verdict.issue.${verdict.issue}`,
        advice: `diagnostics.verdict.advice.${zone}`,
        tone: verdict.issue === "loss" ? "bad" : "warn",
      };
    }
  }
}

/** "4:05" for 245 seconds. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.round(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/** A millisecond reading with one decimal below 100 ms, none above. */
export function ms(value: number | null | undefined): string | null {
  if (value === null || value === undefined) return null;
  return value < 100 ? value.toFixed(1) : value.toFixed(0);
}

/** A loss percentage, e.g. "0 %" or "2.5 %". */
export function percent(value: number): string {
  return Number.isInteger(value) ? `${value} %` : `${value.toFixed(1)} %`;
}
