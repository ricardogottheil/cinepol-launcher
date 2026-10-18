import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import esCommon from "../../i18n/locales/es/common.json";
import { type DeviceCodeDto, IpcError } from "../../ipc/client";
import { renderWithQuery } from "../../test/render";
import { AccountsPage } from "./AccountsPage";
import { useSignInStore } from "./signInStore";

const client = vi.hoisted(() => ({
  listAccounts: vi.fn(),
  createOfflineAccount: vi.fn(),
  setActiveAccount: vi.fn(),
  checkPremiumName: vi.fn(),
  getAccountPolicy: vi.fn(),
  loginMicrosoft: vi.fn(),
  removeAccount: vi.fn(),
  getAccountSkin: vi.fn(),
  openExternal: vi.fn(),
}));

vi.mock("./SkinViewer", () => ({
  SkinViewer: ({
    texture,
    slim,
    label,
  }: {
    texture: string | null;
    slim: boolean;
    label: string;
  }) => <div role="img" aria-label={label} data-texture={texture ?? ""} data-slim={String(slim)} />,
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
  client.getAccountPolicy.mockResolvedValue({
    microsoftAvailable: true,
    requirePremiumForOffline: false,
  });
  client.getAccountSkin.mockResolvedValue(null);
  client.removeAccount.mockResolvedValue(null);
  client.openExternal.mockResolvedValue(undefined);
  useSignInStore.setState({ signIn: { kind: "idle", error: null } });
});

const code: DeviceCodeDto = {
  userCode: "ABCD-EFGH",
  verificationUri: "https://www.microsoft.com/link",
  expiresInSecs: 900,
};
const premium = { id: "9", kind: "microsoft", username: "Polispol1", profileUuid: "p" } as const;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Starts a sign-in whose code arrives at once; returns its controls. */
function scriptSignIn() {
  const done = deferred<typeof premium>();
  const cancel = vi.fn().mockResolvedValue(true);
  client.loginMicrosoft.mockImplementation((onCode: (code: DeviceCodeDto) => void) => {
    queueMicrotask(() => onCode(code));
    return { operationId: "op", done: done.promise, cancel };
  });
  return { done, cancel };
}

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

  it("signs in with Microsoft by showing the code and where to type it", async () => {
    const { done } = scriptSignIn();
    renderWithQuery(<AccountsPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));

    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.openLink }));
    expect(client.openExternal).toHaveBeenCalledWith("https://www.microsoft.com/link");

    client.listAccounts.mockResolvedValue({ accounts: [steve, premium], activeId: "1" });
    await act(async () => done.resolve(premium));
    await waitFor(() => expect(screen.queryByText("ABCD-EFGH")).not.toBeInTheDocument());
    expect(await screen.findByText("Polispol1")).toBeInTheDocument();
  });

  it("cancels a waiting sign-in and explains a refused one", async () => {
    const first = scriptSignIn();
    renderWithQuery(<AccountsPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));
    await screen.findByText("ABCD-EFGH");
    fireEvent.click(screen.getByRole("button", { name: labels.cancelSignIn }));
    expect(first.cancel).toHaveBeenCalledOnce();
    await act(async () => first.done.reject(new IpcError("cancelled", "cancelled")));
    expect(screen.queryByText(esCommon.errors.cancelled)).not.toBeInTheDocument();

    const second = scriptSignIn();
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));
    await act(async () => second.done.reject(new IpcError("app_not_approved", "403")));
    expect(await screen.findByText(esCommon.errors.app_not_approved)).toBeInTheDocument();
  });

  it("says when this build has no Microsoft sign-in", async () => {
    client.getAccountPolicy.mockResolvedValue({
      microsoftAvailable: false,
      requirePremiumForOffline: false,
    });
    renderWithQuery(<AccountsPage />);
    expect(await screen.findByText(labels.microsoftUnavailable)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.microsoftSignIn })).not.toBeInTheDocument();
  });

  it("removes an account after a confirmation", async () => {
    renderWithQuery(<AccountsPage />);
    const removeButtons = await screen.findAllByRole("button", { name: labels.remove });
    const alexRemove = removeButtons[1];
    if (!alexRemove) throw new Error("no remove button for the second account");
    fireEvent.click(alexRemove);
    expect(client.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: labels.confirmRemove }));
    await waitFor(() => expect(client.removeAccount).toHaveBeenCalledWith("2"));
  });

  it("gates offline profiles behind a premium account when the build asks for it", async () => {
    client.getAccountPolicy.mockResolvedValue({
      microsoftAvailable: true,
      requirePremiumForOffline: true,
    });
    renderWithQuery(<AccountsPage />);
    expect(await screen.findByText(labels.offlineRequiresPremium)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.create })).toBeDisabled();
  });

  it("previews the active Microsoft account's skin", async () => {
    client.listAccounts.mockResolvedValue({ accounts: [premium], activeId: "9" });
    client.getAccountSkin.mockResolvedValue({ dataUrl: "data:image/png;base64,AAA", slim: true });
    renderWithQuery(<AccountsPage />);
    const viewer = await screen.findByRole("img", { name: labels.skinLabel });
    await waitFor(() =>
      expect(viewer).toHaveAttribute("data-texture", "data:image/png;base64,AAA"),
    );
    expect(viewer).toHaveAttribute("data-slim", "true");
    expect(client.getAccountSkin).toHaveBeenCalledWith("9");
  });

  it("keeps a waiting sign-in across leaving the page and never starts a second one", async () => {
    scriptSignIn();
    const first = renderWithQuery(<AccountsPage />);
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    first.unmount();

    renderWithQuery(<AccountsPage />);
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.microsoftSignIn })).not.toBeInTheDocument();
    useSignInStore.getState().start(() => {});
    expect(client.loginMicrosoft).toHaveBeenCalledOnce();
  });
});
