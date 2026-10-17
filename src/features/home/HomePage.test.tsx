import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import serverConfig from "../../../config/server-config.json";
import esCommon from "../../i18n/locales/es/common.json";
import { renderWithQuery } from "../../test/render";
import { HomePage, type HomePageProps } from "./HomePage";

const client = vi.hoisted(() => ({ listAccounts: vi.fn(), listInstances: vi.fn() }));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

beforeEach(() => {
  client.listAccounts.mockResolvedValue({ accounts: [], activeId: null });
  client.listInstances.mockResolvedValue([]);
});

function renderHome(props: HomePageProps = {}) {
  const rootRoute = createRootRoute({ component: () => <HomePage {...props} /> });
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
      "/accounts",
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
    const { container } = renderHome({ status: "unavailable" });
    expect(await screen.findByText(esCommon.home.status.unavailable)).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).toBeNull();
  });

  it("lights the lamp and shows the count when the server answers", async () => {
    const { container } = renderHome({ status: "online", onlinePlayers: 8 });
    expect(await screen.findByText("8")).toBeInTheDocument();
    expect(container.querySelector("[data-status-dot]")).not.toBeNull();
  });
});
