import { createMemoryHistory } from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../../app/App";
import { createAppRouter } from "../../app/router";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type LogLineDto, type ProgressEvent } from "../../ipc/client";
import { createTestQueryClient } from "../../test/render";
import { CINEPOL_SESSION, useActivityStore } from "./activityStore";

const client = vi.hoisted(() => ({
  listInstances: vi.fn(),
  listGameVersions: vi.fn(),
  createInstance: vi.fn(),
  listLoaderVersions: vi.fn(),
  installInstance: vi.fn(),
  launchInstance: vi.fn(),
  killInstance: vi.fn(),
  openExternal: vi.fn(),
  updateModpack: vi.fn(),
  exportMrpack: vi.fn(),
  getInstanceSettings: vi.fn(),
  updateInstanceSettings: vi.fn(),
  pickJavaExecutable: vi.fn(),
  duplicateInstance: vi.fn(),
  deleteInstance: vi.fn(),
  listAccounts: vi.fn(),
  updatePackwiz: vi.fn(),
  checkManualDownloads: vi.fn(),
  listServers: vi.fn(),
  editServerList: vi.fn(),
  openCrashReport: vi.fn(),
  openInstanceFolder: vi.fn(),
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
const settingsLabels = esCommon.instances.settings;
const stored = {
  name: "Survival",
  defaultAccountId: null,
  java: { kind: "managed", path: null },
  minMemoryMb: 512,
  maxMemoryMb: 4096,
  jvmArgs: [],
  window: null,
  env: {},
  updatePackOnLaunch: false,
} as const;
const outcome = {
  instanceId: "survival",
  exitCode: 0,
  killed: false,
  playedSecs: 60,
  crashReport: null as string | null,
};

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (error: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(<App router={router} queryClient={createTestQueryClient()} />);
  return router;
}

beforeEach(() => {
  vi.clearAllMocks();
  useActivityStore.setState({ byInstance: {} });
  client.listInstances.mockResolvedValue([instance]);
  client.getInstanceSettings.mockResolvedValue(stored);
  client.listAccounts.mockResolvedValue({
    accounts: [{ id: "a1", kind: "offline", username: "Steve", profileUuid: "u1" }],
    activeId: "a1",
  });
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
    renderAt("/instances");
    const row = (await screen.findByRole("heading", { name: "Survival" })).closest("li");
    if (!row) throw new Error("row not found");
    expect(within(row).getByText("26.2")).toBeInTheDocument();
    expect(within(row).getByText("1 h 5 min")).toBeInTheDocument();
  });

  it("opens an instance on its own page with servers, mods, settings and log tabs", async () => {
    client.listServers.mockResolvedValue({ servers: [] });
    renderAt("/instances");
    const row = (await screen.findByRole("heading", { name: "Survival" })).closest("li");
    if (!row) throw new Error("row not found");
    expect(within(row).getByRole("button", { name: labels.play })).toBeEnabled();
    expect(within(row).queryByRole("button", { name: labels.export })).not.toBeInTheDocument();
    fireEvent.click(within(row).getByRole("link", { name: "Survival" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Survival" })).toBeInTheDocument();
    expect(screen.getByText("1 h 5 min")).toBeInTheDocument();
    const servers = screen.getByRole("tab", { name: labels.tabs.servers });
    expect(servers).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("tabpanel", { name: labels.tabs.servers })).toHaveTextContent(
      labels.servers.empty,
    );

    fireEvent.keyDown(servers, { key: "ArrowRight" });
    const mods = screen.getByRole("tab", { name: labels.tabs.mods });
    expect(mods).toHaveFocus();
    fireEvent.keyDown(mods, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: labels.tabs.settings })).toHaveFocus();
    expect(await screen.findByRole("region", { name: labels.settings.title })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: labels.tabs.log }));
    expect(screen.getByRole("tabpanel", { name: labels.tabs.log })).toHaveTextContent(
      labels.logEmpty,
    );

    fireEvent.click(screen.getByRole("link", { name: labels.back }));
    expect(
      await screen.findByRole("heading", { level: 1, name: labels.title }),
    ).toBeInTheDocument();
  });

  it("opens the instance's folder and says why it could not", async () => {
    client.openInstanceFolder.mockResolvedValueOnce(undefined);
    client.openInstanceFolder.mockRejectedValueOnce(new IpcError("internal", "no file manager"));
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.openFolder }));
    await waitFor(() => expect(client.openInstanceFolder).toHaveBeenCalledWith("survival"));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: labels.openFolder }));
    expect(await screen.findByRole("alert")).toHaveTextContent(esCommon.errors.internal);
  });

  it("opens the folder once while the file manager is starting", async () => {
    const opened = deferred<undefined>();
    client.openInstanceFolder.mockReturnValue(opened.promise);
    renderAt("/instances/survival");
    const button = await screen.findByRole("button", { name: labels.openFolder });
    fireEvent.click(button);
    await waitFor(() => expect(button).toBeDisabled());
    fireEvent.click(button);
    expect(client.openInstanceFolder).toHaveBeenCalledOnce();
    await act(async () => opened.resolve(undefined));
    await waitFor(() => expect(button).toBeEnabled());
  });

  it("starts another instance's page fresh", async () => {
    client.listInstances.mockResolvedValue([instance, { ...instance, id: "fo", name: "FO" }]);
    client.listServers.mockResolvedValue({ servers: [] });
    client.openInstanceFolder.mockRejectedValue(new IpcError("internal", "no file manager"));
    const router = renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("tab", { name: labels.tabs.settings }));
    fireEvent.click(screen.getByRole("button", { name: labels.openFolder }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();

    await act(() =>
      router.navigate({ to: "/instances/$instanceId", params: { instanceId: "fo" } }),
    );
    expect(await screen.findByRole("heading", { level: 1, name: "FO" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: labels.tabs.servers })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("says when the instance of a page does not exist", async () => {
    renderAt("/instances/missing");
    expect(await screen.findByText(labels.notFound)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: labels.back })).toBeInTheDocument();
  });

  it("creates an instance on the latest release by default", async () => {
    client.createInstance.mockResolvedValue({ ...instance, id: "cinepol", name: "CINEPOL" });
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "CINEPOL" } });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() =>
      expect(client.createInstance).toHaveBeenCalledWith({
        name: "CINEPOL",
        gameVersion: "26.2",
        loader: "vanilla",
        loaderVersion: null,
      }),
    );
    expect(client.listLoaderVersions).not.toHaveBeenCalled();
  });

  it("creates a Fabric instance on the newest stable loader by default", async () => {
    client.listLoaderVersions.mockResolvedValue([
      { version: "0.20.0", stable: false },
      { version: "0.19.5", stable: true },
      { version: "0.19.4", stable: true },
    ]);
    client.createInstance.mockResolvedValue({ ...instance, id: "fab", name: "Fab" });
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "fabric" } });
    await waitFor(() => expect(client.listLoaderVersions).toHaveBeenCalledWith("fabric", "26.2"));
    expect(await screen.findByLabelText(labels.loaderVersionLabel)).toHaveValue("0.19.5");

    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "Fab" } });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() =>
      expect(client.createInstance).toHaveBeenCalledWith({
        name: "Fab",
        gameVersion: "26.2",
        loader: "fabric",
        loaderVersion: "0.19.5",
      }),
    );
  });

  it("forgets the picked loader version when the snapshot toggle resets the game version", async () => {
    client.listLoaderVersions.mockImplementation(async (_loader: string, game: string) =>
      game === "1.21.11"
        ? [
            { version: "0.18.0", stable: true },
            { version: "0.17.9", stable: true },
          ]
        : [{ version: "0.19.5", stable: true }],
    );
    client.createInstance.mockResolvedValue({ ...instance, id: "fab", name: "Fab" });
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    fireEvent.change(screen.getByLabelText(labels.versionLabel), { target: { value: "1.21.11" } });
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "fabric" } });
    await screen.findByRole("option", { name: "0.17.9" });
    fireEvent.change(screen.getByLabelText(labels.loaderVersionLabel), {
      target: { value: "0.17.9" },
    });

    // Back to the latest release (26.2): the 1.21.11 loader pick must not stay.
    fireEvent.click(screen.getByLabelText(labels.showSnapshots));
    await screen.findByRole("option", { name: "0.19.5" });
    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "Fab" } });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() =>
      expect(client.createInstance).toHaveBeenCalledWith(
        expect.objectContaining({ gameVersion: "26.2", loaderVersion: "0.19.5" }),
      ),
    );
  });

  it("offers Forge and NeoForge with their recommended release by default", async () => {
    client.listLoaderVersions.mockResolvedValue([
      { version: "65.1.5", stable: false },
      { version: "65.1.0", stable: true },
    ]);
    client.createInstance.mockResolvedValue({ ...instance, id: "forge", name: "Forge" });
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    expect(screen.getByRole("option", { name: labels.loaders.neoforge })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "forge" } });
    await waitFor(() => expect(client.listLoaderVersions).toHaveBeenCalledWith("forge", "26.2"));
    expect(await screen.findByLabelText(labels.loaderVersionLabel)).toHaveValue("65.1.0");
    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "Forge" } });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() =>
      expect(client.createInstance).toHaveBeenCalledWith(
        expect.objectContaining({ loader: "forge", loaderVersion: "65.1.0" }),
      ),
    );
  });

  it("asks to support Forge, since its installer is downloaded automatically", async () => {
    client.listLoaderVersions.mockResolvedValue([{ version: "65.1.0", stable: true }]);
    client.openExternal.mockResolvedValue(undefined);
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    expect(screen.queryByRole("link", { name: labels.supportForge })).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "forge" } });
    expect(screen.getByText(labels.forgeNote)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: labels.supportForge }));
    expect(client.openExternal).toHaveBeenCalledWith("https://www.patreon.com/LexManos/");

    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "neoforge" } });
    expect(screen.queryByRole("link", { name: labels.supportForge })).not.toBeInTheDocument();
  });

  it("explains a loader that does not support the chosen version", async () => {
    client.listLoaderVersions.mockRejectedValue(new IpcError("loader_unavailable", "400"));
    renderAt("/instances");
    await screen.findByRole("option", { name: "1.21.11" });
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "quilt" } });
    fireEvent.change(screen.getByLabelText(labels.nameLabel), { target: { value: "Q" } });
    expect(await screen.findByText(esCommon.errors.loader_unavailable)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.create })).toBeDisabled();
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
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.install }));
    act(() => onProgress({ done: 43_515_908, total: 126_583_468, stage: "assets", unit: "bytes" }));

    expect(screen.getByText(esCommon.stages.assets)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "43515908");
    expect(screen.getByText("41.5 / 120.7 MB")).toBeInTheDocument();
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
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    act(() =>
      onLog({
        stream: "stdout",
        line: "[Render thread/INFO]: Setting user: Steve",
        level: null,
        thread: null,
        logger: null,
      }),
    );

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
    renderAt("/instances/survival");
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
    renderAt("/instances");

    const row = (await screen.findByRole("heading", { name: "CINEPOL" })).closest("li");
    if (!row) throw new Error("no CINEPOL row");
    expect(within(row).queryByRole("button", { name: labels.play })).not.toBeInTheDocument();
    fireEvent.click(within(row).getByRole("button", { name: labels.stop }));
    await waitFor(() => expect(client.killInstance).toHaveBeenCalledWith("cinepol"));

    const other = screen.getByRole("heading", { name: "Survival" }).closest("li");
    if (!other) throw new Error("no Survival row");
    expect(within(other).getByRole("button", { name: labels.play })).toBeEnabled();
  });

  it("offers modpack updates only on modpack instances and reports kept changes", async () => {
    client.listInstances.mockResolvedValue([
      instance,
      {
        ...instance,
        id: "fo",
        name: "Fabulously Optimized",
        origin: "modpack",
        modpackSource: "modrinth",
      },
    ]);
    client.updateModpack.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        upToDate: false,
        instance: {
          ...instance,
          id: "fo",
          name: "Fabulously Optimized",
          origin: "modpack",
          modpackSource: "modrinth",
        },
        files: { written: 3, unchanged: 40, kept: 2, removed: 1 },
      }),
      cancel: vi.fn(),
    });
    const router = renderAt("/instances/survival");
    await screen.findByRole("heading", { level: 1, name: "Survival" });
    expect(screen.getByRole("button", { name: labels.export })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.update })).not.toBeInTheDocument();

    await act(() =>
      router.navigate({ to: "/instances/$instanceId", params: { instanceId: "fo" } }),
    );
    fireEvent.click(await screen.findByRole("button", { name: labels.update }));
    expect(client.updateModpack).toHaveBeenCalledWith("fo", expect.any(Function));
    expect(
      await screen.findByText(labels.keptChanges_other.replace("{{count}}", "2")),
    ).toBeInTheDocument();
  });

  it("says when a modpack is already up to date", async () => {
    client.listInstances.mockResolvedValue([
      { ...instance, origin: "modpack", modpackSource: "modrinth" },
    ]);
    client.updateModpack.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ upToDate: true, instance: null, files: null }),
      cancel: vi.fn(),
    });
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.update }));
    expect(await screen.findByText(labels.upToDate)).toBeInTheDocument();
  });

  it("exports an instance as a .mrpack and shows where it was saved", async () => {
    client.exportMrpack.mockResolvedValue({
      path: "/home/steve/Survival.mrpack",
      indexed: 10,
      overrides: 2,
    });
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.export }));
    expect(client.exportMrpack).toHaveBeenCalledWith("survival");
    expect(
      await screen.findByText(labels.exported.replace("{{path}}", "/home/steve/Survival.mrpack")),
    ).toBeInTheDocument();
  });

  it("shows why an export failed", async () => {
    client.exportMrpack.mockRejectedValue(new IpcError("network", "offline"));
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.export }));
    expect(await screen.findByText(esCommon.errors.network)).toBeInTheDocument();
  });

  it("keeps the row busy while an export runs", async () => {
    const exported = deferred<{ path: string; indexed: number; overrides: number } | null>();
    client.exportMrpack.mockReturnValue(exported.promise);
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.export }));
    expect(await screen.findByText(labels.exporting)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.export })).toBeDisabled();
    expect(screen.getByRole("button", { name: labels.play })).toBeDisabled();
    await act(async () => exported.resolve(null));
    expect(screen.getByRole("button", { name: labels.play })).toBeEnabled();
  });

  it("shows why an update failed and frees the row", async () => {
    client.listInstances.mockResolvedValue([
      { ...instance, origin: "modpack", modpackSource: "modrinth" },
    ]);
    client.updateModpack.mockImplementation(() => ({
      operationId: "op",
      done: Promise.reject(new IpcError("network", "offline")),
      cancel: vi.fn(),
    }));
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.update }));
    expect(await screen.findByText(esCommon.errors.network)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.update })).toBeEnabled();
  });

  async function openSettings() {
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("tab", { name: labels.tabs.settings }));
    return screen.findByRole("region", { name: settingsLabels.title });
  }

  it("edits and saves an instance's settings", async () => {
    client.updateInstanceSettings.mockImplementation((_id: string, settings: unknown) =>
      Promise.resolve(settings),
    );
    const panel = await openSettings();
    const name = await within(panel).findByLabelText(labels.nameLabel);
    expect(name).toHaveValue("Survival");
    fireEvent.change(name, { target: { value: "Survival 2" } });
    fireEvent.change(within(panel).getByLabelText(settingsLabels.maxMemory), {
      target: { value: "6144" },
    });
    fireEvent.change(within(panel).getByLabelText(settingsLabels.account), {
      target: { value: "a1" },
    });
    fireEvent.change(within(panel).getByLabelText(settingsLabels.env), {
      target: { value: "MESA_GL_VERSION_OVERRIDE=4.6" },
    });
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.save }));
    await waitFor(() =>
      expect(client.updateInstanceSettings).toHaveBeenCalledWith("survival", {
        ...stored,
        name: "Survival 2",
        maxMemoryMb: 6144,
        defaultAccountId: "a1",
        env: { MESA_GL_VERSION_OVERRIDE: "4.6" },
      }),
    );
    expect(await within(panel).findByText(settingsLabels.saved)).toBeInTheDocument();
  });

  it("does not save a malformed environment line", async () => {
    const panel = await openSettings();
    fireEvent.change(await within(panel).findByLabelText(settingsLabels.env), {
      target: { value: "A=1\nnot a pair" },
    });
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.save }));
    expect(
      await within(panel).findByText(settingsLabels.envInvalid.replace("{{line}}", "2")),
    ).toBeInTheDocument();
    expect(client.updateInstanceSettings).not.toHaveBeenCalled();
  });

  it("uses a Java executable chosen with the picker", async () => {
    client.pickJavaExecutable.mockResolvedValue("/opt/jdk-25/bin/java");
    client.updateInstanceSettings.mockImplementation((_id: string, settings: unknown) =>
      Promise.resolve(settings),
    );
    const panel = await openSettings();
    fireEvent.change(await within(panel).findByLabelText(settingsLabels.java), {
      target: { value: "custom" },
    });
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.pickJava }));
    expect(await within(panel).findByText("/opt/jdk-25/bin/java")).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.save }));
    await waitFor(() =>
      expect(client.updateInstanceSettings).toHaveBeenCalledWith(
        "survival",
        expect.objectContaining({ java: { kind: "custom", path: "/opt/jdk-25/bin/java" } }),
      ),
    );
  });

  it("shows why the backend refused the settings", async () => {
    client.updateInstanceSettings.mockRejectedValue(new IpcError("invalid_input", "memory"));
    const panel = await openSettings();
    fireEvent.click(await within(panel).findByRole("button", { name: settingsLabels.save }));
    expect(await within(panel).findByText(esCommon.errors.invalid_input)).toBeInTheDocument();
  });

  it("duplicates an instance under a localized copy name", async () => {
    client.duplicateInstance.mockResolvedValue({
      ...instance,
      id: "survival-copia",
      name: "Survival (copia)",
    });
    const panel = await openSettings();
    fireEvent.click(await within(panel).findByRole("button", { name: settingsLabels.duplicate }));
    await waitFor(() =>
      expect(client.duplicateInstance).toHaveBeenCalledWith("survival", "Survival (copia)"),
    );
    expect(
      await within(panel).findByText(
        settingsLabels.duplicated.replace("{{name}}", "Survival (copia)"),
      ),
    ).toBeInTheDocument();
  });

  it("deletes an instance only after the player confirms", async () => {
    client.deleteInstance.mockResolvedValue(undefined);
    const panel = await openSettings();
    fireEvent.click(await within(panel).findByRole("button", { name: settingsLabels.delete }));
    const dialog = screen.getByRole("alertdialog", {
      name: settingsLabels.deleteTitle.replace("{{name}}", "Survival"),
    });
    expect(within(dialog).getByText(settingsLabels.deleteBody)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: labels.cancel })).toHaveFocus();

    fireEvent.click(within(dialog).getByRole("button", { name: labels.cancel }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(client.deleteInstance).not.toHaveBeenCalled();

    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.delete }));
    fireEvent.keyDown(screen.getByRole("alertdialog"), { key: "Escape" });
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    client.listInstances.mockResolvedValue([]);
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.delete }));
    fireEvent.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: settingsLabels.deleteConfirm,
      }),
    );
    await waitFor(() => expect(client.deleteInstance).toHaveBeenCalledWith("survival"));
    expect(await screen.findByText(labels.empty)).toBeInTheDocument();
  });

  it("explains why a running instance cannot be deleted", async () => {
    client.deleteInstance.mockRejectedValue(new IpcError("already_running", "busy"));
    const panel = await openSettings();
    fireEvent.click(await within(panel).findByRole("button", { name: settingsLabels.delete }));
    fireEvent.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: settingsLabels.deleteConfirm,
      }),
    );
    expect(await within(panel).findByText(esCommon.errors.already_running)).toBeInTheDocument();
  });

  const packwizInstance = {
    ...instance,
    id: "gto",
    name: "GregTech Odyssey",
    origin: "modpack",
    modpackSource: "packwiz",
    updatePackOnLaunch: false,
  } as const;
  const jei = {
    name: "Just Enough Items (JEI)",
    filename: "jei-1.20.1-forge-15.20.0.133.jar",
    pageUrl: "https://www.curseforge.com/projects/238222",
  };
  const manual = esCommon.instances.manual;

  it("updates packwiz instances from their pack", async () => {
    client.listInstances.mockResolvedValue([packwizInstance]);
    client.checkManualDownloads.mockResolvedValue({ resolved: [], remaining: [] });
    client.updatePackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        instance: packwizInstance,
        files: { written: 1, unchanged: 3, kept: 0, removed: 0 },
        pending: [],
      }),
      cancel: vi.fn(),
    });
    renderAt("/instances/gto");
    fireEvent.click(await screen.findByRole("button", { name: labels.update }));
    expect(client.updatePackwiz).toHaveBeenCalledWith("gto", expect.any(Function));
    expect(client.updateModpack).not.toHaveBeenCalled();
    expect(await screen.findByText(labels.updated)).toBeInTheDocument();
  });

  it("updates CurseForge instances and lists their manual downloads", async () => {
    const curseforgeInstance = {
      ...packwizInstance,
      id: "fo",
      modpackSource: "curseforge",
    } as const;
    client.listInstances.mockResolvedValue([curseforgeInstance]);
    client.checkManualDownloads.mockResolvedValue({ resolved: [], remaining: [jei] });
    client.updateModpack.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ upToDate: true, instance: null, files: null, pending: [] }),
      cancel: vi.fn(),
    });
    renderAt("/instances/fo");
    const panel = await screen.findByRole("region", { name: manual.title });
    expect(within(panel).getByText(jei.name)).toBeInTheDocument();
    expect(client.checkManualDownloads).toHaveBeenCalledWith("fo");
    fireEvent.click(screen.getByRole("button", { name: labels.update }));
    expect(client.updateModpack).toHaveBeenCalledWith("fo", expect.any(Function));
    expect(client.updatePackwiz).not.toHaveBeenCalled();
  });

  it("lists manual downloads, links their pages and finishes when they arrive", async () => {
    client.listInstances.mockResolvedValue([packwizInstance]);
    client.checkManualDownloads
      .mockResolvedValueOnce({ resolved: [], remaining: [jei] })
      .mockResolvedValue({ resolved: [jei], remaining: [] });
    renderAt("/instances/gto");
    const panel = await screen.findByRole("region", { name: manual.title });
    expect(within(panel).getByText(jei.name)).toBeInTheDocument();
    expect(within(panel).getByText(jei.filename)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.play })).toBeDisabled();
    fireEvent.click(within(panel).getByRole("button", { name: manual.open }));
    expect(client.openExternal).toHaveBeenCalledWith(jei.pageUrl);

    fireEvent.click(within(panel).getByRole("button", { name: manual.check }));
    expect(await screen.findByText(manual.done)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.play })).toBeEnabled();
  });

  it("syncs the pack before playing when the instance asks for it", async () => {
    client.listInstances.mockResolvedValue([{ ...packwizInstance, updatePackOnLaunch: true }]);
    client.checkManualDownloads.mockResolvedValue({ resolved: [], remaining: [] });
    client.updatePackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        instance: packwizInstance,
        files: { written: 0, unchanged: 4, kept: 0, removed: 0 },
        pending: [],
      }),
      cancel: vi.fn(),
    });
    client.launchInstance.mockResolvedValue({ ...outcome, instanceId: "gto" });
    renderAt("/instances/gto");
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    await waitFor(() => expect(client.launchInstance).toHaveBeenCalled());
    expect(client.updatePackwiz).toHaveBeenCalledWith("gto", expect.any(Function));
    expect(client.updatePackwiz.mock.invocationCallOrder[0]).toBeLessThan(
      client.launchInstance.mock.invocationCallOrder[0] ?? 0,
    );
  });

  it("offers the update-on-launch option only for packwiz instances", async () => {
    client.listInstances.mockResolvedValue([packwizInstance]);
    client.checkManualDownloads.mockResolvedValue({ resolved: [], remaining: [] });
    client.updateInstanceSettings.mockImplementation((_id: string, settings: unknown) =>
      Promise.resolve(settings),
    );
    renderAt("/instances/gto");
    fireEvent.click(await screen.findByRole("tab", { name: labels.tabs.settings }));
    const panel = await screen.findByRole("region", { name: settingsLabels.title });
    fireEvent.click(await within(panel).findByLabelText(settingsLabels.updateOnLaunch));
    fireEvent.click(within(panel).getByRole("button", { name: settingsLabels.save }));
    await waitFor(() =>
      expect(client.updateInstanceSettings).toHaveBeenCalledWith(
        "gto",
        expect.objectContaining({ updatePackOnLaunch: true }),
      ),
    );
  });

  it("does not show the update-on-launch option for other instances", async () => {
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("tab", { name: labels.tabs.settings }));
    const panel = await screen.findByRole("region", { name: settingsLabels.title });
    await within(panel).findByLabelText(labels.nameLabel);
    expect(within(panel).queryByLabelText(settingsLabels.updateOnLaunch)).not.toBeInTheDocument();
  });

  it("does not launch when the on-launch sync leaves files to download by hand", async () => {
    client.listInstances.mockResolvedValue([{ ...packwizInstance, updatePackOnLaunch: true }]);
    client.checkManualDownloads
      .mockResolvedValueOnce({ resolved: [], remaining: [] })
      .mockResolvedValue({ resolved: [], remaining: [jei] });
    client.updatePackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        instance: packwizInstance,
        files: { written: 1, unchanged: 3, kept: 0, removed: 0 },
        pending: [jei],
      }),
      cancel: vi.fn(),
    });
    client.launchInstance.mockResolvedValue({ ...outcome, instanceId: "gto" });
    renderAt("/instances/gto");
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    await waitFor(() => expect(client.updatePackwiz).toHaveBeenCalled());
    expect(await screen.findByRole("region", { name: manual.title })).toBeInTheDocument();
    expect(client.launchInstance).not.toHaveBeenCalled();
  });

  it("a manual Update that adds pending files shows the list and disables Play", async () => {
    client.listInstances.mockResolvedValue([packwizInstance]);
    client.checkManualDownloads
      .mockResolvedValueOnce({ resolved: [], remaining: [] })
      .mockResolvedValue({ resolved: [], remaining: [jei] });
    client.updatePackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({
        instance: packwizInstance,
        files: { written: 1, unchanged: 3, kept: 0, removed: 0 },
        pending: [jei],
      }),
      cancel: vi.fn(),
    });
    renderAt("/instances/gto");
    fireEvent.click(await screen.findByRole("button", { name: labels.update }));
    expect(await screen.findByRole("region", { name: manual.title })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.play })).toBeDisabled();
  });

  describe("server list", () => {
    const serverLabels = esCommon.instances.servers;
    const cinepol = {
      name: "CINEPOL",
      address: "mc.policarpiers.com",
      acceptTextures: null,
      icon: null,
    };
    const hypixel = { ...cinepol, name: "Hypixel", address: "mc.hypixel.net" };

    async function openServers() {
      renderAt("/instances/survival");
      return screen.findByRole("region", { name: serverLabels.title });
    }

    it("lists the instance's servers", async () => {
      client.listServers.mockResolvedValue({ servers: [cinepol, hypixel] });
      const panel = await openServers();
      expect(await within(panel).findByText("CINEPOL")).toBeInTheDocument();
      expect(within(panel).getByText("mc.hypixel.net")).toBeInTheDocument();
      expect(client.listServers).toHaveBeenCalledWith("survival");
    });

    it("adds a server", async () => {
      client.listServers.mockResolvedValue({ servers: [cinepol] });
      client.editServerList.mockResolvedValue({ servers: [cinepol, hypixel] });
      const panel = await openServers();
      await within(panel).findByText("CINEPOL");
      fireEvent.change(within(panel).getByLabelText(serverLabels.name), {
        target: { value: "Hypixel" },
      });
      fireEvent.change(within(panel).getByLabelText(serverLabels.address), {
        target: { value: "mc.hypixel.net" },
      });
      fireEvent.change(within(panel).getByLabelText(serverLabels.textures), {
        target: { value: "on" },
      });
      fireEvent.click(within(panel).getByRole("button", { name: serverLabels.add }));
      await waitFor(() =>
        expect(client.editServerList).toHaveBeenCalledWith("survival", {
          kind: "add",
          form: { name: "Hypixel", address: "mc.hypixel.net", acceptTextures: true },
        }),
      );
      expect(await within(panel).findByText("mc.hypixel.net")).toBeInTheDocument();
    });

    it("edits, reorders and removes servers", async () => {
      client.listServers.mockResolvedValue({ servers: [cinepol, hypixel] });
      client.editServerList.mockResolvedValue({ servers: [cinepol, hypixel] });
      const panel = await openServers();
      const row = (await within(panel).findByText("Hypixel")).closest("li");
      if (!row) throw new Error("no server row");

      fireEvent.click(within(row).getByRole("button", { name: serverLabels.moveUp }));
      await waitFor(() =>
        expect(client.editServerList).toHaveBeenCalledWith("survival", {
          kind: "move",
          from: 1,
          to: 0,
        }),
      );
      fireEvent.click(within(row).getByRole("button", { name: serverLabels.remove }));
      await waitFor(() =>
        expect(client.editServerList).toHaveBeenCalledWith("survival", {
          kind: "remove",
          index: 1,
        }),
      );

      fireEvent.click(within(row).getByRole("button", { name: serverLabels.edit }));
      const name = within(panel).getByLabelText(serverLabels.name);
      expect(name).toHaveValue("Hypixel");
      fireEvent.change(name, { target: { value: "Hypixel EU" } });
      fireEvent.click(within(panel).getByRole("button", { name: serverLabels.save }));
      await waitFor(() =>
        expect(client.editServerList).toHaveBeenCalledWith("survival", {
          kind: "update",
          index: 1,
          form: { name: "Hypixel EU", address: "mc.hypixel.net", acceptTextures: null },
        }),
      );
    });

    it("locks reordering and removal while a server is being edited", async () => {
      const s = (n: string) => ({ ...cinepol, name: n, address: `${n.toLowerCase()}.example.com` });
      const [a, b, c] = ["A", "B", "C"].map(s);
      client.listServers.mockResolvedValue({ servers: [a, b, c] });
      client.editServerList.mockResolvedValue({ servers: [a, b, c] });
      const panel = await openServers();
      const rowC = (await within(panel).findByText("C")).closest("li");
      const rowA = within(panel).getByText("A").closest("li");
      if (!rowC || !rowA) throw new Error("no rows");
      fireEvent.click(within(rowC).getByRole("button", { name: serverLabels.edit }));
      expect(within(rowA).getByRole("button", { name: serverLabels.remove })).toBeDisabled();
      expect(within(rowA).getByRole("button", { name: serverLabels.moveDown })).toBeDisabled();
      fireEvent.change(within(panel).getByLabelText(serverLabels.name), {
        target: { value: "C renamed" },
      });
      fireEvent.click(within(panel).getByRole("button", { name: serverLabels.save }));
      await waitFor(() =>
        expect(client.editServerList).toHaveBeenLastCalledWith("survival", {
          kind: "update",
          index: 2,
          form: { name: "C renamed", address: "c.example.com", acceptTextures: null },
        }),
      );
      expect(within(rowA).getByRole("button", { name: serverLabels.remove })).toBeEnabled();
    });

    it("explains why the list cannot change while playing", async () => {
      client.listServers.mockResolvedValue({ servers: [cinepol] });
      client.editServerList.mockRejectedValue(new IpcError("already_running", "busy"));
      const panel = await openServers();
      const row = (await within(panel).findByText("CINEPOL")).closest("li");
      if (!row) throw new Error("no server row");
      fireEvent.click(within(row).getByRole("button", { name: serverLabels.remove }));
      expect(await within(panel).findByText(esCommon.errors.already_running)).toBeInTheDocument();
    });
  });

  it("reports a crash and opens its report", async () => {
    const report = "crash-2026-10-19_12.00.00-client.txt";
    client.launchInstance.mockResolvedValue({ ...outcome, exitCode: 1, crashReport: report });
    client.openCrashReport.mockResolvedValue(undefined);
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    expect(await screen.findByText(labels.crashed)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.openCrashReport }));
    expect(client.openCrashReport).toHaveBeenCalledWith("survival", report);
  });

  it("keeps the plain exit message when the game failed without a report", async () => {
    client.launchInstance.mockResolvedValue({ ...outcome, exitCode: 1 });
    renderAt("/instances/survival");
    fireEvent.click(await screen.findByRole("button", { name: labels.play }));
    expect(await screen.findByText(labels.exited.replace("{{code}}", "1"))).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.openCrashReport })).not.toBeInTheDocument();
  });
});
