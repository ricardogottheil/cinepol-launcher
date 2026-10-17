import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import enCommon from "../i18n/locales/en/common.json";
import esCommon from "../i18n/locales/es/common.json";
import { App } from "./App";

describe("App", () => {
  it("shows the product name", () => {
    render(<App />);
    expect(screen.getByRole("heading", { level: 1, name: "Cinepol Launcher" })).toBeInTheDocument();
  });

  it("uses Spanish as the default display locale", () => {
    render(<App />);
    expect(screen.getByText(esCommon.legal.disclaimer)).toBeInTheDocument();
    expect(screen.queryByText(enCommon.legal.disclaimer)).not.toBeInTheDocument();
  });
});
