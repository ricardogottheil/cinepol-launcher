import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type ProgressEvent } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { UpdateNotice } from "./UpdateNotice";

const client = vi.hoisted(() => ({
  checkAppUpdate: vi.fn(),
  installAppUpdate: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.updates;
const available = {
  kind: "available",
  version: "0.2.0",
  currentVersion: "0.1.0",
  notes: "Faster installs",
} as const;

function running(done: Promise<void>) {
  return { operationId: "op", done, cancel: vi.fn() };
}

describe("UpdateNotice", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows nothing when the launcher is up to date or cannot update", async () => {
    client.checkAppUpdate.mockResolvedValueOnce({ kind: "upToDate" });
    const first = renderWithQuery(<UpdateNotice />);
    await waitFor(() => expect(client.checkAppUpdate).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    first.unmount();

    client.checkAppUpdate.mockResolvedValueOnce({ kind: "notConfigured" });
    renderWithQuery(<UpdateNotice />);
    await waitFor(() => expect(client.checkAppUpdate).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("offers the new version with its notes and installs exactly that one", async () => {
    client.checkAppUpdate.mockResolvedValue(available);
    let report: (event: ProgressEvent) => void = () => {};
    client.installAppUpdate.mockImplementation(
      (_version: string, onProgress: (event: ProgressEvent) => void) => {
        report = onProgress;
        return running(new Promise(() => {}));
      },
    );
    renderWithQuery(<UpdateNotice />);

    expect(
      await screen.findByText(labels.available.replace("{{version}}", "0.2.0")),
    ).toBeInTheDocument();
    expect(screen.getByText("Faster installs")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.install }));

    expect(client.installAppUpdate).toHaveBeenCalledWith("0.2.0", expect.any(Function));
    expect(screen.getByRole("button", { name: labels.install })).toBeDisabled();
    act(() => report({ done: 5, total: 10, stage: null, unit: "bytes" }));
    expect(screen.getByRole("progressbar", { name: labels.downloading })).toHaveAttribute(
      "aria-valuenow",
      "5",
    );
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
    renderWithQuery(<UpdateNotice />);
    const install = await screen.findByRole("button", { name: labels.install });

    fireEvent.click(install);
    expect(await screen.findByText(labels.closeTheGame)).toBeInTheDocument();
    expect(install).toBeEnabled();

    fireEvent.click(install);
    await waitFor(() => expect(client.checkAppUpdate).toHaveBeenCalledTimes(2));
  });
});
