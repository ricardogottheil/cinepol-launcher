import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../../app/App";
import { createAppRouter } from "../../app/router";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type ProgressEvent } from "../../ipc/client";
import { initialBrowse, useModpackBrowse } from "./browseStore";
import { useModpackJobStore } from "./modpackJobStore";

const client = vi.hoisted(() => ({
  searchModpacks: vi.fn(),
  getModpack: vi.fn(),
  installModpack: vi.fn(),
  importMrpack: vi.fn(),
  importCurseForgeZip: vi.fn(),
  getModpackCatalogs: vi.fn(),
  installPackwiz: vi.fn(),
  importPackwiz: vi.fn(),
  listInstances: vi.fn(),
  listAccounts: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.modpacks;
const summary = {
  source: "modrinth",
  project: "1KVo5zza",
  title: "Fabulously Optimized",
  description: "Improve your graphics and performance",
  iconUrl: "https://cdn.modrinth.com/data/1KVo5zza/icon.png",
  downloads: 12_345_678,
  loaders: ["fabric"],
};
const version = {
  id: "uQxD0ENB",
  name: "14.1.1 for 26.2",
  versionNumber: "14.1.1",
  gameVersions: ["26.2"],
  loaders: ["fabric"],
  published: "2026-10-10T12:00:00Z",
  installable: true,
  size: 192_991,
};
const instance = {
  id: "fabulously-optimized",
  name: "Fabulously Optimized",
  gameVersion: "26.2",
  loader: "fabric",
  loaderVersion: "0.19.5",
  origin: "custom",
  modpackSource: null,
  updatePackOnLaunch: false,
  playtimeSecs: 0,
} as const;
const installed = {
  instance,
  files: { written: 51, unchanged: 0, kept: 0, removed: 0 },
  install: { downloadedFiles: 10, reusedFiles: 0, downloadedBytes: 1, javaMajor: 25 },
};

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  useModpackJobStore.setState({ job: { kind: "idle" }, error: null, installed: null });
  useModpackBrowse.setState(initialBrowse);
  client.searchModpacks.mockResolvedValue({ items: [summary], total: 1 });
  client.getModpack.mockResolvedValue({ summary, versions: [version] });
  client.getModpackCatalogs.mockResolvedValue({ curseforge: true });
  client.listInstances.mockResolvedValue([]);
  client.listAccounts.mockResolvedValue({ accounts: [], activeId: null });
});

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(<App router={router} />);
  return router;
}

async function openTab(name: string) {
  fireEvent.click(await screen.findByRole("tab", { name }));
  return screen.getByRole("tabpanel", { name });
}

async function openDetail(title = summary.title) {
  fireEvent.click(await screen.findByRole("link", { name: title }));
  return screen.findByRole("heading", { level: 1, name: title });
}

describe("catalogs", () => {
  it("offers only Modrinth when the build has no CurseForge key", async () => {
    client.getModpackCatalogs.mockResolvedValue({ curseforge: false });
    renderAt("/modpacks");
    await screen.findByRole("link", { name: summary.title });
    expect(screen.queryByRole("combobox", { name: labels.catalog })).not.toBeInTheDocument();
    expect(client.searchModpacks).toHaveBeenCalledWith(
      expect.objectContaining({ source: "modrinth" }),
    );
    const panel = await openTab(labels.tabs.import);
    expect(within(panel).getByRole("button", { name: labels.import })).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: labels.importZip })).not.toBeInTheDocument();
  });

  it("searches CurseForge, opens its packs there and imports its zips", async () => {
    const curseforge = {
      ...summary,
      source: "curseforge",
      project: "396246",
      title: "Fabulously Optimized (CurseForge)",
    };
    client.searchModpacks.mockImplementation(({ source }: { source: string }) =>
      Promise.resolve({ items: [source === "curseforge" ? curseforge : summary], total: 1 }),
    );
    client.getModpack.mockResolvedValue({ summary: curseforge, versions: [version] });
    client.importCurseForgeZip.mockReturnValue({
      operationId: "op",
      done: Promise.resolve(null),
      cancel: vi.fn(),
    });
    renderAt("/modpacks");
    const select = await screen.findByRole("combobox", { name: labels.catalog });
    fireEvent.change(select, { target: { value: "curseforge" } });

    await waitFor(() =>
      expect(client.searchModpacks).toHaveBeenLastCalledWith(
        expect.objectContaining({ source: "curseforge", page: 0 }),
      ),
    );
    expect(
      screen.getByRole("searchbox", {
        name: labels.searchLabel.replace("{{catalog}}", "CurseForge"),
      }),
    ).toBeInTheDocument();
    await openDetail(curseforge.title);
    await waitFor(() => expect(client.getModpack).toHaveBeenCalledWith("curseforge", "396246"));
    expect(screen.getByText(labels.sources.curseforge)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("link", { name: labels.back }));
    const panel = await openTab(labels.tabs.import);
    fireEvent.click(within(panel).getByRole("button", { name: labels.importZip }));
    expect(client.importCurseForgeZip).toHaveBeenCalled();
    expect(client.importMrpack).not.toHaveBeenCalled();
  });
});

describe("explore", () => {
  it("lists modpack cards with their loaders and searches as the player types", async () => {
    renderAt("/modpacks");
    const link = await screen.findByRole("link", { name: summary.title });
    const card = link.closest("article");
    if (!card) throw new Error("card not found");
    expect(within(card).getByText(summary.description)).toBeInTheDocument();
    expect(within(card).getByText(esCommon.instances.loaders.fabric)).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: labels.tabs.explore })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(client.searchModpacks).toHaveBeenCalledWith(
      expect.objectContaining({ query: "", page: 0, loader: null }),
    );
    fireEvent.change(screen.getByLabelText(labels.searchLabel.replace("{{catalog}}", "Modrinth")), {
      target: { value: "fabulously" },
    });
    await waitFor(() =>
      expect(client.searchModpacks).toHaveBeenCalledWith(
        expect.objectContaining({ query: "fabulously", page: 0 }),
      ),
    );
  });

  it("filters by loader", async () => {
    renderAt("/modpacks");
    await screen.findByRole("link", { name: summary.title });
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "fabric" } });
    await waitFor(() =>
      expect(client.searchModpacks).toHaveBeenCalledWith(
        expect.objectContaining({ loader: "fabric" }),
      ),
    );
  });

  it("shows a rate limit error from the search", async () => {
    // The app's query client retries a rate limit twice, waiting in between.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      client.searchModpacks.mockRejectedValue(new IpcError("rate_limited", "slow down"));
      renderAt("/modpacks");
      await vi.advanceTimersByTimeAsync(10_000);
      expect(await screen.findByText(esCommon.errors.rate_limited)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("modpack detail", () => {
  it("shows a version's spec and installs it with progress", async () => {
    const done = deferred<typeof installed>();
    let report: (event: ProgressEvent) => void = () => {};
    client.installModpack.mockImplementation(
      (
        _source: string,
        _project: string,
        _version: string,
        onProgress: (event: ProgressEvent) => void,
      ) => {
        report = onProgress;
        return { operationId: "op", done: done.promise, cancel: vi.fn() };
      },
    );
    renderAt("/modpacks");
    await openDetail();
    const versions = await screen.findByRole("region", { name: labels.versionsTitle });
    await within(versions).findByText("14.1.1");
    expect(within(versions).getByText("26.2")).toBeInTheDocument();
    fireEvent.click(within(versions).getByRole("button", { name: labels.install }));
    expect(client.installModpack).toHaveBeenCalledWith(
      "modrinth",
      summary.project,
      version.id,
      expect.any(Function),
    );
    report({ stage: "modpack", done: 3, total: 10, unit: "steps" });
    expect(await screen.findByText(esCommon.stages.modpack)).toBeInTheDocument();
    done.resolve(installed);
    expect(
      await screen.findByText(labels.installed.replace("{{name}}", instance.name)),
    ).toBeInTheDocument();
  });

  it("does not offer to install a version without a .mrpack", async () => {
    client.getModpack.mockResolvedValue({
      summary,
      versions: [{ ...version, installable: false, size: null }],
    });
    renderAt("/modpacks");
    await openDetail();
    const versions = await screen.findByRole("region", { name: labels.versionsTitle });
    await within(versions).findByText("14.1.1");
    expect(within(versions).getByRole("button", { name: labels.install })).toBeDisabled();
    expect(within(versions).getByText("Sin archivo .mrpack")).toBeInTheDocument();
  });

  it("shows why an install failed", async () => {
    client.installModpack.mockImplementation(() => ({
      operationId: "op",
      done: Promise.reject(new IpcError("invalid_modpack", "bad pack")),
      cancel: vi.fn(),
    }));
    renderAt("/modpacks");
    await openDetail();
    const versions = await screen.findByRole("region", { name: labels.versionsTitle });
    fireEvent.click(await within(versions).findByRole("button", { name: labels.install }));
    expect(await screen.findByText(esCommon.errors.invalid_modpack)).toBeInTheDocument();
  });

  it("goes back to the catalog", async () => {
    renderAt("/modpacks/modrinth/1KVo5zza");
    expect(
      await screen.findByRole("heading", { level: 1, name: summary.title }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("link", { name: labels.back }));
    expect(await screen.findByRole("tab", { name: labels.tabs.explore })).toBeInTheDocument();
  });
});

describe("installed", () => {
  it("lists only the instances that come from a modpack", async () => {
    client.listInstances.mockResolvedValue([
      { ...instance, id: "fo", origin: "modpack", modpackSource: "modrinth" },
      { ...instance, id: "plain", name: "Plain 26.2", origin: "custom" },
    ]);
    renderAt("/modpacks");
    const panel = await openTab(labels.tabs.installed);
    expect(await within(panel).findByText(instance.name)).toBeInTheDocument();
    expect(within(panel).queryByText("Plain 26.2")).not.toBeInTheDocument();
  });

  it("says when no modpack is installed", async () => {
    renderAt("/modpacks");
    const panel = await openTab(labels.tabs.installed);
    expect(await within(panel).findByText(labels.noInstalled)).toBeInTheDocument();
  });
});

describe("import", () => {
  const packwiz = esCommon.modpacks.packwiz;
  const jei = {
    name: "Just Enough Items (JEI)",
    filename: "jei-1.20.1-forge-15.20.0.133.jar",
    pageUrl: "https://www.curseforge.com/projects/238222",
  };

  it("imports a local .mrpack and ignores a closed picker", async () => {
    client.importMrpack.mockReturnValue({
      operationId: "op",
      done: Promise.resolve(null),
      cancel: vi.fn(),
    });
    renderAt("/modpacks");
    await screen.findByRole("link", { name: summary.title });
    const panel = await openTab(labels.tabs.import);
    fireEvent.click(within(panel).getByRole("button", { name: labels.import }));
    await waitFor(() => expect(client.importMrpack).toHaveBeenCalled());
    await waitFor(() => expect(useModpackJobStore.getState().job.kind).toBe("idle"));
    const { error, installed: done } = useModpackJobStore.getState();
    expect(error).toBeNull();
    expect(done).toBeNull();
    expect(screen.queryByText(labels.importing)).not.toBeInTheDocument();
  });

  it("installs a pack from its pack.toml URL and says what is left to download", async () => {
    client.installPackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ ...installed, pending: [jei] }),
      cancel: vi.fn(),
    });
    renderAt("/modpacks");
    const panel = await openTab(labels.tabs.import);
    const url = "https://example.com/pack/pack.toml";
    fireEvent.change(within(panel).getByLabelText(packwiz.urlLabel), {
      target: { value: ` ${url} ` },
    });
    fireEvent.click(within(panel).getByRole("button", { name: packwiz.install }));
    expect(client.installPackwiz).toHaveBeenCalledWith(url, expect.any(Function));
    expect(
      await screen.findByText(
        labels.installedPending_one.replace("{{name}}", instance.name).replace("{{count}}", "1"),
      ),
    ).toBeInTheDocument();
  });

  it("does not install without a URL", async () => {
    renderAt("/modpacks");
    const panel = await openTab(labels.tabs.import);
    expect(within(panel).getByRole("button", { name: packwiz.install })).toBeDisabled();
  });

  it("opens a local pack.toml with the picker", async () => {
    client.importPackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ ...installed, pending: [] }),
      cancel: vi.fn(),
    });
    renderAt("/modpacks");
    const panel = await openTab(labels.tabs.import);
    fireEvent.click(within(panel).getByRole("button", { name: packwiz.import }));
    expect(
      await screen.findByText(labels.installed.replace("{{name}}", instance.name)),
    ).toBeInTheDocument();
  });
});
