import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { SettingsPage } from "./SettingsPage";

const DEFAULT_URL = "https://cinepol.policarpiers.com/server-config.json";

const client = vi.hoisted(() => ({
  getSettings: vi.fn(),
  getServerConfig: vi.fn(),
  setServerConfigUrl: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

beforeEach(() => {
  client.getSettings.mockResolvedValue({
    serverConfigUrl: "https://example.test/config.json",
    defaultServerConfigUrl: DEFAULT_URL,
  });
  client.getServerConfig.mockRejectedValue(new IpcError("internal", "no backend"));
  client.setServerConfigUrl.mockImplementation(async (url: string | null) => ({
    serverConfigUrl: url ?? DEFAULT_URL,
    defaultServerConfigUrl: DEFAULT_URL,
  }));
});

/** The field once the saved URL has loaded into it. */
async function field() {
  const input = await screen.findByLabelText(esCommon.settings.urlLabel);
  await waitFor(() => expect(input).toHaveValue("https://example.test/config.json"));
  return input;
}

describe("SettingsPage", () => {
  it("shows the saved URL and which config is in use", async () => {
    renderWithQuery(<SettingsPage />);
    expect(await field()).toHaveValue("https://example.test/config.json");
    expect(screen.getByText(esCommon.settings.origin.bundled)).toBeInTheDocument();
  });

  it("saves the trimmed URL", async () => {
    renderWithQuery(<SettingsPage />);
    fireEvent.change(await field(), { target: { value: "  https://other.test/c.json " } });
    fireEvent.click(screen.getByRole("button", { name: esCommon.settings.save }));
    await waitFor(() =>
      expect(client.setServerConfigUrl).toHaveBeenCalledWith("https://other.test/c.json"),
    );
    expect(await screen.findByText(esCommon.settings.saved)).toBeInTheDocument();
  });

  it("resets to the default URL", async () => {
    renderWithQuery(<SettingsPage />);
    await field();
    fireEvent.click(screen.getByRole("button", { name: esCommon.settings.reset }));
    await waitFor(() => expect(client.setServerConfigUrl).toHaveBeenCalledWith(null));
  });

  it("shows the backend error under the field", async () => {
    client.setServerConfigUrl.mockRejectedValue(new IpcError("invalid_input", "not https"));
    renderWithQuery(<SettingsPage />);
    fireEvent.change(await field(), { target: { value: "http://insecure.test" } });
    fireEvent.click(screen.getByRole("button", { name: esCommon.settings.save }));
    expect(await screen.findByText(esCommon.errors.invalid_input)).toBeInTheDocument();
  });
});
