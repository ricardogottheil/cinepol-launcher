import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { IpcError } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { AccountsPage } from "./AccountsPage";

const client = vi.hoisted(() => ({
  listAccounts: vi.fn(),
  createOfflineAccount: vi.fn(),
  setActiveAccount: vi.fn(),
  checkPremiumName: vi.fn(),
}));

vi.mock("../../ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../ipc/client")>()),
  ...client,
}));

const steve = { id: "1", kind: "offline", username: "Steve", profileUuid: "u1" } as const;
const alex = { id: "2", kind: "offline", username: "Alex", profileUuid: "u2" } as const;
const labels = esCommon.accounts;

beforeEach(() => {
  vi.clearAllMocks();
  client.listAccounts.mockResolvedValue({ accounts: [steve, alex], activeId: "1" });
  client.checkPremiumName.mockResolvedValue("free");
});

async function createAccount(name: string) {
  fireEvent.change(await screen.findByLabelText(labels.usernameLabel), {
    target: { value: name },
  });
  fireEvent.click(screen.getByRole("button", { name: labels.create }));
}

describe("AccountsPage", () => {
  it("lists accounts and marks the active one", async () => {
    renderWithQuery(<AccountsPage />);
    expect(await screen.findByText("Steve")).toBeInTheDocument();
    expect(screen.getByText(labels.active)).toBeInTheDocument();
    expect(screen.getAllByText(labels.offline)).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: labels.makeActive }));
    await waitFor(() => expect(client.setActiveAccount).toHaveBeenCalledWith("2"));
  });

  it("creates an offline account with the trimmed name", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "Pol" });
    renderWithQuery(<AccountsPage />);
    fireEvent.change(await screen.findByLabelText(labels.usernameLabel), {
      target: { value: " Pol " },
    });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    await waitFor(() => expect(client.createOfflineAccount).toHaveBeenCalledWith("Pol"));
    await waitFor(() => expect(client.listAccounts).toHaveBeenCalledTimes(2));
  });

  it("explains a rejected name by its error code", async () => {
    client.createOfflineAccount.mockRejectedValue(
      new IpcError("already_exists", "an offline profile named Steve already exists"),
    );
    renderWithQuery(<AccountsPage />);
    fireEvent.change(await screen.findByLabelText(labels.usernameLabel), {
      target: { value: "Steve" },
    });
    fireEvent.click(screen.getByRole("button", { name: labels.create }));
    expect(await screen.findByText(esCommon.errors.already_exists)).toBeInTheDocument();
  });

  it("warns, without blocking, when the new name belongs to a premium account", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "polispol1" });
    client.checkPremiumName.mockResolvedValue("premium");
    renderWithQuery(<AccountsPage />);
    await createAccount("polispol1");
    await waitFor(() => expect(client.createOfflineAccount).toHaveBeenCalledWith("polispol1"));
    expect(await screen.findByText(labels.premiumNameWarning)).toBeInTheDocument();
    expect(client.checkPremiumName).toHaveBeenCalledWith("polispol1");
  });

  it("shows no warning for a free name or when the lookup is unavailable", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "Pol" });
    renderWithQuery(<AccountsPage />);
    await createAccount("Pol");
    await waitFor(() => expect(client.checkPremiumName).toHaveBeenCalledWith("Pol"));
    client.checkPremiumName.mockResolvedValue("unknown");
    await createAccount("Pol");
    await waitFor(() => expect(client.checkPremiumName).toHaveBeenCalledTimes(2));
    expect(screen.queryByText(labels.premiumNameWarning)).not.toBeInTheDocument();
  });
});
