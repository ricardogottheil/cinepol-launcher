import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError, type ProgressEvent } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { ModpacksPage } from "./ModpacksPage";
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
  client.searchModpacks.mockResolvedValue({ items: [summary], total: 1 });
  client.getModpack.mockResolvedValue({ summary, versions: [version] });
  client.getModpackCatalogs.mockResolvedValue({ curseforge: true });
});

describe("catalogs", () => {
  it("offers only Modrinth when the build has no CurseForge key", async () => {
    client.getModpackCatalogs.mockResolvedValue({ curseforge: false });
    renderWithQuery(<ModpacksPage />);
    await screen.findByRole("heading", { name: summary.title });
    expect(screen.queryByRole("combobox", { name: labels.catalog })).not.toBeInTheDocument();
    expect(client.searchModpacks).toHaveBeenCalledWith(
      expect.objectContaining({ source: "modrinth" }),
    );
    expect(screen.getByRole("button", { name: labels.import })).toBeInTheDocument();
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
    client.importCurseForgeZip.mockReturnValue({
      operationId: "op",
      done: Promise.resolve(null),
      cancel: vi.fn(),
    });
    renderWithQuery(<ModpacksPage />);
    const select = await screen.findByRole("combobox", { name: labels.catalog });
    fireEvent.change(select, { target: { value: "curseforge" } });

    await waitFor(() =>
      expect(client.searchModpacks).toHaveBeenLastCalledWith(
        expect.objectContaining({ source: "curseforge", page: 0 }),
      ),
    );
    expect(screen.getByText(labels.sources.curseforge)).toBeInTheDocument();
    expect(
      screen.getByRole("searchbox", {
        name: labels.searchLabel.replace("{{catalog}}", "CurseForge"),
      }),
    ).toBeInTheDocument();
    const row = (await screen.findByRole("heading", { name: curseforge.title })).closest("li");
    if (!row) throw new Error("row not found");
    fireEvent.click(within(row).getByRole("button", { name: labels.showVersions }));
    await waitFor(() => expect(client.getModpack).toHaveBeenCalledWith("curseforge", "396246"));

    fireEvent.click(screen.getByRole("button", { name: labels.importZip }));
    expect(client.importCurseForgeZip).toHaveBeenCalled();
    expect(client.importMrpack).not.toHaveBeenCalled();
  });
});

async function openVersions() {
  renderWithQuery(<ModpacksPage />);
  const row = (await screen.findByRole("heading", { name: summary.title })).closest("li");
  if (!row) throw new Error("row not found");
  fireEvent.click(within(row).getByRole("button", { name: labels.showVersions }));
  await within(row).findByText("14.1.1");
  return row;
}

describe("ModpacksPage", () => {
  it("lists Modrinth modpacks and searches as the player types", async () => {
    renderWithQuery(<ModpacksPage />);
    expect(await screen.findByRole("heading", { name: summary.title })).toBeInTheDocument();
    expect(screen.getByText(summary.description)).toBeInTheDocument();
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
    renderWithQuery(<ModpacksPage />);
    await screen.findByRole("heading", { name: summary.title });
    fireEvent.change(screen.getByLabelText(labels.loaderLabel), { target: { value: "fabric" } });
    await waitFor(() =>
      expect(client.searchModpacks).toHaveBeenCalledWith(
        expect.objectContaining({ loader: "fabric" }),
      ),
    );
  });

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
    const row = await openVersions();
    expect(within(row).getByText("26.2")).toBeInTheDocument();
    fireEvent.click(within(row).getByRole("button", { name: labels.install }));
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
    const row = await openVersions();
    expect(within(row).getByRole("button", { name: labels.install })).toBeDisabled();
    expect(within(row).getByText("Sin archivo .mrpack")).toBeInTheDocument();
  });

  it("imports a local .mrpack and ignores a closed picker", async () => {
    client.importMrpack.mockReturnValue({
      operationId: "op",
      done: Promise.resolve(null),
      cancel: vi.fn(),
    });
    renderWithQuery(<ModpacksPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.import }));
    await waitFor(() => expect(client.importMrpack).toHaveBeenCalled());
    expect(screen.queryByRole("status")?.textContent ?? "").toBe("");
  });

  it("shows why an install failed", async () => {
    client.installModpack.mockImplementation(() => ({
      operationId: "op",
      done: Promise.reject(new IpcError("invalid_modpack", "bad pack")),
      cancel: vi.fn(),
    }));
    const row = await openVersions();
    fireEvent.click(within(row).getByRole("button", { name: labels.install }));
    expect(await screen.findByText(esCommon.errors.invalid_modpack)).toBeInTheDocument();
  });

  it("shows a rate limit error from the search", async () => {
    client.searchModpacks.mockRejectedValue(new IpcError("rate_limited", "slow down"));
    renderWithQuery(<ModpacksPage />);
    expect(await screen.findByText(esCommon.errors.rate_limited)).toBeInTheDocument();
  });
});

describe("ModpacksPage packwiz", () => {
  const packwiz = esCommon.modpacks.packwiz;
  const jei = {
    name: "Just Enough Items (JEI)",
    filename: "jei-1.20.1-forge-15.20.0.133.jar",
    pageUrl: "https://www.curseforge.com/projects/238222",
  };

  it("installs a pack from its pack.toml URL and says what is left to download", async () => {
    client.installPackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ ...installed, pending: [jei] }),
      cancel: vi.fn(),
    });
    renderWithQuery(<ModpacksPage />);
    const url = "https://example.com/pack/pack.toml";
    fireEvent.change(await screen.findByLabelText(packwiz.urlLabel), {
      target: { value: ` ${url} ` },
    });
    fireEvent.click(screen.getByRole("button", { name: packwiz.install }));
    expect(client.installPackwiz).toHaveBeenCalledWith(url, expect.any(Function));
    expect(
      await screen.findByText(
        labels.installedPending_one.replace("{{name}}", instance.name).replace("{{count}}", "1"),
      ),
    ).toBeInTheDocument();
  });

  it("does not install without a URL", async () => {
    renderWithQuery(<ModpacksPage />);
    expect(await screen.findByRole("button", { name: packwiz.install })).toBeDisabled();
  });

  it("opens a local pack.toml with the picker", async () => {
    client.importPackwiz.mockReturnValue({
      operationId: "op",
      done: Promise.resolve({ ...installed, pending: [] }),
      cancel: vi.fn(),
    });
    renderWithQuery(<ModpacksPage />);
    fireEvent.click(await screen.findByRole("button", { name: packwiz.import }));
    expect(
      await screen.findByText(labels.installed.replace("{{name}}", instance.name)),
    ).toBeInTheDocument();
  });
});
