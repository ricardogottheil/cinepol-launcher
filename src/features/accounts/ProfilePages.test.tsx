import { createMemoryHistory } from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../../app/App";
import { createAppRouter } from "../../app/router";
import esCommon from "../../i18n/locales/es/common.json";
import { type DeviceCodeDto, IpcError } from "../../ipc/client";
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
const premium = { id: "9", kind: "microsoft", username: "Polispol1", profileUuid: "p" } as const;
const labels = esCommon.accounts;

beforeEach(() => {
  vi.clearAllMocks();
  client.listAccounts.mockResolvedValue({ accounts: [steve, alex, premium], activeId: "1" });
  client.checkPremiumName.mockResolvedValue("free");
  client.getAccountPolicy.mockResolvedValue({
    microsoftAvailable: true,
    requirePremiumForOffline: false,
  });
  client.getAccountSkin.mockResolvedValue(null);
  client.removeAccount.mockResolvedValue(null);
  client.setActiveAccount.mockResolvedValue(null);
  client.openExternal.mockResolvedValue(undefined);
  useSignInStore.setState({ signIn: { kind: "idle", error: null } });
});

function renderAt(path: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [path] }));
  render(<App router={router} />);
  return router;
}

const code: DeviceCodeDto = {
  userCode: "ABCD-EFGH",
  verificationUri: "https://www.microsoft.com/link",
  expiresInSecs: 900,
};

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

async function openSwitcher() {
  const button = await screen.findByRole("button", { name: labels.switcherLabel });
  await waitFor(() => expect(button).toHaveTextContent("Steve"));
  fireEvent.click(button);
  return { button, menu: screen.getByRole("menu", { name: labels.switcherLabel }) };
}

describe("profile switcher", () => {
  it("shows the active profile and switches between online and offline groups", async () => {
    renderAt("/profile");
    const { button, menu } = await openSwitcher();
    expect(button).toHaveAttribute("aria-expanded", "true");

    const online = within(menu).getByRole("group", { name: labels.onlineGroup });
    expect(within(online).getByRole("menuitemradio", { name: /Polispol1/ })).toBeInTheDocument();
    const offline = within(menu).getByRole("group", { name: labels.offlineGroup });
    expect(within(offline).getByRole("menuitemradio", { name: /Steve/ })).toHaveAttribute(
      "aria-checked",
      "true",
    );

    fireEvent.click(within(offline).getByRole("menuitemradio", { name: /Alex/ }));
    await waitFor(() => expect(client.setActiveAccount).toHaveBeenCalledWith("2"));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("closes with Escape and gives the focus back", async () => {
    renderAt("/profile");
    const { button, menu } = await openSwitcher();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });

  it("opens the page to add a profile", async () => {
    renderAt("/profile");
    const { menu } = await openSwitcher();
    fireEvent.click(within(menu).getByRole("menuitem", { name: labels.addProfile }));
    expect(
      await screen.findByRole("heading", { level: 1, name: labels.addTitle }),
    ).toBeInTheDocument();
  });

  it("offers to add a profile when there is none", async () => {
    client.listAccounts.mockResolvedValue({ accounts: [], activeId: null });
    renderAt("/profile");
    expect(await screen.findByText(labels.noProfile)).toBeInTheDocument();
    const links = screen.getAllByRole("link", { name: labels.addProfile });
    expect(links.length).toBeGreaterThan(0);
    fireEvent.click(links[0] as HTMLElement);
    expect(
      await screen.findByRole("heading", { level: 1, name: labels.addTitle }),
    ).toBeInTheDocument();
  });
});

describe("profile page", () => {
  it("shows the active Microsoft profile with its skin", async () => {
    client.listAccounts.mockResolvedValue({ accounts: [premium], activeId: "9" });
    client.getAccountSkin.mockResolvedValue({ dataUrl: "data:image/png;base64,AAA", slim: true });
    renderAt("/profile");
    expect(await screen.findByRole("heading", { level: 1, name: "Polispol1" })).toBeInTheDocument();
    const viewer = await screen.findByRole("img", { name: labels.skinLabel });
    await waitFor(() =>
      expect(viewer).toHaveAttribute("data-texture", "data:image/png;base64,AAA"),
    );
    expect(viewer).toHaveAttribute("data-slim", "true");
    expect(client.getAccountSkin).toHaveBeenCalledWith("9");
  });

  it("says that offline profiles use the default skin", async () => {
    renderAt("/profile");
    expect(await screen.findByRole("heading", { level: 1, name: "Steve" })).toBeInTheDocument();
    expect(screen.getByText(labels.skinOffline)).toBeInTheDocument();
  });

  it("removes the active profile after a confirmation", async () => {
    renderAt("/profile");
    fireEvent.click(await screen.findByRole("button", { name: labels.remove }));
    expect(client.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: labels.confirmRemove }));
    await waitFor(() => expect(client.removeAccount).toHaveBeenCalledWith("1"));
  });
});

describe("profile page keep", () => {
  it("keeps the profile when the removal is not confirmed", async () => {
    renderAt("/profile");
    fireEvent.click(await screen.findByRole("button", { name: labels.remove }));
    fireEvent.click(screen.getByRole("button", { name: labels.keep }));
    expect(screen.queryByRole("button", { name: labels.confirmRemove })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.remove })).toBeInTheDocument();
    expect(client.removeAccount).not.toHaveBeenCalled();
  });
});

describe("profile page confirmation", () => {
  it("never carries a pending removal over to another profile", async () => {
    renderAt("/profile");
    fireEvent.click(await screen.findByRole("button", { name: labels.remove }));
    expect(screen.getByRole("button", { name: labels.confirmRemove })).toBeInTheDocument();

    client.listAccounts.mockResolvedValue({ accounts: [steve, alex, premium], activeId: "2" });
    const { menu } = await openSwitcher();
    fireEvent.click(within(menu).getByRole("menuitemradio", { name: /Alex/ }));
    expect(await screen.findByRole("heading", { level: 1, name: "Alex" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.confirmRemove })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.remove })).toBeInTheDocument();
    expect(client.removeAccount).not.toHaveBeenCalled();
  });
});

describe("add profile page", () => {
  it("creates an offline profile with the trimmed name and offers to use it", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "Pol" });
    renderAt("/profile/new");
    await createAccount(" Pol ");
    await waitFor(() => expect(client.createOfflineAccount).toHaveBeenCalledWith("Pol"));
    expect(await screen.findByText(labels.added.replace("{{name}}", "Pol"))).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.useProfile }));
    await waitFor(() => expect(client.setActiveAccount).toHaveBeenCalledWith("3"));
  });

  it("explains a rejected name by its error code", async () => {
    client.createOfflineAccount.mockRejectedValue(
      new IpcError("already_exists", "an offline profile named Steve already exists"),
    );
    renderAt("/profile/new");
    await createAccount("Steve");
    expect(await screen.findByText(esCommon.errors.already_exists)).toBeInTheDocument();
  });

  it("warns, without blocking, when the new name belongs to a premium account", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "polispol1" });
    client.checkPremiumName.mockResolvedValue("premium");
    renderAt("/profile/new");
    await createAccount("polispol1");
    await waitFor(() => expect(client.createOfflineAccount).toHaveBeenCalledWith("polispol1"));
    expect(await screen.findByText(labels.premiumNameWarning)).toBeInTheDocument();
    expect(client.checkPremiumName).toHaveBeenCalledWith("polispol1");
  });

  it("shows no warning for a free name or when the lookup is unavailable", async () => {
    client.createOfflineAccount.mockResolvedValue({ ...alex, id: "3", username: "Pol" });
    renderAt("/profile/new");
    await createAccount("Pol");
    await waitFor(() => expect(client.checkPremiumName).toHaveBeenCalledWith("Pol"));
    client.checkPremiumName.mockResolvedValue("unknown");
    await createAccount("Pol");
    await waitFor(() => expect(client.checkPremiumName).toHaveBeenCalledTimes(2));
    expect(screen.queryByText(labels.premiumNameWarning)).not.toBeInTheDocument();
  });

  it("signs in with Microsoft by showing the code, then offers the new profile", async () => {
    const { done } = scriptSignIn();
    renderAt("/profile/new");
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));

    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.openLink }));
    expect(client.openExternal).toHaveBeenCalledWith("https://www.microsoft.com/link");

    await act(async () => done.resolve(premium));
    await waitFor(() => expect(screen.queryByText("ABCD-EFGH")).not.toBeInTheDocument());
    expect(
      await screen.findByText(labels.added.replace("{{name}}", "Polispol1")),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: labels.useProfile }));
    await waitFor(() => expect(client.setActiveAccount).toHaveBeenCalledWith("9"));
  });

  it("cancels a waiting sign-in and explains a refused one", async () => {
    const first = scriptSignIn();
    renderAt("/profile/new");
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
    renderAt("/profile/new");
    expect(await screen.findByText(labels.microsoftUnavailable)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.microsoftSignIn })).not.toBeInTheDocument();
  });

  it("gates offline profiles behind a premium account when the build asks for it", async () => {
    client.listAccounts.mockResolvedValue({ accounts: [steve], activeId: "1" });
    client.getAccountPolicy.mockResolvedValue({
      microsoftAvailable: true,
      requirePremiumForOffline: true,
    });
    renderAt("/profile/new");
    expect(await screen.findByText(labels.offlineRequiresPremium)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: labels.create })).toBeDisabled();
  });

  it("keeps a waiting sign-in across leaving the page and never starts a second one", async () => {
    scriptSignIn();
    const router = renderAt("/profile/new");
    fireEvent.click(await screen.findByRole("button", { name: labels.microsoftSignIn }));
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();

    await act(() => router.navigate({ to: "/profile" }));
    await waitFor(() => expect(screen.queryByText("ABCD-EFGH")).not.toBeInTheDocument());
    await act(() => router.navigate({ to: "/profile/new" }));
    expect(await screen.findByText("ABCD-EFGH")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: labels.microsoftSignIn })).not.toBeInTheDocument();
    useSignInStore.getState().start(() => {});
    expect(client.loginMicrosoft).toHaveBeenCalledOnce();
  });
});
