import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { renderWithQuery } from "../../test/render";
import { AboutPage } from "./AboutPage";

const client = vi.hoisted(() => ({
  getAppInfo: vi.fn(),
  checkAppUpdate: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

beforeEach(() => {
  client.getAppInfo.mockResolvedValue({ version: "0.2.0" });
  client.checkAppUpdate.mockResolvedValue({ kind: "upToDate" });
});

function renderAbout() {
  const rootRoute = createRootRoute({ component: AboutPage });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  return renderWithQuery(<RouterProvider router={router} />);
}

describe("AboutPage", () => {
  it("shows the version and the legal notice", async () => {
    renderAbout();
    expect(await screen.findByText("0.2.0")).toBeInTheDocument();
    expect(screen.getByText(esCommon.legal.disclaimer)).toBeInTheDocument();
  });

  it("says whether a newer launcher is out and links to the update page", async () => {
    client.checkAppUpdate.mockResolvedValue({
      kind: "available",
      version: "0.3.0",
      currentVersion: "0.2.0",
    });
    renderAbout();
    expect(
      await screen.findByText(esCommon.about.updateAvailable.replace("{{version}}", "0.3.0")),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: esCommon.about.openUpdates })).toHaveAttribute(
      "href",
      "/updates",
    );
  });

  it("does not offer to change where the server configuration comes from", async () => {
    renderAbout();
    await screen.findByText("0.2.0");
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.queryByText(/server-config\.json/)).not.toBeInTheDocument();
  });
});
