import { create } from "zustand";
import {
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
  /** Starts a sign-in unless one is already running; `onSignedIn` runs on success. */
  start: (onSignedIn: () => void) => void;
}

/**
 * The Microsoft sign-in lives outside the Accounts page, so leaving the page
 * keeps the code on screen when coming back and never starts a second sign-in.
 */
export const useSignInStore = create<SignInState>((set, get) => ({
  signIn: { kind: "idle", error: null },
  start: (onSignedIn) => {
    if (get().signIn.kind !== "idle") return;
    const operation = loginMicrosoft((code) =>
      set({ signIn: { kind: "waiting", code, cancel: operation.cancel } }),
    );
    set({ signIn: { kind: "starting", cancel: operation.cancel } });
    operation.done.then(
      () => {
        set({ signIn: { kind: "idle", error: null } });
        onSignedIn();
      },
      (error: unknown) => {
        const code = toIpcError(error).code;
        set({ signIn: { kind: "idle", error: code === "cancelled" ? null : code } });
      },
    );
  },
}));
