import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { act, fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import serverConfig from "../../../config/server-config.json";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { CINEPOL_SESSION, useActivityStore } from "../instances/activityStore";
import { HomePage } from "./HomePage";

const client = vi.hoisted(() => ({
  openCrashReport: vi.fn(),
  listAccounts: vi.fn(),
  listInstances: vi.fn(),
  getServerConfig: vi.fn(),
  getServerStatus: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

beforeEach(() => {
  client.listAccounts.mockResolvedValue({ accounts: [], activeId: null });
  client.listInstances.mockResolvedValue([]);
  client.getServerConfig.mockRejectedValue(new IpcError("internal", "no backend"));
  client.getServerStatus.mockRejectedValue(new IpcError("internal", "no backend"));
  useActivityStore.setState({ byInstance: {} });
});

const liveStatus = {
  onlinePlayers: 8,
  maxPlayers: 100,
  versionName: "Velocity",
  protocol: 777,
  motd: "Welcome to CINEPOL",
  latencyMs: 42,
};

function setSession(kind: "installing" | "running") {
  const cancel = vi.fn().mockResolvedValue(true);
  useActivityStore.setState({
    byInstance: {
      [CINEPOL_SESSION]: {
        activity:
          kind === "running"
            ? { kind, cancel }
            : { kind, stage: "libraries", done: 0, total: null, unit: "bytes", cancel },
        logs: [],
        error: null,
        lastExit: null,
      },
    },
  });
}

function renderHome() {
  const rootRoute = createRootRoute({ component: HomePage });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  return renderWithQuery(<RouterProvider router={router} />);
}

describe("HomePage", () => {
  it("offers the play control for the recommended version from config", async () => {
    renderHome();
    const play = await screen.findByRole("button", { name: new RegExp(esCommon.home.play) });
    expect(play).toHaveTextContent(`Minecraft ${serverConfig.versions.recommended}`);
  });

  it("shows the server address and the supported range from config", async () => {
    renderHome();
    const { min, max } = serverConfig.versions;
    expect(await screen.findByText(serverConfig.server.address)).toBeInTheDocument();
    expect(screen.getByText(`${min} – ${max}`)).toBeInTheDocument();
  });

  it("asks for an account when none is active", async () => {
    renderHome();
    expect(await screen.findByRole("link", { name: esCommon.home.addAccount })).toHaveAttribute(
      "href",
      "/profile/new",
    );
  });

  it("shows the active account instead of the prompt", async () => {
    client.listAccounts.mockResolvedValue({
      accounts: [{ id: "a", kind: "offline", username: "polispol1", profileUuid: "u" }],
      activeId: "a",
    });
    renderHome();
    expect(await screen.findByText("polispol1")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: esCommon.home.addAccount })).not.toBeInTheDocument();
  });

  it("lists the player's instances", async () => {
    client.listInstances.mockResolvedValue([
      {
        id: "survival",
        name: "Survival",
        gameVersion: "26.2",
        loader: "vanilla",
        loaderVersion: null,
        origin: "custom",
        playtimeSecs: 7_200,
      },
    ]);
    renderHome();
    expect(await screen.findByRole("link", { name: "Survival" })).toHaveAttribute(
      "href",
      "/instances",
    );
    expect(screen.getByText("26.2 · 2 h 0 min")).toBeInTheDocument();
    expect(screen.queryByText(esCommon.home.instances.empty)).not.toBeInTheDocument();
  });

  it("shows the status note and no lamp when the status is unknown", async () => {
    const { container } = renderHome();
    expect(await screen.findByText(esCommon.home.status.unavailable)).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).toBeNull();
  });

  it("says the server does not answer when the ping is unreachable", async () => {
    client.getServerStatus.mockRejectedValue(new IpcError("server_unreachable", "timed out"));
    const { container } = renderHome();
    expect(await screen.findByText(esCommon.home.status.offline)).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).toBeNull();
  });

  it("lights the lamp and shows the live readings when the server answers", async () => {
    client.getServerStatus.mockResolvedValue({
      onlinePlayers: 8,
      maxPlayers: 100,
      versionName: "Velocity",
      protocol: 777,
      motd: "Welcome to CINEPOL",
      latencyMs: 42,
    });
    const { container } = renderHome();
    expect(await screen.findByText("8 / 100")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
    expect(screen.getByText("Welcome to CINEPOL")).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).not.toBeNull();
  });

  it("uses the remote config when the backend provides one", async () => {
    client.getServerConfig.mockResolvedValue({
      name: "CINEPOL",
      address: "play.example.test",
      minVersion: "1.21.11",
      maxVersion: "26.2",
      recommendedVersion: "26.2",
      links: serverConfig.links,
      origin: "remote",
    });
    renderHome();
    expect(await screen.findByText("play.example.test")).toBeInTheDocument();
  });

  it("labels a running session as playing", async () => {
    setSession("running");
    renderHome();
    expect(await screen.findByText(esCommon.home.session.playing)).toBeInTheDocument();
  });

  it("hides the last readings once the server stops answering", async () => {
    client.getServerStatus.mockResolvedValue(liveStatus);
    const { client: queries } = renderHome();
    expect(await screen.findByText("8 / 100")).toBeInTheDocument();

    client.getServerStatus.mockRejectedValue(new IpcError("server_unreachable", "timed out"));
    await act(() => queries.refetchQueries());

    expect(await screen.findByText(esCommon.home.status.offline)).toBeInTheDocument();
    expect(screen.queryByText("8 / 100")).not.toBeInTheDocument();
    expect(screen.queryByText("Welcome to CINEPOL")).not.toBeInTheDocument();
  });

  it("refreshes the instance list once a session has created the instance", async () => {
    renderHome();
    expect(await screen.findByText(esCommon.home.instances.empty)).toBeInTheDocument();
    client.listInstances.mockResolvedValue([
      {
        id: "cinepol",
        name: "CINEPOL",
        gameVersion: "26.2",
        loader: "vanilla",
        loaderVersion: null,
        origin: "cinepolServer",
        playtimeSecs: 0,
      },
    ]);
    act(() => setSession("installing"));
    act(() => setSession("running"));
    expect(await screen.findByRole("link", { name: "CINEPOL" })).toBeInTheDocument();
  });

  it("keeps the account in the same spec sheet as the server and version", async () => {
    client.listAccounts.mockResolvedValue({
      accounts: [{ id: "a", kind: "offline", username: "CinepolTest7319", profileUuid: "u" }],
      activeId: "a",
    });
    renderHome();
    const account = await screen.findByText("CinepolTest7319");
    expect(account.closest("dl")).toBe(screen.getByText(serverConfig.server.address).closest("dl"));
  });

  it("asks to sign in again when the Microsoft session expired", async () => {
    useActivityStore.setState({
      byInstance: {
        [CINEPOL_SESSION]: {
          activity: { kind: "idle" },
          logs: [],
          error: "session_expired",
          lastExit: null,
        },
      },
    });
    renderHome();
    expect(await screen.findByText(esCommon.errors.session_expired)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: esCommon.home.signInAgain })).toHaveAttribute(
      "href",
      "/profile/new",
    );
  });

  it("offers to open the crash report after a Play on CINEPOL crash", async () => {
    const report = "crash-2026-10-19_12.00.00-client.txt";
    client.openCrashReport.mockResolvedValue(undefined);
    useActivityStore.setState({
      byInstance: {
        [CINEPOL_SESSION]: {
          activity: { kind: "idle" },
          logs: [],
          error: null,
          lastExit: {
            instanceId: "cinepol",
            exitCode: 255,
            killed: false,
            playedSecs: 30,
            crashReport: report,
          },
        },
      },
    });
    renderHome();
    expect(await screen.findByText(esCommon.instances.crashed)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: esCommon.instances.openCrashReport }));
    expect(client.openCrashReport).toHaveBeenCalledWith("cinepol", report);
  });
});
