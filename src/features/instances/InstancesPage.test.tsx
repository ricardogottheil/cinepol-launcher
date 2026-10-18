import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type LogLineDto, type ProgressEvent } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { CINEPOL_SESSION, useActivityStore } from "./activityStore";
import { InstancesPage } from "./InstancesPage";

const client = vi.hoisted(() => ({
  listInstances: vi.fn(),
  listGameVersions: vi.fn(),
  createInstance: vi.fn(),
  installInstance: vi.fn(),
  launchInstance: vi.fn(),
  killInstance: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const instance = {
  id: "survival",
  name: "Survival",
  gameVersion: "26.2",
  loader: "vanilla",
  loaderVersion: null,
  origin: "custom",
  playtimeSecs: 3_900,
} as const;
const labels = esCommon.instances;
const outcome = { instanceId: "survival", exitCode: 0, killed: false, playedSecs: 60 };

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  useActivityStore.setState({ byInstance: {} });
  client.listInstances.mockResolvedValue([instance]);
  client.listGameVersions.mockResolvedValue({
    latestRelease: "26.2",
    versions: [
      { id: "26.2", kind: "release", releaseTime: "" },
      { id: "1.21.11", kind: "release", releaseTime: "" },
    ],
  });
});

describe("InstancesPage", () => {
  it("lists instances with their spec readouts", async () => {
    renderWithQuery(<InstancesPage />);
    const row = (await screen.findByRole("heading", { name: "Survival" })).closest("li");
    if (!row) throw new Error("row not found");
    expect(within(row).getByText("26.2")).toBeInTheDocument();
    expect(within(row).getByText("1 h 5 min")).toBeInTheDocument();
  });

  it("creates an instance on the latest release by default", async () => {
    client.createInstance.mockResolvedValue({ ...instance, id: "cinepol", name: "CINEPOL" });
    renderWithQuery(<InstancesPage />);
    await screen.findByRole("option", { name: "1.21.11" });
    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "CINEPOL" } });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() => expect(client.createInstance).toHaveBeenCalledWith("CINEPOL", "26.2"));
  });

  it("shows install progress by stage and can cancel", async () => {
    const done = deferred<unknown>();
    const cancel = vi.fn().mockResolvedValue(true);
    let onProgress: (event: ProgressEvent) => void = () => {};
    client.installInstance.mockImplementation(
      (_id: string, handler: (event: ProgressEvent) => void) => {
        onProgress = handler;
        return { operationId: "op", done: done.promise, cancel };
      },
    );
    renderWithQuery(<InstancesPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.install }));
    act(() => onProgress({ done: 40, total: 100, stage: "assets", unit: "bytes" }));

    expect(screen.getByText(esCommon.stages.assets)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "40");
    fireEvent.click(screen.getByRole("button", { name: labels.cancel }));
    expect(cancel).toHaveBeenCalledOnce();
    await act(async () => done.reject(new IpcError("cancelled", "cancelled")));
    expect(await screen.findByText(esCommon.errors.cancelled)).toBeInTheDocument();
  });

  it("streams the game log while playing and offers stop", async () => {
    const exit = deferred<typeof outcome>();
    let onLog: (line: LogLineDto) => void = () => {};
    client.launchInstance.mockImplementation((_id: string, handler: (line: LogLineDto) => void) => {
      onLog = handler;
      return exit.promise;
    });
    client.killInstance.mockResolvedValue(true);
    renderWithQuery(<InstancesPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    act(() => onLog({ stream: "stdout", line: "[Render thread/INFO]: Setting user: Steve" }));

    expect(screen.getByRole("log")).toHaveTextContent("Setting user: Steve");
    fireEvent.click(screen.getByRole("button", { name: labels.stop }));
    expect(client.killInstance).toHaveBeenCalledWith("survival");
    await act(async () => exit.resolve(outcome));
    expect(await screen.findByRole("button", { name: labels.play })).toBeEnabled();
  });

  it("installs first when the game reports it is not installed, then plays", async () => {
    client.launchInstance
      .mockRejectedValueOnce(new IpcError("not_installed", "missing 26.2.jar"))
      .mockResolvedValueOnce(outcome);
    client.installInstance.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        downloadedFiles: 1,
        reusedFiles: 0,
        downloadedBytes: 10,
        javaMajor: 25,
      }),
      cancel: vi.fn(),
    });
    renderWithQuery(<InstancesPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    await waitFor(() => expect(client.launchInstance).toHaveBeenCalledTimes(2));
    expect(client.installInstance).toHaveBeenCalledOnce();
    expect(screen.queryByText(esCommon.errors.not_installed)).not.toBeInTheDocument();
  });

  it("shows a Play on CINEPOL session on the CINEPOL row and stops it from there", async () => {
    client.listInstances.mockResolvedValue([
      instance,
      { ...instance, id: "cinepol", name: "CINEPOL", origin: "cinepolServer" },
    ]);
    client.killInstance.mockResolvedValue(true);
    useActivityStore.setState({
      byInstance: {
        [CINEPOL_SESSION]: { activity: { kind: "running" }, logs: [], error: null, lastExit: null },
      },
    });
    renderWithQuery(<InstancesPage />);

    const row = (await screen.findByRole("heading", { name: "CINEPOL" })).closest("li");
    if (!row) throw new Error("no CINEPOL row");
    expect(within(row).queryByRole("button", { name: labels.play })).not.toBeInTheDocument();
    fireEvent.click(within(row).getByRole("button", { name: labels.stop }));
    await waitFor(() => expect(client.killInstance).toHaveBeenCalledWith("cinepol"));

    const other = screen.getByRole("heading", { name: "Survival" }).closest("li");
    if (!other) throw new Error("no Survival row");
    expect(within(other).getByRole("button", { name: labels.play })).toBeEnabled();
  });
});
