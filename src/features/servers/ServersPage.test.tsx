import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../../app/App";
import { createAppRouter } from "../../app/router";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError } from "../../ipc/client";
import { useActivityStore } from "../instances/activityStore";
import { bundledServerConfig } from "../server/serverConfig";

const client = vi.hoisted(() => ({
  listSavedServers: vi.fn(),
  saveServer: vi.fn(),
  removeSavedServer: vi.fn(),
  pingServer: vi.fn(),
  listInstances: vi.fn(),
  launchInstance: vi.fn(),
  playOnCinepol: vi.fn(),
  getServerConfig: vi.fn(),
  listAccounts: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const labels = esCommon.servers;
const hypixel = {
  id: "0b7e6c1e-5d3b-4a43-9c49-1c0c6d1c8e11",
  name: "Hypixel",
  address: "mc.hypixel.net",
  instanceId: "skyblock" as string | null,
};
const skyblock = {
  id: "skyblock",
  name: "Skyblock",
  gameVersion: "26.2",
  loader: "vanilla",
  loaderVersion: null,
  origin: "custom",
  modpackSource: null,
  updatePackOnLaunch: false,
  playtimeSecs: 0,
} as const;
const status = {
  onlinePlayers: 30_000,
  maxPlayers: 100_000,
  versionName: "Requires MC 1.8 / 26.2",
  motd: "Hypixel Network",
  latencyMs: 40,
};

beforeEach(() => {
  vi.clearAllMocks();
  useActivityStore.setState({ byInstance: {} });
  client.listSavedServers.mockResolvedValue([hypixel]);
  client.pingServer.mockImplementation((address: string) =>
    Promise.resolve(
      address === hypixel.address ? status : { ...status, onlinePlayers: 9, maxPlayers: 100 },
    ),
  );
  client.listInstances.mockResolvedValue([skyblock]);
  client.listAccounts.mockResolvedValue({ accounts: [], activeId: null });
  client.saveServer.mockImplementation((form: typeof hypixel) =>
    Promise.resolve({ ...form, id: form.id ?? "new-id" }),
  );
  client.removeSavedServer.mockResolvedValue(null);
  client.launchInstance.mockReturnValue(new Promise(() => {}));
  client.getServerConfig.mockRejectedValue(new IpcError("ipc_unavailable", "no backend"));
  client.playOnCinepol.mockReturnValue({
    operationId: "op",
    done: new Promise(() => {}),
    cancel: vi.fn(),
  });
});

function renderAt(path: string) {
  render(<App router={createAppRouter(createMemoryHistory({ initialEntries: [path] }))} />);
}

async function savedRow(name = hypixel.name) {
  const list = await screen.findByRole("list", { name: labels.yourServers });
  const row = (await within(list).findByRole("heading", { name })).closest("li");
  if (!row) throw new Error("row not found");
  return row;
}

describe("servers page", () => {
  it("pins CINEPOL above the saved servers, each with its live status", async () => {
    renderAt("/servers");
    const cinepol = await screen.findByRole("region", { name: "CINEPOL" });
    const list = await screen.findByRole("list", { name: labels.yourServers });
    expect(cinepol.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(await within(cinepol).findByText("9 / 100")).toBeInTheDocument();

    const row = await savedRow();
    expect(within(row).getByText(hypixel.address)).toBeInTheDocument();
    expect(await within(row).findByText("30000 / 100000")).toBeInTheDocument();
    expect(within(row).getByText("40 ms")).toBeInTheDocument();
    expect(client.pingServer).toHaveBeenCalledWith(hypixel.address);
  });

  it("pins every community server and plays the one picked", async () => {
    client.getServerConfig.mockResolvedValue({
      name: "CINEPOL",
      servers: [
        { name: "CINEPOL", address: "mc.policarpiers.com" },
        { name: "CINEPOL Creative", address: "creative.policarpiers.com:25570" },
      ],
      minVersion: "1.21.11",
      maxVersion: "26.2",
      recommendedVersion: "26.2",
      links: { website: "https://example.test", twitch: "", discord: "" },
      origin: "remote",
    });
    renderAt("/servers");
    const creative = await screen.findByRole("region", { name: "CINEPOL Creative" });
    expect(screen.getByRole("region", { name: "CINEPOL" })).toBeInTheDocument();
    expect(within(creative).getByText("creative.policarpiers.com:25570")).toBeInTheDocument();

    fireEvent.click(within(creative).getByRole("button", { name: new RegExp(labels.play) }));
    await waitFor(() => expect(client.playOnCinepol).toHaveBeenCalled());
    expect(client.playOnCinepol.mock.calls[0]?.[0]).toMatchObject({
      server: "creative.policarpiers.com:25570",
    });
  });

  it("leaves the main server to the backend's config when playing it", async () => {
    renderAt("/servers");
    const cinepol = await screen.findByRole("region", { name: "CINEPOL" });
    fireEvent.click(within(cinepol).getByRole("button", { name: new RegExp(labels.play) }));
    await waitFor(() => expect(client.playOnCinepol).toHaveBeenCalled());
    expect(client.playOnCinepol.mock.calls[0]?.[0]).not.toHaveProperty("server");
  });

  it("offers only the main server until the backend's config arrives", async () => {
    const bundled = [...bundledServerConfig.servers];
    bundledServerConfig.servers.push({
      name: "CINEPOL Creative",
      address: "creative.example.test",
    });
    try {
      client.getServerConfig.mockReturnValue(new Promise(() => {}));
      renderAt("/servers");
      expect(await screen.findByRole("region", { name: "CINEPOL" })).toBeInTheDocument();
      await savedRow();
      expect(screen.queryByRole("region", { name: "CINEPOL Creative" })).toBeNull();
    } finally {
      bundledServerConfig.servers.splice(0, Infinity, ...bundled);
    }
  });

  it("shows only the saved servers when the config lists none", async () => {
    client.getServerConfig.mockResolvedValue({
      name: "CINEPOL",
      servers: [],
      minVersion: "1.21.11",
      maxVersion: "26.2",
      recommendedVersion: "26.2",
      links: { website: "https://example.test", twitch: "", discord: "" },
      origin: "remote",
    });
    renderAt("/servers");
    await savedRow();
    await waitFor(() => expect(screen.queryByRole("region", { name: "CINEPOL" })).toBeNull());
  });

  it("is in the Game group of the navigation", async () => {
    renderAt("/servers");
    const nav = await screen.findByRole("navigation", { name: esCommon.chrome.mainNavigation });
    const game = within(nav).getByRole("list", { name: esCommon.navGroups.game });
    expect(within(game).getByRole("link", { name: esCommon.nav.servers })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("plays a saved server with its instance and joins it", async () => {
    renderAt("/servers");
    fireEvent.click(within(await savedRow()).getByRole("button", { name: labels.play }));
    await waitFor(() =>
      expect(client.launchInstance).toHaveBeenCalledWith(
        "skyblock",
        expect.any(Function),
        hypixel.address,
      ),
    );
  });

  it("asks for an instance before playing a server that has none", async () => {
    client.listSavedServers.mockResolvedValue([{ ...hypixel, instanceId: null }]);
    renderAt("/servers");
    const row = await savedRow();
    fireEvent.click(within(row).getByRole("button", { name: labels.play }));
    expect(await within(row).findByText(labels.needsInstance)).toBeInTheDocument();
    expect(within(row).getByLabelText(labels.instanceLabel)).toHaveFocus();
    expect(client.launchInstance).not.toHaveBeenCalled();
  });

  it("remembers the instance picked for a server", async () => {
    client.listSavedServers.mockResolvedValue([{ ...hypixel, instanceId: null }]);
    renderAt("/servers");
    const row = await savedRow();
    fireEvent.change(within(row).getByLabelText(labels.instanceLabel), {
      target: { value: "skyblock" },
    });
    await waitFor(() =>
      expect(client.saveServer).toHaveBeenCalledWith({ ...hypixel, instanceId: "skyblock" }),
    );
  });

  it("adds a server and explains a duplicate", async () => {
    renderAt("/servers");
    const form = await screen.findByRole("form", { name: labels.addTitle });
    fireEvent.change(within(form).getByLabelText(labels.nameLabel), {
      target: { value: "Local" },
    });
    fireEvent.change(within(form).getByLabelText(labels.addressLabel), {
      target: { value: "127.0.0.1:25566" },
    });
    fireEvent.click(within(form).getByRole("button", { name: labels.add }));
    await waitFor(() =>
      expect(client.saveServer).toHaveBeenCalledWith({
        id: null,
        name: "Local",
        address: "127.0.0.1:25566",
        instanceId: null,
      }),
    );
    await waitFor(() => expect(within(form).getByLabelText(labels.nameLabel)).toHaveValue(""));

    client.saveServer.mockRejectedValue(new IpcError("already_exists", "duplicate"));
    fireEvent.change(within(form).getByLabelText(labels.nameLabel), {
      target: { value: "Again" },
    });
    fireEvent.change(within(form).getByLabelText(labels.addressLabel), {
      target: { value: "MC.HYPIXEL.NET" },
    });
    fireEvent.click(within(form).getByRole("button", { name: labels.add }));
    expect(await within(form).findByText(labels.alreadySaved)).toBeInTheDocument();
  });

  it("edits a server's name and address", async () => {
    renderAt("/servers");
    const row = await savedRow();
    fireEvent.click(within(row).getByRole("button", { name: labels.edit }));
    fireEvent.change(within(row).getByLabelText(labels.nameLabel), {
      target: { value: "Hypixel EU" },
    });
    fireEvent.click(within(row).getByRole("button", { name: labels.save }));
    await waitFor(() =>
      expect(client.saveServer).toHaveBeenCalledWith({ ...hypixel, name: "Hypixel EU" }),
    );
  });

  it("removes a server after a confirmation", async () => {
    renderAt("/servers");
    const row = await savedRow();
    fireEvent.click(within(row).getByRole("button", { name: labels.remove }));
    expect(client.removeSavedServer).not.toHaveBeenCalled();
    fireEvent.click(within(row).getByRole("button", { name: labels.confirmRemove }));
    await waitFor(() => expect(client.removeSavedServer).toHaveBeenCalledWith(hypixel.id));
  });

  it("shows an unreachable server as offline", async () => {
    client.pingServer.mockRejectedValue(new IpcError("server_unreachable", "timed out"));
    renderAt("/servers");
    const row = await savedRow();
    expect(await within(row).findByText(esCommon.home.status.offline)).toBeInTheDocument();
  });

  it("says when no server is saved yet", async () => {
    client.listSavedServers.mockResolvedValue([]);
    renderAt("/servers");
    expect(await screen.findByText(labels.empty)).toBeInTheDocument();
  });
});
