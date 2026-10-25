import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { type InstanceDto, IpcError, type ModEntryDto } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { MOD_NOTICE_MS, ModJobStatus, ModsPanel, useModJob } from "./ModsPanel";

const client = vi.hoisted(() => ({
  listInstanceMods: vi.fn(),
  checkModUpdates: vi.fn(),
  setModEnabled: vi.fn(),
  removeMod: vi.fn(),
  searchMods: vi.fn(),
  previewModInstall: vi.fn(),
  installMod: vi.fn(),
  updateMod: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.mods;

/** A translated string with its `{{placeholders}}` filled in. */
function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{\{(\w+)\}\}/g, (_, key: string) => String(values[key]));
}

const fabric: InstanceDto = {
  id: "survival",
  name: "Survival",
  gameVersion: "1.21.1",
  loader: "fabric",
  loaderVersion: "0.16.5",
  origin: "custom",
  modpackSource: null,
  updatePackOnLaunch: false,
  playtimeSecs: 0,
} as InstanceDto;

const iris: ModEntryDto = {
  filename: "iris.jar",
  enabled: true,
  title: "Iris Shaders",
  origin: "launcher",
  project: "iris",
  versionNumber: "1.8.8",
  requested: true,
  requiredBy: [],
  orphans: ["Sodium"],
};
const sodium: ModEntryDto = {
  filename: "sodium.jar",
  enabled: false,
  title: "Sodium",
  origin: "launcher",
  project: "sodium",
  versionNumber: "0.6.0",
  requested: false,
  requiredBy: ["Iris Shaders"],
  orphans: [],
};
const loose: ModEntryDto = {
  filename: "private-tweaks.jar",
  enabled: true,
  title: "private-tweaks.jar",
  origin: "unknown",
  project: null,
  versionNumber: null,
  requested: true,
  requiredBy: [],
  orphans: [],
};

function Panel({ instance = fabric, busy = false }: { instance?: InstanceDto; busy?: boolean }) {
  const jobs = useModJob(instance.id);
  return (
    <>
      <ModJobStatus jobs={jobs} />
      <ModsPanel instance={instance} busy={busy} jobs={jobs} />
    </>
  );
}

/** An operation that reports progress and stays running until `finish` is called. */
function pendingOperation() {
  let finish: (value: unknown) => void = () => {};
  const done = new Promise((resolve) => {
    finish = resolve;
  });
  const cancel = vi.fn(async () => true);
  return {
    start: (_id: string, _project: string, onProgress: (event: unknown) => void) => {
      onProgress({ stage: "mods", done: 1, total: 2, unit: "steps" });
      return { operationId: "op", done, cancel };
    },
    finish,
    cancel,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  client.listInstanceMods.mockResolvedValue([iris, sodium, loose]);
  client.setModEnabled.mockResolvedValue(undefined);
  client.removeMod.mockResolvedValue(["iris.jar", "sodium.jar"]);
  client.searchMods.mockResolvedValue({ items: [], total: 0 });
});

describe("ModsPanel", () => {
  it("explains that Vanilla instances need a loader and asks nothing of the backend", () => {
    renderWithQuery(<Panel instance={{ ...fabric, loader: "vanilla", loaderVersion: null }} />);
    expect(screen.getByText(labels.vanilla)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.add })).not.toBeInTheDocument();
    expect(client.listInstanceMods).not.toHaveBeenCalled();
  });

  it("lists the installed mods with why each is there, and turns one on", async () => {
    renderWithQuery(<Panel />);
    const list = await screen.findByRole("list", { name: labels.installedLabel });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(within(rows[1] as HTMLElement).getByText(labels.off)).toBeInTheDocument();
    expect(
      within(rows[1] as HTMLElement).getByText(fill(labels.neededBy, { mods: "Iris Shaders" }), {
        exact: false,
      }),
    ).toBeInTheDocument();
    expect(
      within(rows[2] as HTMLElement).getByText(labels.notOnModrinth, { exact: false }),
    ).toBeInTheDocument();

    fireEvent.click(within(rows[1] as HTMLElement).getByRole("button", { name: labels.enable }));
    await waitFor(() =>
      expect(client.setModEnabled).toHaveBeenCalledWith("survival", "sodium.jar", true),
    );
  });

  it("removes a mod with the dependencies only it needed, unless the player keeps them", async () => {
    renderWithQuery(<Panel />);
    const list = await screen.findByRole("list", { name: labels.installedLabel });
    const irisRow = within(list).getAllByRole("listitem")[0] as HTMLElement;

    fireEvent.click(within(irisRow).getByRole("button", { name: labels.remove }));
    let dialog = screen.getByRole("alertdialog");
    const orphans = within(dialog).getByRole("checkbox");
    expect(orphans).toBeChecked();
    expect(within(dialog).getByText(/Sodium/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: labels.remove }));
    await waitFor(() =>
      expect(client.removeMod).toHaveBeenCalledWith("survival", "iris.jar", true),
    );

    fireEvent.click(within(irisRow).getByRole("button", { name: labels.remove }));
    dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(within(dialog).getByRole("button", { name: labels.remove }));
    await waitFor(() =>
      expect(client.removeMod).toHaveBeenLastCalledWith("survival", "iris.jar", false),
    );
  });

  it("warns that a needed dependency may break what needs it", async () => {
    renderWithQuery(<Panel />);
    const list = await screen.findByRole("list", { name: labels.installedLabel });
    const sodiumRow = within(list).getAllByRole("listitem")[1] as HTMLElement;
    fireEvent.click(within(sodiumRow).getByRole("button", { name: labels.remove }));
    const dialog = screen.getByRole("alertdialog");
    expect(
      within(dialog).getByText(fill(labels.removeNeeded, { mods: "Iris Shaders" })),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("checks for updates on request, names the mods it could not check, and updates one", async () => {
    client.checkModUpdates.mockResolvedValue({
      updates: [
        {
          project: "iris",
          title: "Iris Shaders",
          filename: "iris.jar",
          current: "1.8.8",
          versionNumber: "1.9.0",
        },
      ],
      unchecked: ["Sodium"],
    });
    const operation = pendingOperation();
    client.updateMod.mockImplementation(operation.start);
    renderWithQuery(<Panel />);
    await screen.findByRole("list", { name: labels.installedLabel });
    expect(client.checkModUpdates).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: labels.checkUpdates }));
    expect(
      await screen.findByText(fill(labels.updatesFound_one, { count: 1 })),
    ).toBeInTheDocument();
    expect(screen.getByText(fill(labels.unchecked, { mods: "Sodium" }))).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: fill(labels.update, { version: "1.9.0" }) }),
    );
    expect(
      await screen.findByText(fill(labels.updating, { mod: "Iris Shaders" })),
    ).toBeInTheDocument();
    expect(client.updateMod).toHaveBeenCalledWith("survival", "iris", expect.any(Function));
    fireEvent.click(screen.getByRole("button", { name: labels.cancelInstall }));
    expect(operation.cancel).toHaveBeenCalledTimes(1);
  });

  it("searches for this instance, confirms the dependencies, then installs with progress", async () => {
    client.searchMods.mockResolvedValue({
      items: [
        {
          project: "iris",
          title: "Iris Shaders",
          description: "Shaders",
          iconUrl: null,
          downloads: 10,
        },
        {
          project: "lithium",
          title: "Lithium",
          description: "Faster ticks",
          iconUrl: null,
          downloads: 5,
        },
      ],
      total: 2,
    });
    client.previewModInstall.mockResolvedValue({
      mods: [
        {
          project: "lithium",
          title: "Lithium",
          versionNumber: "0.13.0",
          requested: true,
          requiredBy: [],
        },
        {
          project: "fabric-api",
          title: "Fabric API",
          versionNumber: "0.105.0",
          requested: false,
          requiredBy: ["Lithium"],
        },
      ],
    });
    const operation = pendingOperation();
    client.installMod.mockImplementation(operation.start);
    renderWithQuery(<Panel />);
    fireEvent.click(await screen.findByRole("button", { name: labels.add }));

    expect(screen.getByText("1.21.1 · Fabric")).toBeInTheDocument();
    const results = await screen.findByRole("region", { name: labels.results });
    await within(results).findByText("Lithium");
    expect(client.searchMods).toHaveBeenCalledWith(
      expect.objectContaining({ instanceId: "survival", page: 0 }),
    );
    // Already installed: no second install offered.
    expect(within(results).getByText(labels.installed)).toBeInTheDocument();

    fireEvent.click(within(results).getByRole("button", { name: `${labels.install} Lithium` }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Fabric API")).toBeInTheDocument();
    expect(
      within(dialog).getByText(fill(labels.requiredBy, { mods: "Lithium" }), { exact: false }),
    ).toBeInTheDocument();
    fireEvent.click(
      within(dialog).getByRole("button", { name: fill(labels.confirmInstall_other, { count: 2 }) }),
    );

    expect(
      await screen.findByText(fill(labels.installing, { mod: "Lithium" })),
    ).toBeInTheDocument();
    expect(client.installMod).toHaveBeenCalledWith("survival", "lithium", expect.any(Function));
    operation.finish([
      { project: "lithium", title: "Lithium", versionNumber: "0.13.0", filename: "l.jar" },
      { project: "fabric-api", title: "Fabric API", versionNumber: "0.105.0", filename: "f.jar" },
    ]);
    expect(
      await screen.findByText(fill(labels.installedDone_other, { mods: "Lithium, Fabric API" })),
    ).toBeInTheDocument();
  });

  it("says why a mod cannot be installed, naming the mod it conflicts with", async () => {
    client.searchMods.mockResolvedValue({
      items: [
        {
          project: "optifabric",
          title: "OptiFabric",
          description: "",
          iconUrl: null,
          downloads: 1,
        },
      ],
      total: 1,
    });
    client.previewModInstall.mockRejectedValue(
      new IpcError("mod_incompatible", "cannot run together", "Sodium"),
    );
    renderWithQuery(<Panel />);
    fireEvent.click(await screen.findByRole("button", { name: labels.add }));
    const results = await screen.findByRole("region", { name: labels.results });
    fireEvent.click(
      await within(results).findByRole("button", { name: `${labels.install} OptiFabric` }),
    );

    expect(
      await within(results).findByText(fill(labels.errors.incompatibleWith, { mod: "Sodium" })),
    ).toBeInTheDocument();
    expect(within(results).getByText(labels.notCompatible)).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(client.installMod).not.toHaveBeenCalled();
  });

  it.each([
    "mod_incompatible",
    "no_compatible_version",
    "too_many_dependencies",
    "already_exists",
  ] as const)("marks a mod refused with %s as not compatible, with no retry", async (code) => {
    client.searchMods.mockResolvedValue({
      items: [
        { project: "lithium", title: "Lithium", description: "", iconUrl: null, downloads: 1 },
      ],
      total: 1,
    });
    client.previewModInstall.mockRejectedValue(new IpcError(code, "refused"));
    renderWithQuery(<Panel />);
    fireEvent.click(await screen.findByRole("button", { name: labels.add }));
    const results = await screen.findByRole("region", { name: labels.results });
    fireEvent.click(
      await within(results).findByRole("button", { name: `${labels.install} Lithium` }),
    );

    expect(await within(results).findByText(esCommon.errors[code])).toBeInTheDocument();
    expect(within(results).getByText(labels.notCompatible)).toBeInTheDocument();
    expect(
      within(results).queryByRole("button", { name: `${labels.install} Lithium` }),
    ).not.toBeInTheDocument();
    expect(client.previewModInstall).toHaveBeenCalledTimes(1);
  });

  it("lets the player retry a mod whose check failed for a passing reason", async () => {
    client.searchMods.mockResolvedValue({
      items: [
        { project: "lithium", title: "Lithium", description: "", iconUrl: null, downloads: 1 },
      ],
      total: 1,
    });
    client.previewModInstall
      .mockRejectedValueOnce(new IpcError("network", "offline"))
      .mockResolvedValueOnce({
        mods: [
          {
            project: "lithium",
            title: "Lithium",
            versionNumber: "0.13.0",
            requested: true,
            requiredBy: [],
          },
        ],
      });
    renderWithQuery(<Panel />);
    fireEvent.click(await screen.findByRole("button", { name: labels.add }));
    const results = await screen.findByRole("region", { name: labels.results });
    const install = await within(results).findByRole("button", {
      name: `${labels.install} Lithium`,
    });
    fireEvent.click(install);

    expect(await within(results).findByText(esCommon.errors.network)).toBeInTheDocument();
    expect(within(results).queryByText(labels.notCompatible)).not.toBeInTheDocument();
    fireEvent.click(within(results).getByRole("button", { name: `${labels.install} Lithium` }));
    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
    expect(within(results).queryByText(esCommon.errors.network)).not.toBeInTheDocument();
  });

  it("clears the install notice after a while", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const operation = pendingOperation();
      client.updateMod.mockImplementation(operation.start);
      client.checkModUpdates.mockResolvedValue({
        updates: [
          {
            project: "iris",
            title: "Iris Shaders",
            filename: "iris.jar",
            current: "1.8.8",
            versionNumber: "1.9.0",
          },
        ],
        unchecked: [],
      });
      renderWithQuery(<Panel />);
      await screen.findByRole("list", { name: labels.installedLabel });
      fireEvent.click(screen.getByRole("button", { name: labels.checkUpdates }));
      fireEvent.click(
        await screen.findByRole("button", { name: fill(labels.update, { version: "1.9.0" }) }),
      );
      await screen.findByText(fill(labels.updating, { mod: "Iris Shaders" }));
      operation.finish([]);
      const done = fill(labels.updatedDone, { mod: "Iris Shaders" });
      expect(await screen.findByText(done)).toBeInTheDocument();
      await vi.advanceTimersByTimeAsync(MOD_NOTICE_MS);
      expect(screen.queryByText(done)).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});
