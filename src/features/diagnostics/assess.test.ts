import { describe, expect, it } from "vitest";
import { clock, dnsTone, latencyTone, lossTone, ms, percent, verdictWords } from "./assess";

describe("tones", () => {
  it("grades latency, DNS and loss", () => {
    expect([latencyTone(45), latencyTone(120), latencyTone(200), latencyTone(null)]).toEqual([
      "good",
      "warn",
      "bad",
      "unknown",
    ]);
    expect([dnsTone(6), dnsTone(300), dnsTone(900)]).toEqual(["good", "warn", "bad"]);
    expect([
      lossTone(0, 300),
      lossTone(1.3, 300),
      lossTone(2, 300),
      lossTone(10, 300),
      lossTone(0, 0),
    ]).toEqual(["good", "good", "warn", "bad", "unknown"]);
  });
});

describe("verdictWords", () => {
  it("puts an issue into where, what and what to do", () => {
    expect(verdictWords({ kind: "issue", issue: "jitter", zone: "home" })).toEqual({
      title: "diagnostics.verdict.where.home",
      issue: "diagnostics.verdict.issue.jitter",
      advice: "diagnostics.verdict.advice.home",
      tone: "warn",
    });
    expect(verdictWords({ kind: "issue", issue: "loss", zone: null }).title).toBe(
      "diagnostics.verdict.where.unknown",
    );
    expect(verdictWords({ kind: "issue", issue: "loss", zone: null }).tone).toBe("bad");
  });

  it("gives a healthy verdict a good tone and no issue", () => {
    expect(verdictWords({ kind: "healthy" })).toMatchObject({ tone: "good", issue: null });
  });
});

describe("formatting", () => {
  it("formats times, milliseconds and percentages", () => {
    expect([clock(0), clock(130), clock(600)]).toEqual(["0:00", "2:10", "10:00"]);
    expect([ms(45.97), ms(150.4), ms(null)]).toEqual(["46.0", "150", null]);
    expect([percent(0), percent(2.5)]).toEqual(["0 %", "2.5 %"]);
  });
});
