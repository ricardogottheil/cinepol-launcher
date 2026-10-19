import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import type { LogLineDto } from "../../ipc/client";
import { LogPanel } from "./LogPanel";

const labels = esCommon.instances.log;

const line = (overrides: Partial<LogLineDto>): LogLineDto => ({
  stream: "stdout",
  line: "",
  level: null,
  thread: null,
  logger: null,
  ...overrides,
});

const lines: LogLineDto[] = [
  line({ line: "Setting user: polispol1", level: "info", thread: "Render thread" }),
  line({ line: "Missing texture minecraft:x", level: "warn", thread: "Worker-Main-2" }),
  line({ line: "Mixin apply failed", level: "error", thread: "main", logger: "mixin" }),
  line({ line: "plain JVM line" }),
  line({ stream: "stderr", line: "Exception in thread main" }),
];

function shown() {
  return within(screen.getByRole("log"))
    .queryAllByTestId("log-line")
    .map((element) => element.textContent ?? "");
}

describe("LogPanel", () => {
  it("shows the level and thread of log4j lines", () => {
    render(<LogPanel lines={lines} />);
    const first = shown()[0] ?? "";
    expect(first).toContain("INFO");
    expect(first).toContain("Render thread");
    expect(first).toContain("Setting user: polispol1");
    expect(shown()).toHaveLength(5);
  });

  it("filters by minimum level, counting stderr lines as errors", () => {
    render(<LogPanel lines={lines} />);
    fireEvent.change(screen.getByLabelText(labels.level), { target: { value: "warn" } });
    expect(shown()).toHaveLength(3);
    fireEvent.change(screen.getByLabelText(labels.level), { target: { value: "error" } });
    const errors = shown();
    expect(errors).toHaveLength(2);
    expect(errors.join("\n")).toContain("Mixin apply failed");
    expect(errors.join("\n")).toContain("Exception in thread main");
    expect(
      screen.getByText(labels.count.replace("{{shown}}", "2").replace("{{total}}", "5")),
    ).toBeInTheDocument();
  });

  it("searches the message, thread and logger, ignoring case", () => {
    render(<LogPanel lines={lines} />);
    fireEvent.change(screen.getByLabelText(labels.search), { target: { value: "MIXIN" } });
    expect(shown()).toEqual([expect.stringContaining("Mixin apply failed")]);
    fireEvent.change(screen.getByLabelText(labels.search), { target: { value: "worker-main" } });
    expect(shown()).toEqual([expect.stringContaining("Missing texture")]);
  });
});
