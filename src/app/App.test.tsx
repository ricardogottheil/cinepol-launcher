import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import enCommon from "../i18n/locales/en/common.json";
import esCommon from "../i18n/locales/es/common.json";
import { App } from "./App";
import { createAppRouter } from "./router";

const listAccounts = vi.hoisted(() => vi.fn(async () => ({ accounts: [], activeId: null })));
const listInstances = vi.hoisted(() => vi.fn(async () => []));
const openExternal = vi.hoisted(() => vi.fn(async (_url: string) => {}));
const getAppInfo = vi.hoisted(() =>
  vi.fn(async () => ({ name: "Cinepol Launcher", version: "0.1.0" })),
);

vi.mock("../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../ipc/client")>()),
  getAppInfo,
  openExternal,
  listAccounts,
  listInstances,
}));

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  return render(<App router={router} />);
}

describe("App shell", () => {
  it("renders the main navigation in Spanish by default, grouped by section", async () => {
    renderAt("/");
    const nav = await screen.findByRole("navigation", { name: esCommon.chrome.mainNavigation });
    const groups: [string, string[]][] = [
      [esCommon.navGroups.game, [esCommon.nav.home, esCommon.nav.instances, esCommon.nav.modpacks]],
      [esCommon.navGroups.player, [esCommon.nav.profile]],
      [esCommon.navGroups.launcher, [esCommon.nav.settings]],
    ];
    for (const [group, items] of groups) {
      const list = within(nav).getByRole("list", { name: group });
      for (const item of items) {
        expect(within(list).getByRole("link", { name: item })).toBeInTheDocument();
      }
    }
    expect(screen.queryByText(enCommon.nav.instances)).not.toBeInTheDocument();
  });

  it("marks the current section", async () => {
    renderAt("/profile");
    const current = await screen.findByRole("link", { current: "page" });
    expect(current).toHaveTextContent(esCommon.nav.profile);
  });

  it("shows the legal disclaimer in the settings' about section", async () => {
    renderAt("/settings");
    const about = await screen.findByRole("region", { name: esCommon.settings.aboutTitle });
    expect(within(about).getByText(esCommon.legal.disclaimer)).toBeInTheDocument();
  });

  it("shows the community links from config", async () => {
    renderAt("/");
    expect(await screen.findByRole("link", { name: esCommon.links.twitch })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: esCommon.links.twitch })).toHaveAttribute(
      "href",
      "https://twitch.tv/polispol1",
    );
    expect(screen.getByRole("link", { name: esCommon.links.discord })).toHaveAttribute(
      "href",
      "https://discord.gg/cinepol",
    );
  });

  it("shows the app version from the backend", async () => {
    getAppInfo.mockResolvedValue({ name: "Cinepol Launcher", version: "0.1.0" });
    renderAt("/settings");
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
  });

  it("opens community links in the browser and says so when it cannot", async () => {
    renderAt("/");
    const twitch = await screen.findByRole("link", { name: esCommon.links.twitch });
    fireEvent.click(twitch);
    await waitFor(() => expect(openExternal).toHaveBeenCalledWith("https://twitch.tv/polispol1"));
    expect(screen.queryByText(esCommon.links.openFailed)).not.toBeInTheDocument();

    openExternal.mockRejectedValueOnce(new Error("denied"));
    fireEvent.click(twitch);
    expect(await screen.findByText(esCommon.links.openFailed)).toBeInTheDocument();
  });
});
