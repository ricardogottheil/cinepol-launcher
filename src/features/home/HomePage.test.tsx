import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import serverConfig from "../../../config/server-config.json";
import esCommon from "../../i18n/locales/es/common.json";
import { HomePage } from "./HomePage";

describe("HomePage", () => {
  it("offers the play control with the server address from config", () => {
    render(<HomePage />);
    const play = screen.getByRole("button", { name: new RegExp(esCommon.home.play) });
    expect(play).toHaveTextContent(serverConfig.server.address);
  });

  it("reads the supported and recommended versions from config", () => {
    render(<HomePage />);
    const { min, max, recommended } = serverConfig.versions;
    expect(screen.getByText(`${min} – ${max}`)).toBeInTheDocument();
    expect(screen.getByText(recommended)).toBeInTheDocument();
  });

  it("shows an em dash and the status note when the status is unknown", () => {
    const { container } = render(<HomePage status="unavailable" />);
    expect(screen.getByText("—")).toBeInTheDocument();
    expect(screen.getByText(esCommon.home.status.unavailable)).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).toBeNull();
  });

  it("lights the lamp and shows the count when the server answers", () => {
    const { container } = render(<HomePage status="online" onlinePlayers={8} />);
    expect(screen.getByText("8")).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).not.toBeNull();
  });
});
