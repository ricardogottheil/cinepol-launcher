import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { detectPlatform } from "../../ipc/window";
import { TitleBar } from "./TitleBar";

describe("TitleBar", () => {
  it.each(["windows", "linux"] as const)("draws window controls on %s", (platform) => {
    render(<TitleBar platform={platform} />);
    for (const name of [
      esCommon.chrome.minimize,
      esCommon.chrome.maximize,
      esCommon.chrome.close,
    ]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
  });

  it("keeps the native traffic lights on macOS", () => {
    render(<TitleBar platform="macos" />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("is a drag region", () => {
    render(<TitleBar platform="windows" />);
    expect(screen.getByRole("banner")).toHaveAttribute("data-tauri-drag-region");
  });
});

describe("detectPlatform", () => {
  it.each([
    ["Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15", "macos"],
    ["Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36", "windows"],
    ["Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/605.1.15", "linux"],
  ])("maps %s", (userAgent, expected) => {
    expect(detectPlatform(userAgent)).toBe(expected);
  });
});
