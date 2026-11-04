import { describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import enCommon from "../../i18n/locales/en/common.json";
import esCommon from "../../i18n/locales/es/common.json";
import { cinepolDiagnostic } from "./fixtures";
import { renderReport } from "./reportText";

const es = esCommon.diagnostics;
const startedAt = new Date(2026, 10, 4, 18, 22);

function report(showPrivate: boolean, overrides = {}, language = "es") {
  const t = i18n.getFixedT(language);
  return renderReport(t, {
    diagnostic: cinepolDiagnostic(overrides),
    startedAt,
    launcherVersion: "0.5.0",
    showPrivate,
  });
}

describe("renderReport", () => {
  it("hides the player's router and provider by default", () => {
    const text = report(false);
    expect(text).toContain(es.hops.hidden.home);
    expect(text).toContain(es.hops.hidden.provider);
    for (const hidden of ["192.168.68.1", "192.168.1.254", "190.248.0.245", "200.24.33.104"]) {
      expect(text).not.toContain(hidden);
    }
    expect(text).toContain("200.24.33.90");
    expect(text).toContain("103.195.101.252");
    expect(text).toContain(es.report.hopsHidden);
  });

  it("shows every hop when the player chooses to", () => {
    const text = report(true);
    expect(text).toContain("192.168.68.1");
    expect(text).toContain("190.248.0.245");
    expect(text).not.toContain(es.hops.hidden.home);
    expect(text).toContain(es.report.hopsShown);
  });

  it("states the server, the date, how long it ran and the verdict", () => {
    const text = report(false, { state: "stopped", elapsedSecs: 130 });
    const ran = es.report.ran.replace("{{elapsed}}", "2:10").replace("{{planned}}", "5:00");
    expect(text).toContain(`${es.report.server}: mc.policarpiers.com (103.195.101.252:25565)`);
    expect(text).toContain(`${es.report.date}: 2026-11-04 18:22`);
    expect(text).toContain(`${es.report.duration}: ${ran} ${es.report.stoppedEarly}`);
    expect(text).toContain(`${es.report.launcher}: Cinepol Launcher 0.5.0`);
    expect(text).toContain(`${es.report.verdict}: ${es.verdict.healthy}`);
    expect(text).toContain(`11/100 ${es.report.players}, Velocity-CTD 1.21.11-26.3`);
  });

  it("names the zone and the issue of a problem", () => {
    const text = report(false, { verdict: { kind: "issue", issue: "loss", zone: "provider" } });
    expect(text).toContain(
      `${es.report.verdict}: ${es.verdict.where.provider} ${es.verdict.issue.loss}`,
    );
  });

  it("aligns the hop table and marks silent hops", () => {
    const lines = report(false).split("\n");
    const header = lines.findIndex((line) => line.startsWith(" #"));
    const rows = lines.slice(header + 1, header + 9);
    expect(rows).toHaveLength(8);
    expect(rows[4]).toContain(es.hops.noAnswer);
    expect(rows[4]).toContain("100 %");
    const width = lines[header]?.length ?? 0;
    for (const row of rows) expect(row.length).toBeLessThanOrEqual(width);
    expect(lines[header]?.indexOf(es.hops.loss)).toBeGreaterThan(0);
  });

  it("says when hops could not be measured", () => {
    const text = report(false, { hopProbingUnavailable: "not permitted", hops: [] });
    expect(text).toContain(es.hops.unavailable);
  });

  it("writes the report in the player's language", () => {
    const text = report(false, {}, "en");
    expect(text).toContain(enCommon.diagnostics.report.title);
    expect(text).toContain(enCommon.diagnostics.hops.hidden.home);
  });
});
