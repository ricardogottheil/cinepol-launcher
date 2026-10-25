import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type ProgressEvent } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { useAppUpdateInstall } from "./appUpdateStore";
import { UpdatePage } from "./UpdatePage";

const client = vi.hoisted(() => ({
  checkAppUpdate: vi.fn(),
  installAppUpdate: vi.fn(),
  getAppInfo: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.updates;
const available = {
  kind: "available",
  version: "0.3.0",
  currentVersion: "0.2.0",
  notes: "Cinepol Launcher 0.3.0\n\n- Servers on Home\n\n---\n\n- Servidores en Inicio\n- Avisos",
} as const;

function running(done: Promise<void>) {
  return { operationId: "op", done, cancel: vi.fn() };
}

const installButton = () => screen.findByRole("button", { name: new RegExp(labels.install) });

describe("UpdatePage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAppUpdateInstall.setState({ installing: false, progress: null, error: null });
    client.getAppInfo.mockResolvedValue({ name: "Cinepol Launcher", version: "0.2.0" });
  });

  it("says the launcher is up to date with its version", async () => {
    client.checkAppUpdate.mockResolvedValue({ kind: "upToDate" });
    renderWithQuery(<UpdatePage />);
    expect(await screen.findByText(labels.upToDate)).toBeInTheDocument();
    expect(
      await screen.findByText(labels.upToDateDetail.replace("{{version}}", "0.2.0")),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: new RegExp(labels.install) })).toBeNull();
  });

  it("says a build that cannot update does not update itself", async () => {
    client.checkAppUpdate.mockResolvedValue({ kind: "notConfigured" });
    renderWithQuery(<UpdatePage />);
    expect(await screen.findByText(labels.notConfigured)).toBeInTheDocument();
  });

  it("shows both versions and the notes in the player's language as a list", async () => {
    client.checkAppUpdate.mockResolvedValue(available);
    renderWithQuery(<UpdatePage />);
    await installButton();
    expect(screen.getByText("0.2.0")).toBeInTheDocument();
    expect(screen.getByText("0.3.0")).toBeInTheDocument();
    const notes = screen.getByRole("region", { name: labels.whatsNew });
    expect(
      within(notes)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["Servidores en Inicio", "Avisos"]);
    expect(within(notes).queryByText("---")).toBeNull();
    expect(within(notes).queryByText(/Servers on Home/)).toBeNull();
  });

  it("installs exactly the offered version and shows its progress", async () => {
    client.checkAppUpdate.mockResolvedValue(available);
    let report: (event: ProgressEvent) => void = () => {};
    client.installAppUpdate.mockImplementation(
      (_version: string, onProgress: (event: ProgressEvent) => void) => {
        report = onProgress;
        return running(new Promise(() => {}));
      },
    );
    renderWithQuery(<UpdatePage />);
    fireEvent.click(await installButton());

    expect(client.installAppUpdate).toHaveBeenCalledWith("0.3.0", expect.any(Function));
    expect(screen.queryByRole("button", { name: new RegExp(labels.install) })).toBeNull();
    act(() => report({ done: 5, total: 10, stage: null, unit: "bytes" }));
    expect(screen.getByRole("progressbar", { name: labels.downloading })).toHaveAttribute(
      "aria-valuenow",
      "5",
    );
  });

  it("keeps the install running when the player leaves and comes back", async () => {
    client.checkAppUpdate.mockResolvedValue(available);
    client.installAppUpdate.mockImplementation(() => running(new Promise(() => {})));
    const first = renderWithQuery(<UpdatePage />);
    fireEvent.click(await installButton());
    first.unmount();

    renderWithQuery(<UpdatePage />);
    expect(await screen.findByText(labels.installing)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: new RegExp(labels.install) })).toBeNull();
    expect(client.installAppUpdate).toHaveBeenCalledTimes(1);
  });

  it("says when the check for updates failed", async () => {
    client.checkAppUpdate.mockRejectedValue(new IpcError("network", "offline"));
    renderWithQuery(<UpdatePage />);
    expect(await screen.findByText(esCommon.errors.network)).toBeInTheDocument();
  });

  it("explains a refused install and checks again when the release changed", async () => {
    client.checkAppUpdate.mockResolvedValue(available);
    client.installAppUpdate
      .mockImplementationOnce(() =>
        running(Promise.reject(new IpcError("already_running", "a game is running"))),
      )
      .mockImplementationOnce(() =>
        running(Promise.reject(new IpcError("update_changed", "moved on"))),
      );
    renderWithQuery(<UpdatePage />);

    fireEvent.click(await installButton());
    expect(await screen.findByRole("alert")).toHaveTextContent(labels.closeTheGame);

    fireEvent.click(await installButton());
    await waitFor(() => expect(client.checkAppUpdate).toHaveBeenCalledTimes(2));
  });
});
