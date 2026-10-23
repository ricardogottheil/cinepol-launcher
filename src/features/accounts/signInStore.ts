import { create } from "zustand";
import {
  type AccountDto,
  type ClientErrorCode,
  type DeviceCodeDto,
  loginMicrosoft,
  toIpcError,
} from "../../ipc/client";

export type MicrosoftSignIn =
  | { kind: "idle"; error: ClientErrorCode | null }
  | { kind: "starting"; cancel: () => Promise<boolean> }
  | { kind: "waiting"; code: DeviceCodeDto; cancel: () => Promise<boolean> };

interface SignInState {
  signIn: MicrosoftSignIn;
  /** Starts a sign-in unless one is already running; `onSignedIn` gets the new account. */
  start: (onSignedIn: (account: AccountDto) => void) => void;
}

/**
 * The Microsoft sign-in lives outside the add-profile page, so leaving the page
 * keeps the code on screen when coming back and never starts a second sign-in.
 */
export const useSignInStore = create<SignInState>((set, get) => ({
  signIn: { kind: "idle", error: null },
  start: (onSignedIn) => {
    if (get().signIn.kind !== "idle") return;
    // The code may arrive before `loginMicrosoft` returns, so the callback
    // cancels through this handle instead of the not-yet-assigned operation.
    const handle = { cancel: async () => false };
    const cancel = () => handle.cancel();
    set({ signIn: { kind: "starting", cancel } });
    const operation = loginMicrosoft((code) => set({ signIn: { kind: "waiting", code, cancel } }));
    handle.cancel = operation.cancel;
    operation.done.then(
      (account) => {
        set({ signIn: { kind: "idle", error: null } });
        onSignedIn(account);
      },
      (error: unknown) => {
        const code = toIpcError(error).code;
        set({ signIn: { kind: "idle", error: code === "cancelled" ? null : code } });
      },
    );
  },
}));
