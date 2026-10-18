import { describe, expect, it } from "vitest";
import { formatBytes } from "./formatBytes";

describe("formatBytes", () => {
  it("reads downloaded bytes as megabytes with one decimal", () => {
    expect(formatBytes(43_515_908)).toBe("41.5");
    expect(formatBytes(126_583_468)).toBe("120.7");
    expect(formatBytes(0)).toBe("0.0");
  });
});
