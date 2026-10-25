import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { renderWithQuery } from "../../test/render";
import { SettingsPage } from "./SettingsPage";

const client = vi.hoisted(() => ({
  getAppInfo: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

beforeEach(() => {
  client.getAppInfo.mockResolvedValue({ version: "0.2.0" });
});

describe("SettingsPage", () => {
  it("shows the version and the legal notice", async () => {
    renderWithQuery(<SettingsPage />);
    expect(await screen.findByText("0.2.0")).toBeInTheDocument();
    expect(screen.getByText(esCommon.legal.disclaimer)).toBeInTheDocument();
  });

  it("does not offer to change where the server configuration comes from", () => {
    renderWithQuery(<SettingsPage />);
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByText(/server-config\.json/)).not.toBeInTheDocument();
  });
});
