import { describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import enCommon from "../../i18n/locales/en/common.json";
import { cinepolDiagnostic } from "./fixtures";
import { renderReport } from "./reportText";

const en = enCommon.diagnostics.report;
const startedAt = new Date(2026, 10, 4, 18, 22);

function report(showPrivate: boolean, overrides = {}) {
  return renderReport({
    diagnostic: cinepolDiagnostic(overrides),
    startedAt,
    launcherVersion: "0.5.0",
    showPrivate,
  });
}

/** `UTC-05:00` for the machine running the tests. */
function utcOffset(date: Date): string {
  const minutes = -date.getTimezoneOffset();
  const sign = minutes < 0 ? "-" : "+";
  const abs = Math.abs(minutes);
  const two = (value: number) => String(value).padStart(2, "0");
  return `UTC${sign}${two(Math.floor(abs / 60))}:${two(abs % 60)}`;
}

describe("renderReport", () => {
  it("is written in English whatever the player's language", async () => {
    await i18n.changeLanguage("es");
    try {
      const text = report(false);
      expect(text).toContain(en.title);
      expect(text).toContain(`${en.assessment}: ${en.verdict.healthy}`);
    } finally {
      await i18n.changeLanguage("en");
    }
  });

  it("hides the client's local network and ISP by default", () => {
    const text = report(false);
    for (const hidden of ["192.168.68.1", "192.168.1.254", "190.248.0.245", "200.24.33.104"]) {
      expect(text).not.toContain(hidden);
    }
    expect(text).toContain("200.24.33.90");
    expect(text).toContain("103.195.101.252");
    expect(text).toContain(en.hopsHidden);
  });

  it("shows every hop when the player chooses to", () => {
    const text = report(true);
    expect(text).toContain("192.168.68.1");
    expect(text).toContain("190.248.0.245");
    expect(text).not.toContain(en.hidden.home);
    expect(text).toContain(en.hopsShown);
  });

  it("states the target, the start time with its offset, the duration and the client", () => {
    const text = report(false, { state: "stopped", elapsedSecs: 130 });
    expect(text).toContain(`${en.target}: mc.policarpiers.com`);
    expect(text).toContain(`${en.resolved}: 103.195.101.252:25565`);
    expect(text).toContain(`${en.started}: 2026-11-04 18:22 ${utcOffset(startedAt)}`);
    expect(text).toContain(`${en.duration}: 2:10 of 5:00 planned ${en.stoppedEarly}`);
    expect(text).toContain(`${en.client}: Cinepol Launcher 0.5.0`);
    expect(text).toContain(`${en.method}: ${en.methodText}`);
    expect(text).toContain("11/100 players, Velocity-CTD 1.21.11-26.3");
  });

  it("names the issue and where it starts in neutral terms", () => {
    const text = report(false, { verdict: { kind: "issue", issue: "loss", zone: "provider" } });
    const issue = en.verdict.issue.loss;
    const sentence = en.verdict.where.provider.replace("{{issue}}", issue);
    expect(text).toContain(
      `${en.assessment}: ${sentence.charAt(0).toUpperCase()}${sentence.slice(1)}`,
    );
    expect(text).not.toContain("your");
  });

  it("aligns the hop table, labels each zone and marks silent hops", () => {
    const lines = report(false).split("\n");
    const header = lines.findIndex((line) => line.startsWith(" #"));
    const rows = lines.slice(header + 1, header + 9);
    expect(rows).toHaveLength(8);
    expect(rows[0]).toContain(en.zone.home);
    expect(rows[2]).toContain(en.zone.provider);
    expect(rows[5]).toContain(en.zone.internet);
    expect(rows[7]).toContain(en.zone.server);
    expect(rows[4]).toContain(en.noReply);
    expect(rows[4]).toContain("100 %");
    const width = lines[header]?.length ?? 0;
    for (const row of rows) expect(row.length).toBeLessThanOrEqual(width);
    expect(lines[header]?.indexOf(en.columns.loss)).toBeGreaterThan(0);
  });

  it("says when hops could not be measured", () => {
    const text = report(false, { hopProbingUnavailable: "not permitted", hops: [] });
    expect(text).toContain(en.unavailable);
  });
});
