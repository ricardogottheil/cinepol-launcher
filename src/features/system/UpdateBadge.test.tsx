import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { renderWithQuery } from "../../test/render";
import { UpdateBadge } from "./UpdateBadge";

const client = vi.hoisted(() => ({ checkAppUpdate: vi.fn() }));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

function renderBadge() {
  const rootRoute = createRootRoute({ component: UpdateBadge });
  const router = createRouter({ routeTree: rootRoute, history: createMemoryHistory() });
  return renderWithQuery(<RouterProvider router={router} />);
}

describe("UpdateBadge", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("announces a newer launcher and links to the update page", async () => {
    client.checkAppUpdate.mockResolvedValue({
      kind: "available",
      version: "0.3.0",
      currentVersion: "0.2.0",
    });
    renderBadge();
    const link = await screen.findByRole("link", {
      name: new RegExp(esCommon.updates.seeWhatsNew),
    });
    expect(link).toHaveAttribute("href", "/updates");
    expect(link).toHaveTextContent(esCommon.updates.badge.replace("{{version}}", "0.3.0"));
  });

  it.each([{ kind: "upToDate" }, { kind: "notConfigured" }])(
    "shows nothing when the check says $kind",
    async (status) => {
      client.checkAppUpdate.mockResolvedValue(status);
      const { client: queries } = renderBadge();
      await waitFor(() => expect(queries.getQueryData(["app-update"])).toEqual(status));
      expect(screen.queryByRole("link")).toBeNull();
    },
  );
});
