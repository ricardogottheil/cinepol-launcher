import { createMemoryHistory } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import enCommon from "../i18n/locales/en/common.json";
import esCommon from "../i18n/locales/es/common.json";
import { App } from "./App";
import { createAppRouter } from "./router";

const getAppInfo = vi.hoisted(() =>
  vi.fn(async () => ({ name: "Cinepol Launcher", version: "0.1.0" })),
);

vi.mock("../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../ipc/client")>()),
  getAppInfo,
}));

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  return render(<App router={router} />);
}

describe("App shell", () => {
  it("renders the main navigation in Spanish by default", async () => {
    renderAt("/");
    const nav = await screen.findByRole("navigation", { name: esCommon.chrome.mainNavigation });
    for (const label of [esCommon.nav.home, esCommon.nav.instances, esCommon.nav.accounts]) {
      expect(nav).toHaveTextContent(label);
    }
    expect(screen.queryByText(enCommon.nav.instances)).not.toBeInTheDocument();
  });

  it("marks the current section", async () => {
    renderAt("/accounts");
    const current = await screen.findByRole("link", { current: "page" });
    expect(current).toHaveTextContent(esCommon.nav.accounts);
  });

  it("shows the legal disclaimer and the community links from config", async () => {
    renderAt("/");
    expect(await screen.findByText(esCommon.legal.disclaimer)).toBeInTheDocument();
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
    renderAt("/");
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
  });
});
